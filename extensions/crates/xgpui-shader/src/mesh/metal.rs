use super::MeshGradient;
use anyhow::{Result, bail};
use gpui::{DevicePixels, Size};
use gpui_apple::custom_draw::MetalCustomContext;
use gpui_apple::metal::*;
use std::{
    any::Any,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

#[derive(Default)]
pub(super) struct PipelineCache {
    cached: Mutex<Option<CachedPipelines>>,
    pub compilations: AtomicUsize,
}
struct CachedPipelines {
    key: u64,
    pipelines: Arc<Pipelines>,
    _queue: CommandQueue,
}
struct Pipelines {
    coefficients: ComputePipelineState,
    evaluate: ComputePipelineState,
    triangles: ComputePipelineState,
    draw: RenderPipelineState,
}
// GPU-shared types use float4/uint4-sized records, with explicit alignment.
#[repr(C, align(16))]
struct UploadControl {
    position_and_u: [f32; 4],
    tangent_v: [f32; 4],
    color: [f32; 4],
}
#[repr(C, align(16))]
struct Params {
    width: u32,
    height: u32,
    samples: u32,
    patches: u32,
}

impl PipelineCache {
    fn get(&self, ctx: &MetalCustomContext) -> Result<Arc<Pipelines>> {
        let mut cached = self
            .cached
            .lock()
            .map_err(|_| anyhow::anyhow!("mesh pipeline cache poisoned"))?;
        if let Some(cache) = cached.as_ref()
            && cache.key == ctx.cache_key()
        {
            return Ok(cache.pipelines.clone());
        }
        let library = ctx
            .device
            .newLibraryWithSource_options_error(
                &objc2_foundation::NSString::from_str(include_str!("mesh.metal")),
                Some(&CompileOptions::new()),
            )
            .map_err(anyhow::Error::msg)?;
        let compute = |name| -> Result<ComputePipelineState> {
            let function = library
                .newFunctionWithName(&objc2_foundation::NSString::from_str(name))
                .ok_or_else(|| anyhow::anyhow!("Metal function missing"))
                .map_err(anyhow::Error::msg)?;
            ctx.device
                .newComputePipelineStateWithFunction_error(&function)
                .map_err(anyhow::Error::msg)
        };
        let vertex = library
            .newFunctionWithName(&objc2_foundation::NSString::from_str("mesh_vertex"))
            .ok_or_else(|| anyhow::anyhow!("Metal function missing"))
            .map_err(anyhow::Error::msg)?;
        let fragment = library
            .newFunctionWithName(&objc2_foundation::NSString::from_str("mesh_fragment"))
            .ok_or_else(|| anyhow::anyhow!("Metal function missing"))
            .map_err(anyhow::Error::msg)?;
        let descriptor = RenderPipelineDescriptor::new();
        descriptor.setVertexFunction(Some(&vertex));
        descriptor.setFragmentFunction(Some(&fragment));
        unsafe { descriptor.colorAttachments().objectAtIndexedSubscript(0) }
            .setPixelFormat(MTLPixelFormat::BGRA8Unorm);
        let pipelines = Arc::new(Pipelines {
            coefficients: compute("mesh_coefficients")?,
            evaluate: compute("mesh_evaluate")?,
            triangles: compute("mesh_triangles")?,
            draw: ctx
                .device
                .newRenderPipelineStateWithDescriptor_error(&descriptor)
                .map_err(anyhow::Error::msg)?,
        });
        *cached = Some(CachedPipelines {
            key: ctx.cache_key(),
            pipelines: pipelines.clone(),
            _queue: ctx.queue.clone(),
        });
        self.compilations.fetch_add(1, Ordering::Relaxed);
        Ok(pipelines)
    }
}
fn compute(
    command: &CommandBufferRef,
    pipeline: &ComputePipelineStateRef,
    input: &BufferRef,
    output: &BufferRef,
    params: &Params,
    count: u32,
) {
    let encoder = command
        .computeCommandEncoder()
        .expect("Metal compute encoder allocation failed");
    encoder.setComputePipelineState(pipeline);
    unsafe { encoder.setBuffer_offset_atIndex(Some(input), 0, 0) };
    unsafe { encoder.setBuffer_offset_atIndex(Some(output), 0, 1) };
    unsafe {
        encoder.setBytes_length_atIndex(
            std::ptr::NonNull::new((params as *const Params).cast::<std::ffi::c_void>() as *mut _)
                .expect("non-null bytes"),
            std::mem::size_of_val(params) as usize,
            2,
        )
    };
    let group = pipeline
        .threadExecutionWidth()
        .min(pipeline.maxTotalThreadsPerThreadgroup());
    encoder.dispatchThreadgroups_threadsPerThreadgroup(
        MTLSize {
            width: (count as usize).div_ceil(group),
            height: 1,
            depth: 1,
        },
        MTLSize {
            width: group,
            height: 1,
            depth: 1,
        },
    );
    // Separate tracked-resource encoders order writes before the next stage.
    encoder.endEncoding();
}
impl MeshGradient {
    pub(super) fn prepare_metal(
        &self,
        backend: &dyn Any,
        size: Size<DevicePixels>,
    ) -> Result<Box<dyn Any>> {
        let Some(ctx) = backend.downcast_ref::<MetalCustomContext>() else {
            bail!("MeshGradient requires the Metal backend");
        };
        let (output, generated) = self.output.prepare(ctx, size, || {
            let pipelines = self.pipelines.get(ctx)?;
            let upload: Vec<UploadControl> = self
                .grid
                .points
                .iter()
                .map(|p| UploadControl {
                    position_and_u: [p.position[0], p.position[1], p.u_tangent[0], p.u_tangent[1]],
                    tangent_v: [p.v_tangent[0], p.v_tangent[1], 0., 0.],
                    color: [p.color[0], p.color[1], p.color[2], 1.],
                })
                .collect();
            let controls = unsafe {
                ctx.device
                        .newBufferWithBytes_length_options(
                            std::ptr::NonNull::new(
                                upload.as_ptr().cast::<std::ffi::c_void>() as *mut _
                            )
                            .expect("non-null upload"),
                            std::mem::size_of_val(upload.as_slice()) as usize,
                            MTLResourceOptions::StorageModeShared,
                        )
                        .expect("Metal buffer allocation failed")
            };
            let patches = ctx
                .device
                .newBufferWithLength_options(
                    self.stats.patches as usize * 320,
                    MTLResourceOptions::StorageModePrivate,
                )
                .expect("Metal buffer allocation failed");
            let vertices = ctx
                .device
                .newBufferWithLength_options(
                    self.stats.evaluated_vertices as usize * 32,
                    MTLResourceOptions::StorageModePrivate,
                )
                .expect("Metal buffer allocation failed");
            let triangles = ctx
                .device
                .newBufferWithLength_options(
                    self.stats.triangles as usize * 3 * 32,
                    MTLResourceOptions::StorageModePrivate,
                )
                .expect("Metal buffer allocation failed");
            let descriptor = TextureDescriptor::new();
            descriptor.setTextureType(MTLTextureType::Type2D);
            descriptor.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
            unsafe { descriptor.setWidth(size.width.0 as usize) };
            unsafe { descriptor.setHeight(size.height.0 as usize) };
            descriptor.setStorageMode(MTLStorageMode::Private);
            descriptor.setUsage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead);
            let texture = ctx
                .device
                .newTextureWithDescriptor(&descriptor)
                .expect("Metal texture allocation failed");
            let command = ctx
                .queue
                .commandBuffer()
                .expect("Metal command buffer allocation failed")
                .to_owned();
            let params = Params {
                width: self.grid.width,
                height: self.grid.height,
                samples: self.samples,
                patches: self.stats.patches,
            };
            compute(
                &command,
                &pipelines.coefficients,
                &controls,
                &patches,
                &params,
                self.stats.patches,
            );
            compute(
                &command,
                &pipelines.evaluate,
                &patches,
                &vertices,
                &params,
                self.stats.evaluated_vertices,
            );
            compute(
                &command,
                &pipelines.triangles,
                &vertices,
                &triangles,
                &params,
                self.stats.triangles / 2,
            );
            let pass = RenderPassDescriptor::new();
            let attachment = unsafe { pass.colorAttachments().objectAtIndexedSubscript(0) };
            attachment.setTexture(Some(&texture));
            attachment.setLoadAction(MTLLoadAction::Clear);
            attachment.setStoreAction(MTLStoreAction::Store);
            attachment.setClearColor(MTLClearColor {
                red: 0.,
                green: 0.,
                blue: 0.,
                alpha: 0.,
            });
            let encoder = command
                .renderCommandEncoderWithDescriptor(&pass)
                .expect("Metal render encoder allocation failed");
            encoder.setRenderPipelineState(&pipelines.draw);
            encoder.setCullMode(MTLCullMode::None);
            encoder.setViewport(MTLViewport {
                originX: 0.,
                originY: 0.,
                width: size.width.0 as f64,
                height: size.height.0 as f64,
                znear: 0.,
                zfar: 1.,
            });
            unsafe { encoder.setVertexBuffer_offset_atIndex(Some(&triangles), 0, 0) };
            unsafe {
                encoder.drawPrimitives_vertexStart_vertexCount(
                    MTLPrimitiveType::Triangle,
                    0,
                    self.stats.triangles as usize * 3,
                )
            };
            encoder.endEncoding();
            Ok((texture, command))
        })?;
        if generated {
            self.preparations.fetch_add(1, Ordering::Relaxed);
        }
        Ok(Box::new(output))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use gpui::CustomContent;
    #[test]
    fn upload_layout_is_explicit_and_backend_errors_are_checked() {
        assert_eq!(std::mem::size_of::<UploadControl>(), 48);
        assert_eq!(std::mem::align_of::<UploadControl>(), 16);
        assert_eq!(std::mem::offset_of!(UploadControl, tangent_v), 16);
        assert_eq!(std::mem::offset_of!(UploadControl, color), 32);
        assert_eq!(std::mem::size_of::<Params>(), 16);
        let grid = crate::MeshGrid::regular(2, 2, vec![[0.; 3]; 4]).unwrap();
        assert!(
            MeshGradient::new(grid, 18)
                .unwrap()
                .prepare(&(), gpui::size(DevicePixels(16), DevicePixels(16)))
                .is_err()
        );
    }
}

// SAFETY: pipeline state is immutable; the cache mutex serializes replacement and retains its device/queue.
unsafe impl Send for PipelineCache {}
unsafe impl Sync for PipelineCache {}
