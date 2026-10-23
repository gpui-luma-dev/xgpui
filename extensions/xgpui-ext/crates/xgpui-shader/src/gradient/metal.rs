use super::{Gradient, GradientKind};
use ::metal::*;
use anyhow::{Result, bail, ensure};
use gpui::{CustomContent, DevicePixels, Size};
use gpui_apple::custom_draw::{MetalCustomContext, MetalCustomOutput};
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
            .new_library_with_source(include_str!("gradient.metal"), &CompileOptions::new())
            .map_err(anyhow::Error::msg)?;
        let descriptor = RenderPipelineDescriptor::new();
        let vertex = library
            .get_function("gradient_vertex", None)
            .map_err(anyhow::Error::msg)?;
        let fragment = library
            .get_function("gradient_fragment", None)
            .map_err(anyhow::Error::msg)?;
        descriptor.set_vertex_function(Some(&vertex));
        descriptor.set_fragment_function(Some(&fragment));
        descriptor
            .color_attachments()
            .object_at(0)
            .unwrap()
            .set_pixel_format(MTLPixelFormat::BGRA8Unorm);
        let pipeline = ctx
            .device
            .new_render_pipeline_state(&descriptor)
            .map_err(anyhow::Error::msg)?;
        *cached = Some((ctx.cache_key(), pipeline.clone(), ctx.queue.clone()));
        self.compilations.fetch_add(1, Ordering::Relaxed);
        Ok(pipeline)
    }
}
pub(super) struct CachedOutput {
    key: u64,
    extent: Size<DevicePixels>,
    texture: Texture,
    command: CommandBuffer,
    _queue: CommandQueue,
}
#[repr(C, align(16))]
struct UploadStop {
    color: [f32; 4],
    offset: [f32; 4],
}
impl CustomContent for Gradient {
    fn prepare(&self, backend: &dyn Any, extent: Size<DevicePixels>) -> Result<Box<dyn Any>> {
        let Some(ctx) = backend.downcast_ref::<MetalCustomContext>() else {
            bail!("Gradient requires the Metal backend")
        };
        ensure!(
            extent.width.0 > 0
                && extent.height.0 > 0
                && extent.width.0 <= 16384
                && extent.height.0 <= 16384,
            "invalid gradient extent"
        );
        ensure!(
            extent.width.0 as u64 * extent.height.0 as u64 * 4 <= 64 * 1024 * 1024,
            "gradient output exceeds 64 MiB"
        );
        let mut cached = self
            .output
            .lock()
            .map_err(|_| anyhow::anyhow!("gradient output cache poisoned"))?;
        if let Some(cache) = cached.as_ref()
            && cache.key == ctx.cache_key()
            && cache.extent == extent
        {
            let commands = match cache.command.status() {
                MTLCommandBufferStatus::NotEnqueued => vec![cache.command.clone()],
                MTLCommandBufferStatus::Error => bail!("gradient preparation failed on the GPU"),
                MTLCommandBufferStatus::Enqueued => bail!("gradient command enqueued outside host"),
                _ => Vec::new(),
            };
            return Ok(Box::new(MetalCustomOutput {
                texture: cache.texture.clone(),
                commands,
            }));
        }
        let pipeline = self.pipelines.get(ctx)?;
        let desc = TextureDescriptor::new();
        desc.set_width(extent.width.0 as u64);
        desc.set_height(extent.height.0 as u64);
        desc.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
        desc.set_storage_mode(MTLStorageMode::Private);
        desc.set_usage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead);
        let texture = ctx.device.new_texture(&desc);
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
        let command = ctx.queue.new_command_buffer().to_owned();
        let pass = RenderPassDescriptor::new();
        let attachment = pass.color_attachments().object_at(0).unwrap();
        attachment.set_texture(Some(&texture));
        attachment.set_load_action(MTLLoadAction::DontCare);
        attachment.set_store_action(MTLStoreAction::Store);
        let encoder = command.new_render_command_encoder(pass);
        encoder.set_render_pipeline_state(&pipeline);
        encoder.set_fragment_bytes(
            0,
            std::mem::size_of_val(&params) as u64,
            params.as_ptr().cast(),
        );
        encoder.set_fragment_bytes(
            1,
            std::mem::size_of_val(stops.as_slice()) as u64,
            stops.as_ptr().cast(),
        );
        encoder.draw_primitives(MTLPrimitiveType::Triangle, 0, 6);
        encoder.end_encoding();
        *cached = Some(CachedOutput {
            key: ctx.cache_key(),
            extent,
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
