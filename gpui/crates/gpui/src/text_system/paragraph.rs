//! Optional paragraph interop. Layout providers retain their own interaction data.
use crate::{Bounds, FontRun, Hsla, Pixels, Point, Result, ShapedRun, Size, Window};
use std::sync::Arc;

/// What a text backend's paragraph layout supports, so callers need not infer
/// support from `layout_paragraph` errors.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum ParagraphSupport {
    /// `layout_paragraph` returns an error.
    #[default]
    Unsupported,
    /// Wrapping, hit testing, carets, visual movement and selection, in
    /// left-to-right visual order only. Runs keep the backend's line shaping.
    LeftToRight,
    /// Everything in `LeftToRight`, plus bidirectional reordering and automatic
    /// base direction.
    Full,
}

impl ParagraphSupport {
    /// `layout_paragraph` can succeed on this backend.
    pub fn supports_layout(self) -> bool {
        self >= Self::LeftToRight
    }

    /// Mixed-direction and right-to-left paragraphs are laid out in visual order.
    pub fn supports_bidi(self) -> bool {
        self >= Self::Full
    }
}

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

#[cfg(test)]
mod tests {
    use super::ParagraphSupport;
    use crate::{NoopTextSystem, TextSystem};
    use std::sync::Arc;

    #[test]
    fn paragraph_support_levels_and_backend_default() {
        assert!(!ParagraphSupport::Unsupported.supports_layout());
        assert!(ParagraphSupport::LeftToRight.supports_layout());
        assert!(!ParagraphSupport::LeftToRight.supports_bidi());
        assert!(ParagraphSupport::Full.supports_layout());
        assert!(ParagraphSupport::Full.supports_bidi());
        assert_eq!(ParagraphSupport::default(), ParagraphSupport::Unsupported);

        let text_system = TextSystem::new(Arc::new(NoopTextSystem::new()));
        assert_eq!(
            text_system.paragraph_support(),
            ParagraphSupport::Unsupported
        );
    }
}
