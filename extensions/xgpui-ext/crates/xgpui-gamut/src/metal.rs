use super::*;
use ::metal::*;
use gpui_apple::custom_draw::{MetalCustomContext, MetalWideSdrOutput};

impl WideColor {
    pub(super) fn prepare_metal(
        &self,
        backend: &dyn Any,
        extent: Size<DevicePixels>,
    ) -> Result<Box<dyn Any>> {
        let context = backend
            .downcast_ref::<MetalCustomContext>()
            .ok_or_else(|| anyhow::anyhow!("WideColor requires native Metal"))?;
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
        let [r, g, b, a] = self.rgba.map(f64::from);
        c.set_clear_color(MTLClearColor::new(r * a, g * a, b * a, a));
        command.new_render_command_encoder(pass).end_encoding();
        Ok(Box::new(MetalWideSdrOutput::from_linear_p3_premultiplied(
            context,
            texture,
            vec![command.to_owned()],
        )?))
    }
}
