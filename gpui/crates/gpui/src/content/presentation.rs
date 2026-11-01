//! Portable window presentation requests. Native realizations remain renderer-owned.
use super::*;
use crate::{CustomContent, DevicePixels, PreparationRequest, Size};
use std::{any::Any, sync::Arc};

/// Stable logical paint identity, scoped to each window. Allocate once per paint
/// role and retain across content replacement. Removal retires its history.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PresentationSlotId(u64);
impl PresentationSlotId {
    /// Create a fresh paint identity. Do not recreate it on every render.
    pub fn new() -> Self {
        Self(next_content_identity())
    }
}
impl Default for PresentationSlotId {
    fn default() -> Self {
        Self::new()
    }
}
/// Visibility while the requested generation is being produced.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PendingPresentation {
    /// Reuse this slot's last valid result; anonymous/first draws are transparent.
    #[default]
    RetainPrevious,
    /// Show no content until the requested generation completes.
    Transparent,
}
/// How a failed content generation affects the window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ContentErrorPresentation {
    /// Report once for this generation and retain this slot's last valid result.
    #[default]
    ReportAndRetainPrevious,
    /// Report once and show transparent content.
    ReportAndTransparent,
    /// Return the content error from rendering this frame.
    FailFrame,
}
/// Presentation policy, independent of source identity and graph realization.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContentPresentation {
    /// Stable paint identity; duplicate slots in one scene are rejected.
    pub slot: Option<PresentationSlotId>,
    /// Visibility before native completion.
    pub pending: PendingPresentation,
    /// Behavior after validation or execution fails.
    pub on_error: ContentErrorPresentation,
}
/// Immutable portable source and presentation policy. Replays keep their slot.
/// The full normalized source stretches over unclipped destination geometry;
/// coverage, clipping and element opacity are applied by the compositor.
#[derive(Clone, Debug)]
pub struct ContentDraw {
    /// Immutable portable producer.
    pub content: Content,
    /// Window presentation policy.
    pub presentation: ContentPresentation,
}
impl ContentDraw {
    /// Anonymous draw: initial/pending content is transparent by default.
    pub fn new(content: Content) -> Self {
        Self {
            content,
            presentation: ContentPresentation::default(),
        }
    }
    /// Associate this draw with a stable logical paint slot.
    pub fn slot(mut self, slot: PresentationSlotId) -> Self {
        self.presentation.slot = Some(slot);
        self
    }
    /// Replace the complete presentation policy.
    pub fn presentation(mut self, presentation: ContentPresentation) -> Self {
        self.presentation = presentation;
        self
    }
    /// Renderer bridge retaining this portable request in the scene. Application
    /// code normally uses `Window::paint_content` instead.
    pub fn into_scene_content(self) -> Arc<dyn CustomContent> {
        Arc::new(self)
    }
}
/// CPU description extracted from a retained scene record by a renderer.
#[derive(Clone, Debug)]
pub struct ContentDrawRequest {
    /// Portable draw policy and immutable source.
    pub draw: ContentDraw,
    /// Original logical geometry, exact raster extent and window scale.
    pub request: ContentRequest,
}
impl CustomContent for ContentDraw {
    fn portable_content(
        &self,
        request: &PreparationRequest,
    ) -> Result<Option<ContentDrawRequest>, ContentError> {
        let raster = request.raster_size();
        Ok(Some(ContentDrawRequest {
            draw: self.clone(),
            request: ContentRequest::new(
                request.logical_size(),
                ImageExtent::new(raster.width.0 as u32, raster.height.0 as u32)?,
                request.scale_factor(),
            )?,
        }))
    }
    fn prepare(&self, _: &dyn Any, _: Size<DevicePixels>) -> anyhow::Result<Box<dyn Any>> {
        Err(ContentError::new(
            ContentErrorKind::UnsupportedCapability,
            "this renderer does not support portable content presentation",
        )
        .into())
    }
}
