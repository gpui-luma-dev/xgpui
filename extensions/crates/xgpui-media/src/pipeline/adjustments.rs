use super::{ColorMatrix, ImagePrimaries, Vignette};
use gpui::{CustomContent, DevicePixels, PreparationFrame, PreparationRequest, Size};
use std::any::Any;

/// Validated GPU placement adjustments for retained RGBA8 or RGBA16F images.
/// Source identity is unchanged. Uses declared linear sRGB/P3 working primaries,
/// clips once at output and preserves source alpha.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImageAdjustments(gpui::LinearImagePaint);
impl ImageAdjustments {
    pub fn new(matrix: ColorMatrix, vignette: Option<Vignette>) -> anyhow::Result<Self> {
        let rows = matrix.rows().map(|row| row.map(|v| v as f32));
        let vignette = vignette
            .map(|v| v.parameters())
            .unwrap_or([0., 0., 1., 1.])
            .map(|v| v as f32);
        Ok(Self(
            gpui::LinearImagePaint::new(rows, vignette)?
                .with_p3(matrix.primaries() == ImagePrimaries::DisplayP3),
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
impl CustomContent for AdjustedImage {
    fn image_color_mode(&self) -> gpui::ImageColorMode {
        self.image.image_color_mode()
    }

    fn image_linear_adjustments(&self) -> Option<gpui::LinearImagePaint> {
        Some(self.adjustments.paint())
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

/// Explicit native wide-SDR placement. Requires a canonical RGBA16F source and
/// an accepted WideGamutSdr window. wgpu/browser hosts reject this output policy.
#[derive(Clone, Debug)]
pub struct WideImage {
    image: crate::MediaImage,
    adjustments: Option<ImageAdjustments>,
}
impl crate::MediaImage {
    pub fn wide(&self, adjustments: Option<ImageAdjustments>) -> anyhow::Result<WideImage> {
        anyhow::ensure!(
            self.format() == gpui::ImageTextureFormat::LinearP3Rgba16Float,
            "wide placement requires RGBA16F source pixels"
        );
        Ok(WideImage {
            image: self.clone(),
            adjustments,
        })
    }
}
impl CustomContent for WideImage {
    fn image_color_mode(&self) -> gpui::ImageColorMode {
        gpui::ImageColorMode::LinearP3Wide
    }
    fn image_linear_adjustments(&self) -> Option<gpui::LinearImagePaint> {
        self.adjustments.map(|v| v.paint())
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
