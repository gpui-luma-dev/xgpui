//! Optional layout for GPUI's plain text elements.
//!
//! By default, plain text elements (`&'static str`, `String`, `SharedString`,
//! [`crate::Text`], [`crate::StyledText`] and [`crate::InteractiveText`]) shape
//! their text into flat lines. A text engine with paragraph layout, such as one
//! with full bidirectional support, can register a [`TextLayoutProvider`] to lay
//! these elements out instead. Elements that shape text themselves (for
//! example through `shape_line`) are not affected.
use crate::{
    App, AvailableSpace, Bounds, Global, Pixels, Point, SharedString, Size, TextAlign,
    TextOverflow, TextRun, WhiteSpace, Window,
};
use anyhow::Result;
use std::{rc::Rc, sync::Arc};

/// Lays out GPUI's plain text elements in place of flat lines.
///
/// Register one with [`App::set_text_layout_provider`].
pub trait TextLayoutProvider: 'static {
    /// Measure `request` for the given layout constraints, as a measure function
    /// passed to `Window::request_measured_layout` would. Return `None` to keep
    /// GPUI's flat lines for this text, for example for runs the provider can't
    /// lay out.
    ///
    /// This may be called several times per frame with different constraints.
    /// The element keeps the last layout and calls
    /// [`ProvidedTextLayout::prepaint`] with its final bounds.
    fn measure(
        &self,
        request: &TextLayoutRequest,
        known_dimensions: Size<Option<Pixels>>,
        available_space: Size<AvailableSpace>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Box<dyn ProvidedTextLayout>>;
}

/// The text and resolved text style a plain text element asks a
/// [`TextLayoutProvider`] to lay out.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct TextLayoutRequest {
    /// The element's text.
    pub text: SharedString,
    /// Styled runs covering `text` exactly.
    pub runs: Arc<[TextRun]>,
    /// The resolved font size.
    pub font_size: Pixels,
    /// The resolved, pixel-snapped line height.
    pub line_height: Pixels,
    /// Whether the text wraps.
    pub white_space: WhiteSpace,
    /// How text that doesn't fit is truncated, if at all.
    pub text_overflow: Option<TextOverflow>,
    /// The maximum number of lines to show, if limited.
    pub line_clamp: Option<usize>,
    /// The text's alignment.
    pub text_align: TextAlign,
}

/// A layout measured by a [`TextLayoutProvider`]. Positions are relative to the
/// element's origin, and indices are byte offsets into the request's text.
pub trait ProvidedTextLayout {
    /// The measured size.
    fn size(&self) -> Size<Pixels>;

    /// Called once per frame with the element's final bounds, before painting
    /// and before any geometry query. The element may be wider or narrower than
    /// the last measured size.
    fn prepaint(&mut self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App);

    /// Paint the run backgrounds, without glyphs, with the element at `origin`.
    fn paint_background(
        &self,
        origin: Point<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) -> Result<()>;

    /// Paint the glyphs and decorations, without run backgrounds, with the
    /// element at `origin`.
    fn paint_foreground(
        &self,
        origin: Point<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) -> Result<()>;

    /// The byte index at `position`: `Ok` when the position is over the text,
    /// otherwise `Err` with the closest index.
    fn index_for_position(&self, position: Point<Pixels>) -> Result<usize, usize>;

    /// The position of the caret before the byte at `index`, at the top of its line.
    fn position_for_index(&self, index: usize) -> Option<Point<Pixels>>;

    /// The displayed text, with each soft wrap as a newline.
    fn wrapped_text(&self) -> String;
}

struct GlobalTextLayoutProvider(Rc<dyn TextLayoutProvider>);

impl Global for GlobalTextLayoutProvider {}

impl App {
    /// Lay out plain text elements with `provider` from the next layout on,
    /// replacing any provider already registered.
    pub fn set_text_layout_provider(&mut self, provider: Rc<dyn TextLayoutProvider>) {
        self.set_global(GlobalTextLayoutProvider(provider));
    }

    /// Return plain text elements to GPUI's flat lines from the next layout on.
    pub fn clear_text_layout_provider(&mut self) {
        if self.has_global::<GlobalTextLayoutProvider>() {
            self.remove_global::<GlobalTextLayoutProvider>();
        }
    }

    pub(crate) fn text_layout_provider(&self) -> Option<Rc<dyn TextLayoutProvider>> {
        self.try_global::<GlobalTextLayoutProvider>()
            .map(|global| global.0.clone())
    }
}
