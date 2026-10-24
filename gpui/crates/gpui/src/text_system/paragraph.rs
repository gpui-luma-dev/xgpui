//! Optional paragraph interop. Layout providers retain their own interaction data.
use crate::{Bounds, FontRun, Hsla, Pixels, Point, Result, ShapedRun, Size, Window};
use std::sync::Arc;

/// Side of a logical boundary at a wrap or bidirectional transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextAffinity {
    /// Position associated with the preceding logical cluster.
    Upstream,
    /// Position associated with the following logical cluster.
    Downstream,
}

/// Paragraph-relative UTF-8 position; providers validate and snap cluster edges.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParagraphPosition {
    /// Logical byte offset in the original source.
    pub byte_index: usize,
    /// Visual side of this logical boundary.
    pub affinity: TextAffinity,
}

/// Physical horizontal movement, independent of logical text direction.
#[derive(Clone, Copy, Debug)]
pub enum VisualDirection {
    /// Move left in visual order.
    Left,
    /// Move right in visual order.
    Right,
}

/// Immutable shaping result. Reflow must not mutate previously published layouts.
pub trait PlatformParagraphShape: Send + Sync {
    /// Break at a positive finite logical width, with automatic direction/start alignment.
    fn layout(&self, width: Pixels) -> Result<Arc<dyn PlatformParagraphLayout>>;
}

/// Immutable paragraph paint and interaction data, all in paragraph-local pixels.
///
/// Glyph positions include their line baseline, measured from the paragraph top.
/// Glyph indices remain logical UTF-8 offsets even when paint order is visual.
pub trait PlatformParagraphLayout: Send + Sync {
    /// Original unmodified source.
    fn source(&self) -> &str;
    /// Logical font size shared by every run in this initial interface.
    fn font_size(&self) -> Pixels;
    /// Paragraph layout size, including its allocated wrapping width.
    fn size(&self) -> Size<Pixels>;
    /// Resolved faces/glyphs in visual paint order; never requested descriptor IDs.
    fn runs(&self) -> &[ShapedRun];
    /// Find a logical boundary at a local point.
    fn hit_test(&self, point: Point<Pixels>) -> Result<ParagraphPosition>;
    /// A one-pixel caret; invalid positions return an error.
    fn caret_bounds(&self, position: ParagraphPosition) -> Result<Bounds<Pixels>>;
    /// Move to a neighboring visual cluster boundary.
    fn move_visual(
        &self,
        position: ParagraphPosition,
        direction: VisualDirection,
    ) -> Result<ParagraphPosition>;
    /// Possibly discontiguous selection rectangles.
    fn selection_bounds(
        &self,
        anchor: ParagraphPosition,
        focus: ParagraphPosition,
    ) -> Result<Vec<Bounds<Pixels>>>;
}

/// Paint visual glyph runs through the normal bounds cache and sprite atlas.
///
/// Call during element paint. `color_for_index` resolves color by logical source
/// index (not a forward-only style iterator). Backgrounds, selection and decoration
/// geometry are caller-owned. No font resolution or text shaping occurs here.
pub fn paint_paragraph(
    layout: &dyn PlatformParagraphLayout,
    origin: Point<Pixels>,
    color_for_index: impl Fn(usize) -> Hsla,
    window: &mut Window,
) -> Result<()> {
    for run in layout.runs() {
        for glyph in &run.glyphs {
            let position = origin + glyph.position;
            if glyph.is_emoji {
                window.paint_emoji(position, run.font_id, glyph.id, layout.font_size())?;
            } else {
                window.paint_glyph(
                    position,
                    run.font_id,
                    glyph.id,
                    layout.font_size(),
                    color_for_index(glyph.index),
                )?;
            }
        }
    }
    Ok(())
}

// FontRun is intentionally the same descriptor identity boundary as flat lines.
/// Borrowed shaping request for one logical paragraph. Direction is inferred and
/// alignment is start; future shaping options must also be added to cache keys.
pub struct ParagraphRequest<'a> {
    /// Original paragraph UTF-8 source.
    pub text: &'a str,
    /// Shared logical font size.
    pub font_size: Pixels,
    /// Ordered runs covering source bytes, or empty for the backend default.
    pub runs: &'a [FontRun],
}
