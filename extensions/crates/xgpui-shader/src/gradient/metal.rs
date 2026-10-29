use super::{Gradient, GradientKind};
use anyhow::{Result, bail};
use gpui::{DevicePixels, Size};
use gpui_apple::custom_draw::MetalCustomContext;
use gpui_apple::metal::*;
use std::{
    any::Any,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

#[derive(Default)]
pub(super) struct PipelineCache {
    cached: Mutex<Option<(u64, RenderPipelineState, CommandQueue)>>,
    pub compilations: AtomicUsize,
}
impl PipelineCache {
    fn get(&self, ctx: &MetalCustomContext) -> Result<RenderPipelineState> {
        let mut cached = self
            .cached
            .lock()
            .map_err(|_| anyhow::anyhow!("gradient pipeline cache poisoned"))?;
        if let Some((key, pipeline, _)) = cached.as_ref()
            && *key == ctx.cache_key()
        {
            return Ok(pipeline.clone());
        }
        let library = ctx
            .device
            .newLibraryWithSource_options_error(
                &objc2_foundation::NSString::from_str(include_str!("gradient.metal")),
                Some(&CompileOptions::new()),
            )
            .map_err(anyhow::Error::msg)?;
        let descriptor = RenderPipelineDescriptor::new();
        let vertex = library
            .newFunctionWithName(&objc2_foundation::NSString::from_str("gradient_vertex"))
            .ok_or_else(|| anyhow::anyhow!("Metal function missing"))
            .map_err(anyhow::Error::msg)?;
        let fragment = library
            .newFunctionWithName(&objc2_foundation::NSString::from_str("gradient_fragment"))
            .ok_or_else(|| anyhow::anyhow!("Metal function missing"))
            .map_err(anyhow::Error::msg)?;
        descriptor.setVertexFunction(Some(&vertex));
        descriptor.setFragmentFunction(Some(&fragment));
        unsafe { descriptor.colorAttachments().objectAtIndexedSubscript(0) }
            .setPixelFormat(MTLPixelFormat::BGRA8Unorm);
        let pipeline = ctx
            .device
            .newRenderPipelineStateWithDescriptor_error(&descriptor)
            .map_err(anyhow::Error::msg)?;
        *cached = Some((ctx.cache_key(), pipeline.clone(), ctx.queue.clone()));
        self.compilations.fetch_add(1, Ordering::Relaxed);
        Ok(pipeline)
    }
}
#[repr(C, align(16))]
struct UploadStop {
    color: [f32; 4],
    offset: [f32; 4],
}
impl Gradient {
    pub(super) fn prepare_metal(
        &self,
        backend: &dyn Any,
        extent: Size<DevicePixels>,
    ) -> Result<Box<dyn Any>> {
        let Some(ctx) = backend.downcast_ref::<MetalCustomContext>() else {
            bail!("Gradient requires the Metal backend")
        };
        let (output, generated) = self.output.prepare(ctx, extent, || {
            let pipeline = self.pipelines.get(ctx)?;
            let desc = TextureDescriptor::new();
            unsafe { desc.setWidth(extent.width.0 as usize) };
            unsafe { desc.setHeight(extent.height.0 as usize) };
            desc.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
            desc.setStorageMode(MTLStorageMode::Private);
            desc.setUsage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead);
            let texture = ctx
                .device
                .newTextureWithDescriptor(&desc)
                .expect("Metal texture allocation failed");
            let (kind, center, radius, angle) = match self.kind {
                GradientKind::Linear { angle } => (0., [0.5; 2], [0.5; 2], angle),
                GradientKind::Radial { center, radius } => (1., center, radius, 0.),
                GradientKind::Conic { center, angle } => (2., center, [0.5; 2], angle),
                GradientKind::Diamond {
                    center,
                    radius,
                    angle,
                } => (3., center, radius, angle),
            };
            let radians = angle.rem_euclid(360.).to_radians();
            let direction = if kind == 0. {
                [radians.sin(), -radians.cos(), radians, 0.]
            } else {
                [radians.cos(), radians.sin(), radians, 0.]
            };
            let params = [
                [
                    extent.width.0 as f32,
                    extent.height.0 as f32,
                    kind,
                    self.stops.len() as f32,
                ],
                [center[0], center[1], radius[0], radius[1]],
                direction,
            ];
            let stops: Vec<_> = self
                .stops
                .iter()
                .map(|s| UploadStop {
                    color: s.color,
                    offset: [s.offset, 0., 0., 0.],
                })
                .collect();
            let command = ctx
                .queue
                .commandBuffer()
                .expect("Metal command buffer allocation failed")
                .to_owned();
            let pass = RenderPassDescriptor::new();
            let attachment = unsafe { pass.colorAttachments().objectAtIndexedSubscript(0) };
            attachment.setTexture(Some(&texture));
            attachment.setLoadAction(MTLLoadAction::DontCare);
            attachment.setStoreAction(MTLStoreAction::Store);
            let encoder = command
                .renderCommandEncoderWithDescriptor(&pass)
                .expect("Metal render encoder allocation failed");
            encoder.setRenderPipelineState(&pipeline);
            unsafe {
                encoder.setFragmentBytes_length_atIndex(
                    std::ptr::NonNull::new(params.as_ptr().cast::<std::ffi::c_void>() as *mut _)
                        .expect("non-null bytes"),
                    std::mem::size_of_val(&params) as usize,
                    0,
                )
            };
            unsafe {
                encoder.setFragmentBytes_length_atIndex(
                    std::ptr::NonNull::new(stops.as_ptr().cast::<std::ffi::c_void>() as *mut _)
                        .expect("non-null bytes"),
                    std::mem::size_of_val(stops.as_slice()) as usize,
                    1,
                )
            };
            unsafe {
                encoder.drawPrimitives_vertexStart_vertexCount(MTLPrimitiveType::Triangle, 0, 6)
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

// SAFETY: pipeline state is immutable; the cache mutex serializes replacement and retains its device/queue.
unsafe impl Send for PipelineCache {}
unsafe impl Sync for PipelineCache {}
