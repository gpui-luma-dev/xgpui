use super::*;
use gpui_windows::custom_draw::Dx11CustomContext;

impl WideColor {
    pub(super) fn prepare_dx11(
        &self,
        backend: &dyn Any,
        extent: Size<DevicePixels>,
    ) -> Result<Box<dyn Any>> {
        let context = backend
            .downcast_ref::<Dx11CustomContext>()
            .ok_or_else(|| anyhow::anyhow!("WideColor requires native D3D11"))?;
        let output = context.create_wide_sdr_output(extent)?;
        #[cfg(test)]
        super::PRODUCER_ALLOCATIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let [r, g, b, a] = self.rgba;
        context.record_wide_sdr(&output, |deferred, target| {
            unsafe {
                deferred.ClearRenderTargetView(target, &[r * a, g * a, b * a, a]);
            }
            Ok(())
        })?;
        Ok(Box::new(output))
    }
}
