//! Opt-in source-sized SDR images. Existing GPUI images keep their atlas path.
mod filter;
mod loader;
pub use filter::{FilteredImage, ImageFilter};
mod mipmaps;
mod texture;
mod upload;
mod video;
#[cfg(target_os = "macos")]
pub use gpui_apple::custom_draw::core_video::{CoreVideoFrame, Nv12Matrix};
#[cfg(all(unix, feature = "wgpu"))]
pub use gpui_wgpu::custom_draw::dma_buf::{DmaBufFrame, DmaBufLayout};
#[cfg(target_os = "windows")]
pub use gpui_windows::custom_draw::dxgi::DxgiFrame;
pub use video::{VideoFrame, VideoSender, VideoStats, VideoStream, VideoView, video, video_frame};

pub use loader::{ImageJob, LoadStatus, MediaLimits, MediaLoader};
pub use texture::{MediaImage, UploadStatus};
pub use upload::{FrameUploadLimits, UploadLimits, UploadStats};

use gpui::{
    App, Bounds, Corners, CustomGeometry, Element, ElementId, GlobalElementId, InspectorElementId,
    IntoElement, LayoutId, Pixels, Style, StyleRefinement, Styled, Window,
};
use std::sync::Arc;

/// Paint an immutable source-sized image with native clipping and rounded corners.
/// Layout size is explicit. This first API stretches to the supplied rectangle.
impl MediaImage {
    pub fn paint(&self, window: &mut Window, bounds: Bounds<Pixels>, radii: Corners<Pixels>) {
        self.paint_filtered(window, bounds, radii, ImageFilter::new());
    }
    pub fn paint_filtered(
        &self,
        window: &mut Window,
        bounds: Bounds<Pixels>,
        radii: Corners<Pixels>,
        filter: ImageFilter,
    ) {
        if bounds.is_empty()
            || bounds.intersect(&window.content_mask().bounds).is_empty()
            || window.element_opacity() <= 0.
        {
            return;
        }
        if !matches!(
            self.upload_status(),
            UploadStatus::Ready | UploadStatus::Failed(_)
        ) {
            window.request_animation_frame();
        }
        window.paint_custom(
            CustomGeometry::RoundedRect {
                bounds,
                corner_radii: radii,
            },
            Arc::new(self.filtered(filter)),
        );
    }
}

/// A styled image viewport. Construct once-loaded images with `MediaLoader` and
/// retain the image in the host view. Uses a direct source texture, not the atlas.
/// The image stretches to this element's bounds; set an appropriate aspect ratio.
pub fn image_view(image: MediaImage) -> MediaImageView {
    MediaImageView {
        image,
        filter: ImageFilter::new(),
        style: StyleRefinement::default(),
    }
}
/// Styled image viewport with native background, opacity, and corner coverage.
pub struct MediaImageView {
    image: MediaImage,
    filter: ImageFilter,
    style: StyleRefinement,
}
impl MediaImageView {
    pub fn filter(mut self, filter: ImageFilter) -> Self {
        self.filter = filter;
        self
    }
}
impl Styled for MediaImageView {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}
impl IntoElement for MediaImageView {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for MediaImageView {
    type RequestLayoutState = Style;
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Style) {
        use refineable::Refineable;
        let mut style = Style::default();
        style.refine(&self.style);
        (window.request_layout(style.clone(), [], cx), style)
    }
    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _style: &mut Style,
        _window: &mut Window,
        _cx: &mut App,
    ) {
    }
    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        style: &mut Style,
        _prepaint: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let radii = style
            .corner_radii
            .to_pixels(window.rem_size())
            .clamp_radii_for_quad_size(bounds.size);
        window.with_element_opacity(style.opacity, |window| {
            style.paint(bounds, window, cx, |window, _| {
                self.image
                    .paint_filtered(window, bounds, radii, self.filter)
            });
        });
    }
}

#[cfg(test)]
mod tests;

/// Build a complete alpha-aware, linear-light RGBA8 mip chain synchronously.
/// Intended for already-decoded small fixtures and hosts with their own task
/// scheduling. Large images should use native `MediaLoader` workers; this call
/// does not move CPU work off the calling thread or reserve loader budgets.
pub fn mipmapped_rgba8(
    width: u32,
    height: u32,
    rgba: Vec<u8>,
) -> anyhow::Result<gpui::ImageTextureData> {
    mipmaps::build(width, height, rgba, || false)
}
