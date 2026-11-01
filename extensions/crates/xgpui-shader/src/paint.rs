//! Extension-owned paint values and convenience dispatch; GPUI only records geometry.
use gpui::content::{Content, ContentDraw};
use gpui::{
    Background, CustomContent, CustomGeometry, DevicePixels, Edges, Hsla, PaintQuad, Path, Pixels,
    Size, Window,
};
use std::sync::Arc;
/// CPU-side paint source. Native paints keep their existing GPU representation.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum Paint {
    /// An existing GPUI background.
    Native(Background),
    /// Portable immutable GPU source and presentation policy; runtime-only.
    #[serde(skip)]
    Content(ContentDraw),
}
impl PartialEq for Paint {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Native(a), Self::Native(b)) => a == b,
            (Self::Content(a), Self::Content(b)) => {
                a.content.id() == b.content.id() && a.presentation == b.presentation
            }
            _ => false,
        }
    }
}
impl From<Content> for Paint {
    fn from(content: Content) -> Self {
        Self::Content(ContentDraw::new(content))
    }
}
impl From<ContentDraw> for Paint {
    fn from(draw: ContentDraw) -> Self {
        Self::Content(draw)
    }
}
impl From<Background> for Paint {
    fn from(value: Background) -> Self {
        Self::Native(value)
    }
}
impl From<Hsla> for Paint {
    fn from(value: Hsla) -> Self {
        Self::Native(value.into())
    }
}
impl From<gpui::Rgba> for Paint {
    fn from(value: gpui::Rgba) -> Self {
        Self::Native(value.into())
    }
}
/// Extension convenience methods; native GPUI paint methods keep their original API.
pub trait WindowPaintExt {
    fn paint_path_with_paint(&mut self, path: Path<Pixels>, paint: impl Into<Paint>);
    fn paint_quad_with_paints(
        &mut self,
        quad: PaintQuad,
        fill: impl Into<Paint>,
        border: impl Into<Paint>,
    );
}
impl WindowPaintExt for Window {
    fn paint_path_with_paint(&mut self, path: Path<Pixels>, paint: impl Into<Paint>) {
        match paint.into() {
            Paint::Native(color) => self.paint_path(path, color),
            Paint::Content(draw) => self.paint_content(CustomGeometry::Path(path), draw),
        }
    }
    fn paint_quad_with_paints(
        &mut self,
        mut quad: PaintQuad,
        fill: impl Into<Paint>,
        border: impl Into<Paint>,
    ) {
        match fill.into() {
            Paint::Native(color) if !color.is_transparent() => {
                quad.background = color;
                quad.border_color = gpui::transparent_black();
                self.paint_quad(quad.clone());
            }
            Paint::Native(_) => {}
            Paint::Content(draw) => self.paint_content(
                CustomGeometry::RoundedRect {
                    bounds: quad.bounds,
                    corner_radii: quad.corner_radii,
                },
                draw,
            ),
        }
        // Border dispatch must not emit native strips or prepare an adapter when
        // no border can be visible. Custom source alpha remains unknown.
        if quad.border_widths == Edges::default() {
            return;
        }
        match border.into() {
            Paint::Native(color) if !color.is_transparent() => {
                if let Some(color) = color.as_solid() {
                    quad.background = gpui::transparent_black().into();
                    quad.border_color = color;
                    self.paint_quad(quad);
                } else {
                    self.paint_custom(
                        CustomGeometry::Border(quad),
                        Arc::new(NativePaintSource(color)),
                    );
                }
            }
            Paint::Native(_) => {}
            Paint::Content(draw) => self.paint_content(CustomGeometry::Border(quad), draw),
        }
    }
}
// Native background border sampling uses the core native-paint recording hook.
#[derive(Debug)]
struct NativePaintSource(Background);
impl CustomContent for NativePaintSource {
    fn prepare(
        &self,
        _: &dyn std::any::Any,
        _: Size<DevicePixels>,
    ) -> anyhow::Result<Box<dyn std::any::Any>> {
        Ok(Box::new(self.0))
    }
}
