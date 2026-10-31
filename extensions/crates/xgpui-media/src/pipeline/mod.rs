//! Linear SDR image adjustments and pipeline contracts for issue #26.
//!
//! `ImageAdjustments` renders matrix/vignette effects on retained RGBA8 sources
//! in linear sRGB or P3. Supplied RGBA16F pixels use explicit source metadata. Existing `ImageFilter` remains encoded sRGB. Progressive
//! providers and bounded tiled scheduling use immutable source pages.
//! All source color contracts here use D65 and bounded SDR.
mod blur;
pub use blur::ImageBlur;
mod canvas;
pub use canvas::{
    CanvasOptions, CanvasPage, CanvasSnapshot, CanvasStats, CanvasView, TileCanvas, TileProvider,
};
mod generated;
pub use generated::GeneratedCanvas;
mod adjustments;
pub use adjustments::{AdjustedImage, ImageAdjustments, WideImage};
pub(crate) mod float_image;
pub use float_image::{
    rec2020_rgba16f_data, rgba16f_data, rgba16f_data_with_mapping, rgba16f_payload_bytes,
};
mod color;
mod progressive;
mod tiles;

pub use color::{
    AlphaMode, ColorMatrix, ImageColor, ImagePrimaries, TransferFunction, Vignette, WorkingPixel,
};
pub use progressive::{
    ProgressiveImage, ProgressiveStatus, SourceCapabilities, SourceEvent, SourceId, SourceRequest,
    SourceUpdate,
};
pub use tiles::{
    ImageDescriptor, PixelFormat, PixelRect, TileBudgets, TileDescriptor, TileKey, TileLayout,
    TilePyramid,
};
pub use xgpui_color::GamutMapping;

/// New GPU features, distinct from the existing encoded-sRGB image filter path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PipelineFeature {
    LinearAdjustments,
    Vignette,
    Rgba16FloatImages,
    ProgressiveImages,
    TiledImages,
    ImageBlur,
}

/// Implementation availability, not a device/driver probe or #23 validation.
/// RGBA8 and RGBA16F matrix/vignette drawing is implemented. Wide output still
/// requires native backend/window acceptance. Blur uses the sRGB layer compositor.
#[derive(Clone, Copy, Debug)]
pub struct PipelineCapabilities;
impl PipelineCapabilities {
    pub const fn current() -> Self {
        Self
    }
    pub const fn supports(self, feature: PipelineFeature) -> bool {
        matches!(
            feature,
            PipelineFeature::LinearAdjustments
                | PipelineFeature::Vignette
                | PipelineFeature::Rgba16FloatImages
                | PipelineFeature::ProgressiveImages
                | PipelineFeature::TiledImages
                | PipelineFeature::ImageBlur
        )
    }
    /// Fail explicitly instead of interpreting wide content as ordinary sRGB.
    pub fn require(self, feature: PipelineFeature) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.supports(feature),
            "media pipeline feature is not implemented: {feature:?}"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests;
