use super::MeshGradient;
use ::metal::*;
use anyhow::{Result, bail, ensure};
use gpui::{CustomContent, DevicePixels, Size};
use gpui_apple::custom_draw::{MetalCustomContext, MetalCustomOutput};
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
pub(super) struct CachedOutput {
    key: u64,
    extent: Size<DevicePixels>,
    pub texture: Texture,
    command: CommandBuffer,
    _queue: CommandQueue,
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
            .new_library_with_source(include_str!("mesh.metal"), &CompileOptions::new())
            .map_err(anyhow::Error::msg)?;
        let compute = |name| -> Result<ComputePipelineState> {
            let function = library
                .get_function(name, None)
                .map_err(anyhow::Error::msg)?;
            ctx.device
                .new_compute_pipeline_state_with_function(&function)
                .map_err(anyhow::Error::msg)
        };
        let vertex = library
            .get_function("mesh_vertex", None)
            .map_err(anyhow::Error::msg)?;
        let fragment = library
            .get_function("mesh_fragment", None)
            .map_err(anyhow::Error::msg)?;
        let descriptor = RenderPipelineDescriptor::new();
        descriptor.set_vertex_function(Some(&vertex));
        descriptor.set_fragment_function(Some(&fragment));
        descriptor
            .color_attachments()
            .object_at(0)
            .unwrap()
            .set_pixel_format(MTLPixelFormat::BGRA8Unorm);
        let pipelines = Arc::new(Pipelines {
            coefficients: compute("mesh_coefficients")?,
            evaluate: compute("mesh_evaluate")?,
            triangles: compute("mesh_triangles")?,
            draw: ctx
                .device
                .new_render_pipeline_state(&descriptor)
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
    let encoder = command.new_compute_command_encoder();
    encoder.set_compute_pipeline_state(pipeline);
    encoder.set_buffer(0, Some(input), 0);
    encoder.set_buffer(1, Some(output), 0);
    encoder.set_bytes(
        2,
        std::mem::size_of_val(params) as u64,
        (params as *const Params).cast(),
    );
    let group = pipeline
        .thread_execution_width()
        .min(pipeline.max_total_threads_per_threadgroup());
    encoder.dispatch_thread_groups(
        MTLSize::new((count as u64).div_ceil(group), 1, 1),
        MTLSize::new(group, 1, 1),
    );
    // Separate tracked-resource encoders order writes before the next stage.
    encoder.end_encoding();
}
impl CustomContent for MeshGradient {
    fn prepare(&self, backend: &dyn Any, size: Size<DevicePixels>) -> Result<Box<dyn Any>> {
        let Some(ctx) = backend.downcast_ref::<MetalCustomContext>() else {
            bail!("MeshGradient requires the Metal backend");
        };
        ensure!(
            size.width.0 > 0
                && size.height.0 > 0
                && size.width.0 <= 16384
                && size.height.0 <= 16384,
            "invalid mesh texture size"
        );
        ensure!(
            size.width.0 as u64 * size.height.0 as u64 * 4 <= 64 * 1024 * 1024,
            "mesh output exceeds 64 MiB texture budget"
        );
        let mut cached = self
            .output
            .lock()
            .map_err(|_| anyhow::anyhow!("mesh output cache poisoned"))?;
        if let Some(cache) = cached.as_ref()
            && cache.key == ctx.cache_key()
            && cache.extent == size
        {
            let commands = match cache.command.status() {
                MTLCommandBufferStatus::NotEnqueued => vec![cache.command.clone()],
                MTLCommandBufferStatus::Error => bail!("mesh preparation failed on the GPU"),
                MTLCommandBufferStatus::Enqueued => bail!("mesh command enqueued outside the host"),
                _ => vec![],
            };
            return Ok(Box::new(MetalCustomOutput {
                texture: cache.texture.clone(),
                commands,
            }));
        }
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
        let controls = ctx.device.new_buffer_with_data(
            upload.as_ptr().cast(),
            std::mem::size_of_val(upload.as_slice()) as u64,
            MTLResourceOptions::StorageModeShared,
        );
        let patches = ctx.device.new_buffer(
            self.stats.patches as u64 * 320,
            MTLResourceOptions::StorageModePrivate,
        );
        let vertices = ctx.device.new_buffer(
            self.stats.evaluated_vertices as u64 * 32,
            MTLResourceOptions::StorageModePrivate,
        );
        let triangles = ctx.device.new_buffer(
            self.stats.triangles as u64 * 3 * 32,
            MTLResourceOptions::StorageModePrivate,
        );
        let descriptor = TextureDescriptor::new();
        descriptor.set_texture_type(MTLTextureType::D2);
        descriptor.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
        descriptor.set_width(size.width.0 as u64);
        descriptor.set_height(size.height.0 as u64);
        descriptor.set_storage_mode(MTLStorageMode::Private);
        descriptor.set_usage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead);
        let texture = ctx.device.new_texture(&descriptor);
        let command = ctx.queue.new_command_buffer().to_owned();
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
        let attachment = pass.color_attachments().object_at(0).unwrap();
        attachment.set_texture(Some(&texture));
        attachment.set_load_action(MTLLoadAction::Clear);
        attachment.set_store_action(MTLStoreAction::Store);
        attachment.set_clear_color(MTLClearColor::new(0., 0., 0., 0.));
        let encoder = command.new_render_command_encoder(pass);
        encoder.set_render_pipeline_state(&pipelines.draw);
        encoder.set_cull_mode(MTLCullMode::None);
        encoder.set_viewport(MTLViewport {
            originX: 0.,
            originY: 0.,
            width: size.width.0 as f64,
            height: size.height.0 as f64,
            znear: 0.,
            zfar: 1.,
        });
        encoder.set_vertex_buffer(0, Some(&triangles), 0);
        encoder.draw_primitives(
            MTLPrimitiveType::Triangle,
            0,
            self.stats.triangles as u64 * 3,
        );
        encoder.end_encoding();
        *cached = Some(CachedOutput {
            key: ctx.cache_key(),
            extent: size,
            texture: texture.clone(),
            command: command.clone(),
            _queue: ctx.queue.clone(),
        });
        self.preparations.fetch_add(1, Ordering::Relaxed);
        Ok(Box::new(MetalCustomOutput {
            texture,
            commands: vec![command],
        }))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
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
