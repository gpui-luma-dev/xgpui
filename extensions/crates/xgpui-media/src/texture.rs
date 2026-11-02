use crate::loader::Reservation;
use gpui::ImageTextureData;
use std::sync::Arc;

/// Immutable decoded image. Clones share CPU pixels, their payload reservation
/// and portable source identity. GPU uploads and completion are renderer-owned.
#[derive(Clone)]
pub struct MediaImage(Arc<Inner>);
struct Inner {
    data: ImageTextureData,
    content: gpui::content::Content,
    source_color: crate::pipeline::PixelColor,
}
impl std::fmt::Debug for MediaImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaImage")
            .field("width", &self.width())
            .field("height", &self.height())
            .finish()
    }
}
impl MediaImage {
    pub(crate) fn new_with_color(
        data: ImageTextureData,
        reservation: Arc<Reservation>,
        source_color: crate::pipeline::PixelColor,
    ) -> Self {
        let pixels = data.pixels().clone().with_owner(reservation);
        let content = gpui::content::Content::image(gpui::content::ImageAsset::ready(pixels));
        Self(Arc::new(Inner {
            data,
            content,
            source_color,
        }))
    }
    /// Immutable source for portable graphs. Clones share pixels, identity and
    /// the loader's source-memory reservation; no GPU work occurs here.
    pub fn content(&self) -> gpui::content::Content {
        self.0.content.clone()
    }
    /// Ordinary image presentation: source-sized mip sampling and an explicit
    /// SDR sRGB preview. Use the source directly with an explicit wide output
    /// policy when preserving its original wide gamut.
    pub fn draw(&self) -> gpui::content::ContentDraw {
        use gpui::content::*;
        let sampler = Sampler {
            mip_filter: (self.mip_count() > 1).then_some(FilterMode::Linear),
            ..Sampler::linear_clamp()
        };
        ContentDraw::new(self.content())
            .mapping(
                ImageMapping::default()
                    .sampler(sampler)
                    .expect("clamped image sampler"),
            )
            .color_output(ContentColorOutput::SrgbPreview)
    }
    /// Original supplied pixel interpretation, before canonical upload conversion.
    pub fn source_color(&self) -> crate::pipeline::PixelColor {
        self.0.source_color.clone()
    }
    pub fn format(&self) -> gpui::ImageTextureFormat {
        self.0.data.format()
    }
    pub fn width(&self) -> u32 {
        self.0.data.width()
    }
    pub fn height(&self) -> u32 {
        self.0.data.height()
    }
    pub fn mip_count(&self) -> usize {
        self.0.data.levels().len()
    }
    pub fn payload_bytes(&self) -> usize {
        self.0.data.byte_len()
    }
}
