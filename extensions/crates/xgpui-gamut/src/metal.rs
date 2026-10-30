use super::*;
use anyhow::Context as _;
use gpui_apple::custom_draw::{MetalCustomContext, MetalWideSdrOutput};
use objc2_metal::*;

impl WideColor {
    pub(super) fn prepare_metal(
        &self,
        backend: &dyn Any,
        extent: Size<DevicePixels>,
    ) -> Result<Box<dyn Any>> {
        let context = backend
            .downcast_ref::<MetalCustomContext>()
            .ok_or_else(|| anyhow::anyhow!("WideColor requires native Metal"))?;
        anyhow::ensure!(
            extent.width.0 > 0 && extent.height.0 > 0,
            "empty wide-color extent"
        );
        let d = MTLTextureDescriptor::new();
        // SAFETY: the checked physical extent is positive. Allocation failure
        // (including an unsupported device extent) is returned to the producer.
        unsafe {
            d.setWidth(extent.width.0 as usize);
            d.setHeight(extent.height.0 as usize);
        }
        d.setPixelFormat(MTLPixelFormat::RGBA16Float);
        d.setUsage(MTLTextureUsage::ShaderRead | MTLTextureUsage::RenderTarget);
        d.setStorageMode(MTLStorageMode::Private);
        let texture = context
            .objc2_device()
            .newTextureWithDescriptor(&d)
            .context("failed to allocate wide-color Metal texture")?;
        #[cfg(test)]
        super::PRODUCER_ALLOCATIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let command = context
            .objc2_queue()
            .commandBuffer()
            .context("failed to allocate wide-color Metal command buffer")?;
        let pass = MTLRenderPassDescriptor::new();
        // SAFETY: attachment zero is within Metal's eight color attachments.
        let c = unsafe { pass.colorAttachments().objectAtIndexedSubscript(0) };
        c.setTexture(Some(&texture));
        c.setLoadAction(MTLLoadAction::Clear);
        c.setStoreAction(MTLStoreAction::Store);
        let [r, g, b, a] = self.rgba.map(f64::from);
        c.setClearColor(MTLClearColor {
            red: r * a,
            green: g * a,
            blue: b * a,
            alpha: a,
        });
        command
            .renderCommandEncoderWithDescriptor(&pass)
            .context("failed to encode wide-color Metal clear")?
            .endEncoding();
        Ok(Box::new(
            MetalWideSdrOutput::from_objc2_linear_p3_premultiplied(
                context,
                texture,
                vec![command],
            )?,
        ))
    }
}
