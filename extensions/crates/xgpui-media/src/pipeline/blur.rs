use gpui::{Bounds, LayerBlurQuality, Pixels, Window};

/// Reusable Gaussian blur of a completed bounded sRGB preview.
///
/// Compose pages before blurring: internal tile edges then share one kernel.
/// The kernel clamps at the capture edge. Undrawn pixels outside the image or
/// canvas viewport are transparent; final output clips to bounds. No off-viewport
/// source tiles are loaded.
/// The shared shader compositor retains raw/filtered targets and reuses them
/// while size permits. Custom media draws refresh captured pixels every paint;
/// source uploads/imports and allocated capture/filter textures are reused. Its allocations are separate from
/// `TileBudgets` and can grow with the viewport. This is not linear-light or
/// native wide-SDR filtering. Alpha is filtered in premultiplied form.
#[derive(Clone, Debug)]
pub struct ImageBlur {
    effect: xgpui_shader::LayerEffect,
    radius: f32,
}
impl ImageBlur {
    /// Gaussian support radius in logical pixels, 0..=16 (at most 64 device px).
    pub fn new(radius: Pixels) -> anyhow::Result<Self> {
        Ok(Self {
            effect: xgpui_shader::LayerEffect::blur(radius)?,
            radius: f32::from(radius),
        })
    }
    /// Adaptive working resolution by default; FullResolution is a reference.
    pub fn with_quality(mut self, quality: LayerBlurQuality) -> Self {
        self.effect = self.effect.with_blur_quality(quality);
        self
    }
    /// Paint an image, a complete tiled snapshot or an existing video frame.
    /// Reuse this value across frames. Apply sampling adjustments inside this
    /// closure; blur follows them. Bounds define the finite capture and clip.
    /// Nonzero blur requires the capture plus conservative sampling halo to fit
    /// 1024 device pixels per side. Returns an error before painting otherwise.
    /// The shared compositor allows at most four layer groups per scene.
    pub fn paint<R>(
        &self,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        paint: impl FnOnce(&mut Window) -> R,
    ) -> anyhow::Result<R> {
        if self.radius == 0. {
            return Ok(paint(window));
        }
        let scale = window.scale_factor();
        let viewport = Bounds::new(gpui::Point::default(), window.viewport_size());
        let clip = bounds
            .intersect(&window.content_mask().bounds)
            .intersect(&viewport);
        self.check_capture(clip, viewport, scale)?;
        Ok(self.effect.paint(window, bounds, paint))
    }
    fn check_capture(
        &self,
        clip: Bounds<Pixels>,
        viewport: Bounds<Pixels>,
        scale: f32,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            scale.is_finite() && scale > 0. && self.radius * scale <= 64.,
            "blur support exceeds 64 device pixels"
        );
        // Match the compositor's maximum four-level halo reservation. Nested
        // captures reserve less; this deliberately conservative check is stable.
        let source = clip
            .dilate(gpui::px((16. + 8. / scale) * gpui::MAX_LAYER_GROUPS as f32))
            .intersect(&viewport);
        anyhow::ensure!(
            f32::from(source.size.width) * scale + 2. <= gpui::MAX_LAYER_SIDE as f32
                && f32::from(source.size.height) * scale + 2. <= gpui::MAX_LAYER_SIDE as f32,
            "blur capture including halo must fit 1024 device pixels per side; use a smaller preview"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, px, size};
    #[test]
    fn capture_checks_include_halo_and_display_scale_before_recording() {
        let blur = ImageBlur::new(px(16.)).unwrap();
        let rect = |x, y, w, h| Bounds::new(point(px(x), px(y)), size(px(w), px(h)));
        let viewport = rect(0., 0., 4000., 2000.);
        assert!(
            blur.check_capture(rect(200., 200., 350., 300.), viewport, 2.)
                .is_ok()
        );
        assert!(
            blur.check_capture(rect(200., 200., 900., 300.), viewport, 2.)
                .is_err()
        );
        assert!(
            blur.check_capture(rect(200., 200., 100., 100.), viewport, 5.)
                .is_err()
        );
        assert!(
            blur.check_capture(rect(200., 200., 100., 100.), viewport, f32::NAN)
                .is_err()
        );
    }
}
