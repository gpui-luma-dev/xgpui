//! Explicit bounded effects over an isolated paint subtree.
use anyhow::{Result, ensure};
use gpui::*;
use std::sync::Arc;

/// Color filtering and opacity applied after the surface and ordinary descendants.
/// Native `.opacity()` remains per-primitive inside this isolated group.
#[derive(Clone, Debug)]
pub struct LayerEffect {
    opacity: f32,
    color_filter: LayerColorFilter,
    program: Arc<SceneShaderProgram>,
}
impl LayerEffect {
    /// Construct a finite group opacity in [0,1].
    pub fn opacity(opacity: f32) -> Result<Self> {
        ensure!(
            opacity.is_finite() && (0. ..=1.).contains(&opacity),
            "group opacity must be in [0,1]"
        );
        Ok(Self {
            opacity,
            color_filter: LayerColorFilter::default(),
            program: crate::backdrop::shared_program()?,
        })
    }
    /// Construct saturation in [0,2]: 0 is grayscale, 1 unchanged, 2 intensified.
    /// RGB is filtered in encoded sRGB; alpha is preserved before group opacity.
    pub fn saturation(saturation: f32) -> Result<Self> {
        Self::opacity(1.)?.with_saturation(saturation)
    }
    /// Construct grayscale strength in [0,1]: 0 unchanged, 1 fully gray.
    pub fn grayscale(amount: f32) -> Result<Self> {
        Self::opacity(1.)?.with_grayscale(amount)
    }
    /// Set saturation on this group; replaces any earlier saturation/grayscale.
    pub fn with_saturation(mut self, saturation: f32) -> Result<Self> {
        ensure!(
            saturation.is_finite() && (0. ..=2.).contains(&saturation),
            "group saturation must be in [0,2]"
        );
        self.color_filter.saturation = saturation;
        Ok(self)
    }
    /// Set grayscale strength; replaces any earlier saturation/grayscale.
    pub fn with_grayscale(self, amount: f32) -> Result<Self> {
        ensure!(
            amount.is_finite() && (0. ..=1.).contains(&amount),
            "group grayscale amount must be in [0,1]"
        );
        self.with_saturation(1. - amount)
    }
    /// Construct brightness in [0,2]: 0 black, 1 unchanged, 2 doubled.
    pub fn brightness(brightness: f32) -> Result<Self> {
        Self::opacity(1.)?.with_brightness(brightness)
    }
    /// Construct contrast in [0,2]: 0 mid-gray, 1 unchanged, 2 intensified.
    pub fn contrast(contrast: f32) -> Result<Self> {
        Self::opacity(1.)?.with_contrast(contrast)
    }
    /// Set brightness, applied after saturation and before contrast.
    pub fn with_brightness(mut self, brightness: f32) -> Result<Self> {
        ensure!(
            brightness.is_finite() && (0. ..=2.).contains(&brightness),
            "group brightness must be in [0,2]"
        );
        self.color_filter.brightness = brightness;
        Ok(self)
    }
    /// Set contrast around straight RGB 0.5, applied before group opacity.
    pub fn with_contrast(mut self, contrast: f32) -> Result<Self> {
        ensure!(
            contrast.is_finite() && (0. ..=2.).contains(&contrast),
            "group contrast must be in [0,2]"
        );
        self.color_filter.contrast = contrast;
        Ok(self)
    }
    /// Invert completed subtree RGB: 0 unchanged, 1 complemented, 0.5 gray.
    /// Applied after contrast in encoded sRGB; alpha is preserved.
    pub fn invert(amount: f32) -> Result<Self> {
        Self::opacity(1.)?.with_invert(amount)
    }
    /// Set inversion strength in [0,1], applied before group opacity.
    pub fn with_invert(mut self, amount: f32) -> Result<Self> {
        ensure!(
            amount.is_finite() && (0. ..=1.).contains(&amount),
            "group inversion must be in [0,1]"
        );
        self.color_filter.invert = amount;
        Ok(self)
    }
    /// Blur the completed subtree within its clipped bounds.
    /// Radius is the Gaussian kernel support in logical pixels, in [0,16].
    pub fn blur(radius: Pixels) -> Result<Self> {
        Self::opacity(1.)?.with_blur(radius)
    }
    /// Set blur before color filtering, opacity and destination blending.
    /// The physical support radius must fit within 64 device pixels.
    pub fn with_blur(mut self, radius: Pixels) -> Result<Self> {
        let value = f32::from(radius);
        ensure!(
            value.is_finite() && (0. ..=16.).contains(&value),
            "layer blur radius must be in [0,16] logical pixels"
        );
        self.color_filter.blur_radius = radius;
        Ok(self)
    }
    /// Select adaptive working resolution or the full-resolution blur reference.
    /// Changing quality reuses the raw capture and refreshes filtered pixels.
    pub fn with_blur_quality(mut self, quality: gpui::LayerBlurQuality) -> Self {
        self.color_filter.blur_quality = quality;
        self
    }
    /// Blend the completed subtree with destination pixels, in encoded sRGB.
    /// Filtering and group opacity are applied before source-over blending.
    pub fn blend(mode: crate::BlendMode) -> Result<Self> {
        Ok(Self::opacity(1.)?.with_blend_mode(mode))
    }
    /// Set the destination blend without invalidating retained raw child pixels.
    pub fn with_blend_mode(mut self, mode: crate::BlendMode) -> Self {
        self.color_filter.blend_mode = match mode {
            crate::BlendMode::Normal => LayerBlendMode::Normal,
            crate::BlendMode::Multiply => LayerBlendMode::Multiply,
            crate::BlendMode::Screen => LayerBlendMode::Screen,
            crate::BlendMode::Overlay => LayerBlendMode::Overlay,
            crate::BlendMode::ColorDodge => LayerBlendMode::ColorDodge,
            crate::BlendMode::SoftLight => LayerBlendMode::SoftLight,
            crate::BlendMode::LinearBurn => LayerBlendMode::LinearBurn,
        };
        self
    }
    pub(crate) fn paint<R>(
        &self,
        window: &mut Window,
        bounds: Bounds<Pixels>,
        paint: impl FnOnce(&mut Window) -> R,
    ) -> R {
        window.with_layer_color_filter(
            bounds,
            self.opacity,
            self.color_filter,
            self.program.clone(),
            paint,
        )
    }
}
