use super::{ColorMatrix, ImagePrimaries, Vignette};
use gpui::{CustomContent, DevicePixels, PreparationRequest, Size};
use std::any::Any;

/// Validated GPU placement adjustments for retained RGBA8 or RGBA16F images.
/// Source identity is unchanged. Uses declared linear sRGB/P3 working primaries,
/// maps at the preview output; native wide preserves extended RGB and source alpha.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImageAdjustments(gpui::LinearImagePaint);
impl ImageAdjustments {
    pub fn new(matrix: ColorMatrix, vignette: Option<Vignette>) -> anyhow::Result<Self> {
        let p3 = match matrix.primaries() {
            ImagePrimaries::Srgb => false,
            ImagePrimaries::DisplayP3 => true,
            ImagePrimaries::Rec2020 => anyhow::bail!(
                "GPU image adjustments require linear sRGB or Display P3 working primaries; Rec.2020 input can be adjusted in either space"
            ),
            ImagePrimaries::AdobeRgb1998 => anyhow::bail!(
                "GPU image adjustments require linear sRGB or Display P3 working primaries; convert Adobe RGB input first"
            ),
        };
        let rows = matrix.rows().map(|row| row.map(|v| v as f32));
        let vignette = vignette
            .map(|v| v.parameters())
            .unwrap_or([0., 0., 1., 1.])
            .map(|v| v as f32);
        Ok(Self(
            gpui::LinearImagePaint::new(rows, vignette)?.with_p3(p3),
        ))
    }
    /// Shared renderer SPI parameters, also usable by application-owned image
    /// sources (including browser sources using the existing wgpu image SPI).
    pub fn paint(self) -> gpui::LinearImagePaint {
        self.0
    }
}
#[derive(Clone, Debug)]
pub struct AdjustedImage {
    image: crate::MediaImage,
    adjustments: ImageAdjustments,
}
impl crate::MediaImage {
    pub fn adjusted(&self, adjustments: ImageAdjustments) -> AdjustedImage {
        AdjustedImage {
            image: self.clone(),
            adjustments,
        }
    }
}
impl AdjustedImage {
    /// Portable linear adjustment, without changing source identity or mips.
    pub fn draw(&self) -> gpui::content::ContentDraw {
        self.image
            .draw()
            .color_transform(gpui::content::ImageColorTransform::linear(
                self.adjustments.paint(),
            ))
    }
}
impl CustomContent for AdjustedImage {
    fn portable_content(
        &self,
        request: &PreparationRequest,
    ) -> Result<Option<gpui::content::ContentDrawRequest>, gpui::content::ContentError> {
        self.draw().portable_content(request)
    }
    fn prepare(&self, _: &dyn Any, _: Size<DevicePixels>) -> anyhow::Result<Box<dyn Any>> {
        Err(gpui::content::ContentError::new(
            gpui::content::ContentErrorKind::UnsupportedCapability,
            "adjusted images require portable content presentation",
        )
        .into())
    }
}

/// Explicit native wide-SDR placement. Accepts RGBA8 or RGBA16F source pixels and
/// an accepted WideGamutSdr window. wgpu/browser hosts reject this output policy.
/// Adjustment affine row absolute sums must not exceed 4, keeping four nested
/// wide color filters finite in binary16.
#[derive(Clone, Debug)]
pub struct WideImage {
    image: crate::MediaImage,
    adjustments: Option<ImageAdjustments>,
}
impl crate::MediaImage {
    pub fn wide(&self, adjustments: Option<ImageAdjustments>) -> anyhow::Result<WideImage> {
        if let Some(adjustments) = adjustments {
            anyhow::ensure!(
                adjustments.paint().rows().iter().all(|row| row
                    .iter()
                    .map(|v| f64::from(v.abs()))
                    .sum::<f64>()
                    <= 4.),
                "wide SDR adjustment row magnitude exceeds 4"
            );
        }
        Ok(WideImage {
            image: self.clone(),
            adjustments,
        })
    }
}
impl WideImage {
    /// Portable wide placement; preserves the source gamut and applies any
    /// validated linear adjustment before native wide composition.
    pub fn draw(&self) -> gpui::content::ContentDraw {
        use gpui::content::*;
        let draw = self.image.draw().color_output(ContentColorOutput::WideSdr);
        if let Some(adjustments) = self.adjustments {
            draw.color_transform(ImageColorTransform::linear(adjustments.paint()))
        } else {
            draw
        }
    }
}
impl CustomContent for WideImage {
    fn portable_content(
        &self,
        request: &PreparationRequest,
    ) -> Result<Option<gpui::content::ContentDrawRequest>, gpui::content::ContentError> {
        self.draw().portable_content(request)
    }
    fn prepare(&self, _: &dyn Any, _: Size<DevicePixels>) -> anyhow::Result<Box<dyn Any>> {
        Err(gpui::content::ContentError::new(
            gpui::content::ContentErrorKind::UnsupportedCapability,
            "wide images require portable content presentation",
        )
        .into())
    }
}
