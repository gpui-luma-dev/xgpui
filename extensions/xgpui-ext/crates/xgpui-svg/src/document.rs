use crate::{MIB, image_bytes};
use anyhow::{Result, ensure};
use gpui::{DevicePixels, ParsedSvg, RenderImage, Size, SvgRenderer, SvgSize, size};
use std::{ops::Deref, sync::Arc, time::Instant};

/// A parsed, immutable static SVG. Clone shares the tree; no hidden image cache.
#[derive(Clone)]
pub struct SvgDocument(Arc<ParsedSvg>);
impl SvgDocument {
    /// Parse supplied bytes using the application's font/resolver configuration.
    /// Run off the UI thread for untrusted or complex artwork. Relative resources
    /// are subject to GPUI's resolver; this does not fetch remote resources.
    pub fn parse(renderer: &SvgRenderer, bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() <= 32 * MIB,
            "SVG exceeds the 32 MiB input limit"
        );
        Ok(Self(Arc::new(renderer.parse_svg(bytes)?)))
    }
    /// True when both handles share the same immutable parsed tree.
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
    pub fn from_parsed(parsed: Arc<ParsedSvg>) -> Self {
        Self(parsed)
    }
    /// Normalize the longest edge while retaining the resolved intrinsic ratio.
    pub fn fit_size(&self, edge: f32) -> Result<Size<f32>> {
        ensure!(
            edge.is_finite() && edge > 0.,
            "logical edge must be finite and positive"
        );
        let intrinsic = self.size();
        Ok(if intrinsic.width >= intrinsic.height {
            size(edge, edge * (intrinsic.height / intrinsic.width))
        } else {
            size(edge * (intrinsic.width / intrinsic.height), edge)
        })
    }
    /// Render a job from `RasterQueue` on a background worker.
    pub fn rasterize(
        &self,
        renderer: &SvgRenderer,
        pixels: Size<DevicePixels>,
        max_output_bytes: usize,
    ) -> Result<RasterImage> {
        RasterImage::render(renderer, self, pixels, max_output_bytes)
    }
}
impl Deref for SvgDocument {
    type Target = ParsedSvg;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
/// Full-color CPU image and raster/conversion wall time. Clone shares image bytes.
#[derive(Clone)]
pub struct RasterImage {
    pub image: Arc<RenderImage>,
    pub ms: f64,
}
impl RasterImage {
    pub fn render(
        renderer: &SvgRenderer,
        parsed: &ParsedSvg,
        pixels: Size<DevicePixels>,
        max_output_bytes: usize,
    ) -> Result<Self> {
        ensure!(
            (4..=256 * MIB).contains(&max_output_bytes),
            "invalid output budget"
        );
        ensure!(
            [pixels.width, pixels.height]
                .iter()
                .all(|p| (1..=8192).contains(&i32::from(*p))),
            "raster edge must be 1..=8192"
        );
        ensure!(
            image_bytes(pixels) <= max_output_bytes,
            "requested raster exceeds output budget"
        );
        let start = Instant::now();
        let image = renderer.render_parsed(parsed, SvgSize::ExactSize(pixels))?;
        Ok(Self {
            image,
            ms: start.elapsed().as_secs_f64() * 1000.,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn immutable_document_keeps_ratio_color_and_enforces_render_budget() {
        let renderer = SvgRenderer::new(Arc::new(()));
        let doc=SvgDocument::parse(&renderer,br##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="400"><path fill="#25aacc" d="M0 0H100V400H0Z"/></svg>"##).unwrap();
        assert_eq!(doc.fit_size(320.).unwrap(), size(80., 320.));
        let pixels = size(DevicePixels(80), DevicePixels(320));
        let image = doc.clone().rasterize(&renderer, pixels, 102400).unwrap();
        assert_eq!(image.image.as_bytes(0).unwrap().len(), 102400);
        assert!(doc.rasterize(&renderer, pixels, 102399).is_err());
        assert!(
            doc.rasterize(&renderer, size(DevicePixels(0), DevicePixels(320)), MIB)
                .is_err()
        );
        assert!(SvgDocument::parse(&renderer, b"invalid SVG").is_err());
        assert_eq!(doc.size(), size(100., 400.)); // a failed parse cannot mutate retained trees
    }
}
