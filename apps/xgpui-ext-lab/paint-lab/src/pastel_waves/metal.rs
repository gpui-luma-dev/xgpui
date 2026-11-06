//! Native Metal producer for pastel waves: a compute pass evaluates every
//! ribbon curve once per pixel column, then one fragment pass shades the
//! target. GPUI composites the result in its main pass without a pass break.
use super::native::{PastelWavesNative, Uniforms};
use anyhow::{Context as _, Result};
use gpui::{DevicePixels, Size};
use gpui_apple::custom_draw::{MetalCustomContext, MetalCustomOutput, MetalQueueIdentity};
use gpui_apple::metal::*;
use std::{
    any::Any,
    sync::{Arc, Mutex},
};

struct Pipelines {
    columns: ComputePipelineState,
    shade: RenderPipelineState,
}
struct Cached {
    key: u64,
    pipelines: Arc<Pipelines>,
    _queue: MetalQueueIdentity,
}
struct PipelineCache(Mutex<Option<Cached>>);
// SAFETY: pipeline states are immutable; the mutex serializes replacement and
// the entry retains its queue so the key cannot be recycled.
unsafe impl Send for PipelineCache {}
unsafe impl Sync for PipelineCache {}
static PIPELINES: PipelineCache = PipelineCache(Mutex::new(None));

/// Compile once per host device/queue.
fn pipelines(ctx: &MetalCustomContext) -> Result<Arc<Pipelines>> {
    let mut cached = PIPELINES
        .0
        .lock()
        .map_err(|_| anyhow::anyhow!("pastel waves pipeline cache poisoned"))?;
    if let Some(entry) = cached.as_ref()
        && entry.key == ctx.cache_key()
    {
        return Ok(entry.pipelines.clone());
    }
    let library = ctx
        .device
        .newLibraryWithSource_options_error(
            &objc2_foundation::NSString::from_str(&super::metal_source()),
            Some(&CompileOptions::new()),
        )
        .map_err(anyhow::Error::msg)
        .context("compile pastel_waves.metal")?;
    let function = |name: &str| -> Result<Function> {
        library
            .newFunctionWithName(&objc2_foundation::NSString::from_str(name))
            .ok_or_else(|| anyhow::anyhow!("Metal function {name} missing"))
    };
    let column_function = function("pastel_columns")?;
    let vertex = function("pastel_vertex")?;
    let fragment = function("pastel_fragment")?;
    let columns = ctx
        .device
        .newComputePipelineStateWithFunction_error(&column_function)
        .map_err(anyhow::Error::msg)?;
    let descriptor = RenderPipelineDescriptor::new();
    descriptor.setVertexFunction(Some(&vertex));
    descriptor.setFragmentFunction(Some(&fragment));
    unsafe { descriptor.colorAttachments().objectAtIndexedSubscript(0) }
        .setPixelFormat(MTLPixelFormat::BGRA8Unorm);
    let shade = ctx
        .device
        .newRenderPipelineStateWithDescriptor_error(&descriptor)
        .map_err(anyhow::Error::msg)?;
    let pipelines = Arc::new(Pipelines { columns, shade });
    *cached = Some(Cached {
        key: ctx.cache_key(),
        pipelines: pipelines.clone(),
        _queue: ctx.queue_identity(),
    });
    Ok(pipelines)
}

/// Record both passes into the host batch and return the output texture.
pub(super) fn prepare(
    waves: &PastelWavesNative,
    ctx: &MetalCustomContext,
    extent: Size<DevicePixels>,
) -> Result<Box<dyn Any>> {
    let pipelines = pipelines(ctx)?;
    let width = extent.width.0.max(1) as usize;
    let height = extent.height.0.max(1) as usize;

    let descriptor = TextureDescriptor::new();
    descriptor.setTextureType(MTLTextureType::Type2D);
    descriptor.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
    unsafe { descriptor.setWidth(width) };
    unsafe { descriptor.setHeight(height) };
    descriptor.setStorageMode(MTLStorageMode::Private);
    descriptor.setUsage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead);
    let texture = ctx
        .device
        .newTextureWithDescriptor(&descriptor)
        .context("Metal texture allocation failed")?;
    // 9 ribbons x 2 float4 per column, written and read only on the GPU.
    let columns = ctx
        .device
        .newBufferWithLength_options(width * 9 * 2 * 16, MTLResourceOptions::StorageModePrivate)
        .context("Metal buffer allocation failed")?;

    let uniforms = waves.uniforms(width, height);
    let uniform_bytes =
        std::ptr::NonNull::new((&uniforms as *const Uniforms).cast::<std::ffi::c_void>() as *mut _)
            .expect("non-null bytes");

    let command = ctx.record(|command| {
        let compute = command
            .computeCommandEncoder()
            .context("Metal compute encoder allocation failed")?;
        compute.setComputePipelineState(&pipelines.columns);
        unsafe { compute.setBuffer_offset_atIndex(Some(&*columns), 0, 0) };
        unsafe {
            compute.setBytes_length_atIndex(uniform_bytes, std::mem::size_of::<Uniforms>(), 1)
        };
        let group = pipelines
            .columns
            .threadExecutionWidth()
            .min(pipelines.columns.maxTotalThreadsPerThreadgroup())
            .max(1);
        compute.dispatchThreadgroups_threadsPerThreadgroup(
            MTLSize {
                width: width.div_ceil(group),
                height: 1,
                depth: 1,
            },
            MTLSize {
                width: group,
                height: 1,
                depth: 1,
            },
        );
        // Tracked resources order the column writes before the fragment reads.
        compute.endEncoding();

        let pass = RenderPassDescriptor::new();
        let attachment = unsafe { pass.colorAttachments().objectAtIndexedSubscript(0) };
        attachment.setTexture(Some(&*texture));
        attachment.setLoadAction(MTLLoadAction::DontCare);
        attachment.setStoreAction(MTLStoreAction::Store);
        let encoder = command
            .renderCommandEncoderWithDescriptor(&pass)
            .context("Metal render encoder allocation failed")?;
        encoder.setRenderPipelineState(&pipelines.shade);
        unsafe {
            encoder.setFragmentBytes_length_atIndex(
                uniform_bytes,
                std::mem::size_of::<Uniforms>(),
                0,
            )
        };
        unsafe { encoder.setFragmentBuffer_offset_atIndex(Some(&*columns), 0, 1) };
        unsafe { encoder.drawPrimitives_vertexStart_vertexCount(MTLPrimitiveType::Triangle, 0, 3) };
        encoder.endEncoding();
        Ok(())
    })?;
    Ok(Box::new(MetalCustomOutput {
        texture,
        commands: vec![command],
    }))
}
