use gpui::{CustomContent, DevicePixels, PreparationFrame, PreparationRequest, Size};
use std::any::Any;

/// Ordered affine RGB adjustments in encoded sRGB, preserving source alpha.
/// Calls compose in order. Factors clamp to 0..8; nonfinite factors use identity.
/// An overflowing composition is ignored. Values clamp to the SDR target range after the combined transform, not per step.
#[derive(Clone, Copy, Debug, PartialEq)]
#[must_use]
pub struct ImageFilter {
    rows: [[f32; 4]; 3],
}
impl Default for ImageFilter {
    fn default() -> Self {
        Self {
            rows: [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]],
        }
    }
}
impl ImageFilter {
    pub fn new() -> Self {
        Self::default()
    }
    fn then(self, next: [[f32; 4]; 3]) -> Self {
        let mut rows = [[0.; 4]; 3];
        for (i, row) in rows.iter_mut().enumerate() {
            for (j, value) in row.iter_mut().enumerate() {
                *value = (0..3).map(|k| next[i][k] * self.rows[k][j]).sum::<f32>()
                    + if j == 3 { next[i][3] } else { 0. };
            }
        }
        if rows.iter().flatten().all(|v| v.is_finite()) {
            Self { rows }
        } else {
            self
        }
    }
    pub fn brightness(self, factor: f32) -> Self {
        let f = factor_or_identity(factor);
        self.then([[f, 0., 0., 0.], [0., f, 0., 0.], [0., 0., f, 0.]])
    }
    pub fn contrast(self, factor: f32) -> Self {
        let f = factor_or_identity(factor);
        let b = 0.5 * (1. - f);
        self.then([[f, 0., 0., b], [0., f, 0., b], [0., 0., f, b]])
    }
    pub fn saturation(self, factor: f32) -> Self {
        let f = factor_or_identity(factor);
        let luma = [0.2126, 0.7152, 0.0722];
        let mut rows = [[0.; 4]; 3];
        for (i, row) in rows.iter_mut().enumerate() {
            for j in 0..3 {
                row[j] = (1. - f) * luma[j] + if i == j { f } else { 0. };
            }
        }
        self.then(rows)
    }
    /// Hue rotation in degrees, using the encoded-sRGB luminance axis.
    pub fn hue_rotate(self, degrees: f32) -> Self {
        let degrees = if degrees.is_finite() {
            degrees.rem_euclid(360.)
        } else {
            0.
        };
        let (s, c) = degrees.to_radians().sin_cos();
        self.then([
            [
                0.213 + 0.787 * c - 0.213 * s,
                0.715 - 0.715 * c - 0.715 * s,
                0.072 - 0.072 * c + 0.928 * s,
                0.,
            ],
            [
                0.213 - 0.213 * c + 0.143 * s,
                0.715 + 0.285 * c + 0.140 * s,
                0.072 - 0.072 * c - 0.283 * s,
                0.,
            ],
            [
                0.213 - 0.213 * c - 0.787 * s,
                0.715 - 0.715 * c + 0.715 * s,
                0.072 + 0.928 * c + 0.072 * s,
                0.,
            ],
        ])
    }
    /// Mix RGB toward encoded sRGB tint by its fourth component; preserve alpha.
    pub fn tint(self, rgba: [f32; 4]) -> Self {
        let rgba = rgba.map(|v| if v.is_finite() { v.clamp(0., 1.) } else { 0. });
        let a = rgba[3];
        self.then([
            [1. - a, 0., 0., rgba[0] * a],
            [0., 1. - a, 0., rgba[1] * a],
            [0., 0., 1. - a, rgba[2] * a],
        ])
    }
    pub fn rows(self) -> [[f32; 4]; 3] {
        self.rows
    }
}
fn factor_or_identity(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0., 8.)
    } else {
        1.
    }
}

/// Filtered placement shares the source's upload cache and lifetime.
#[derive(Clone, Debug)]
pub struct FilteredImage {
    pub(crate) image: crate::MediaImage,
    pub(crate) filter: ImageFilter,
}
impl crate::MediaImage {
    pub fn filtered(&self, filter: ImageFilter) -> FilteredImage {
        FilteredImage {
            image: self.clone(),
            filter,
        }
    }
}
impl CustomContent for FilteredImage {
    fn image_color_transform(&self) -> [[f32; 4]; 3] {
        self.filter.rows()
    }
    fn prepare(&self, backend: &dyn Any, size: Size<DevicePixels>) -> anyhow::Result<Box<dyn Any>> {
        self.image.prepare(backend, size)
    }
    fn prepare_in_frame(
        &self,
        backend: &dyn Any,
        request: &PreparationRequest,
        frame: &PreparationFrame,
    ) -> anyhow::Result<Box<dyn Any>> {
        self.image.prepare_in_frame(backend, request, frame)
    }
    fn prepare_with_request(
        &self,
        backend: &dyn Any,
        request: &PreparationRequest,
    ) -> anyhow::Result<Box<dyn Any>> {
        self.image.prepare_with_request(backend, request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn apply(f: ImageFilter, rgb: [f32; 3]) -> [f32; 3] {
        f.rows
            .map(|row| row[0] * rgb[0] + row[1] * rgb[1] + row[2] * rgb[2] + row[3])
    }
    #[test]
    fn ordered_adjustments_and_sanitization() {
        assert_eq!(apply(ImageFilter::new(), [0.2, 0.4, 0.8]), [0.2, 0.4, 0.8]);
        let a = apply(ImageFilter::new().brightness(2.).contrast(0.5), [0.2; 3]);
        assert!((a[0] - 0.45).abs() < 1e-6);
        let b = apply(ImageFilter::new().contrast(0.5).brightness(2.), [0.2; 3]);
        assert!((b[0] - 0.7).abs() < 1e-6);
        assert_eq!(ImageFilter::new().brightness(f32::NAN), ImageFilter::new());
        assert_eq!(
            apply(ImageFilter::new().tint([1., 0., 0., 1.]), [0.2; 3]),
            [1., 0., 0.]
        );
        let gray = apply(ImageFilter::new().saturation(0.), [1., 0., 0.]);
        for channel in gray {
            assert!((channel - 0.2126).abs() < 1e-6);
        }
        let neutral = apply(ImageFilter::new().hue_rotate(120.), [0.5; 3]);
        for channel in neutral {
            assert!((channel - 0.5).abs() < 1e-6);
        }
    }
}
