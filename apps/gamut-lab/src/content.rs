//! Explicit wide SDR producer using the same CustomContent protocol as sRGB paints.
use anyhow::{Result, ensure};
use gpui::{CustomContent, DevicePixels, Size};
use gpui_apple::custom_draw::{MetalCustomContext, MetalWideSdrOutput};
use metal::*;
use std::any::Any;

#[derive(Debug)]
pub(crate) struct WideColor(pub [f64; 4]);
impl CustomContent for WideColor {
    fn prepare(&self, backend: &dyn Any, extent: Size<DevicePixels>) -> Result<Box<dyn Any>> {
        ensure!(
            self.0
                .iter()
                .all(|c| c.is_finite() && (0.0..=1.0).contains(c)),
            "invalid SDR P3 color"
        );
        let context = backend
            .downcast_ref::<MetalCustomContext>()
            .ok_or_else(|| anyhow::anyhow!("gamut-lab requires native Metal"))?;
        ensure!(
            extent.width.0 > 0
                && extent.height.0 > 0
                && extent.width.0 <= 16384
                && extent.height.0 <= 16384,
            "invalid surface extent"
        );
        ensure!(
            extent.width.0 as u64 * extent.height.0 as u64 * 8 <= 64 * 1024 * 1024,
            "surface exceeds 64 MiB"
        );
        let d = TextureDescriptor::new();
        d.set_width(extent.width.0 as u64);
        d.set_height(extent.height.0 as u64);
        d.set_pixel_format(MTLPixelFormat::RGBA16Float);
        d.set_usage(MTLTextureUsage::ShaderRead | MTLTextureUsage::RenderTarget);
        d.set_storage_mode(MTLStorageMode::Private);
        let texture = context.device.new_texture(&d);
        let command = context.queue.new_command_buffer();
        let pass = RenderPassDescriptor::new();
        let c = pass.color_attachments().object_at(0).unwrap();
        c.set_texture(Some(&texture));
        c.set_load_action(MTLLoadAction::Clear);
        c.set_store_action(MTLStoreAction::Store);
        let [r, g, b, a] = self.0;
        c.set_clear_color(MTLClearColor::new(r * a, g * a, b * a, a));
        command.new_render_command_encoder(pass).end_encoding();
        Ok(Box::new(MetalWideSdrOutput::from_linear_p3_premultiplied(
            context,
            texture,
            vec![command.to_owned()],
        )?))
    }
}
