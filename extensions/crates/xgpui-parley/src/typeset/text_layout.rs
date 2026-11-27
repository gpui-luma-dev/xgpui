//! Plain GPUI text laid out by the `typeset()` engine: a
//! [`gpui::TextLayoutProvider`] installed with [`install_text_layout`].
use super::{
    Breaks, Fitted, LINE_SEPARATOR, PaintRun, Pass, Piece, Prepared, SpecStyle, TypesetEngine,
    build_spec, install, paint_prepared, prepare, source_index,
};
use crate::{ParleyTextSystem, TextDirection};
use gpui::{
    App, AvailableSpace, Bounds, ParagraphPosition, Pixels, Point, ProvidedTextLayout, Size,
    TextAffinity, TextLayoutProvider, TextLayoutRequest, Window, point, px,
};
use icu_properties::{CodePointMapData, props::BidiClass};
use std::{rc::Rc, sync::Arc};

/// An explicit base direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BaseDir {
    /// Left to right.
    Ltr,
    /// Right to left.
    Rtl,
}

impl From<BaseDir> for TextDirection {
    fn from(direction: BaseDir) -> Self {
        match direction {
            BaseDir::Ltr => Self::LeftToRight,
            BaseDir::Rtl => Self::RightToLeft,
        }
    }
}

/// Base-direction policy for plain text laid out by [`install_text_layout`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextLayoutDirection {
    /// Infer direction from the first strong character, falling back to LTR.
    #[default]
    Auto,
    /// Infer direction, using this fallback when there is no strong character
    /// outside a directional isolate.
    AutoOr(BaseDir),
    /// Use this base direction for every plain text layout.
    Fixed(BaseDir),
}

struct PlainTextDirection(TextLayoutDirection);

impl gpui::Global for PlainTextDirection {}

fn text_layout_direction(cx: &App) -> TextLayoutDirection {
    cx.try_global::<PlainTextDirection>()
        .map_or(TextLayoutDirection::Auto, |setting| setting.0)
}

/// Set the app-wide base direction for plain text laid out by
/// [`install_text_layout`]. May be called before installation or at runtime.
/// A change refreshes the app's windows. `typeset()` uses its own direction.
pub fn set_text_layout_direction(direction: TextLayoutDirection, cx: &mut App) {
    if text_layout_direction(cx) != direction {
        cx.set_global(PlainTextDirection(direction));
        cx.refresh_windows();
    }
}

pub(super) fn resolve_direction(policy: TextLayoutDirection, text: &str) -> TextDirection {
    let fallback = match policy {
        TextLayoutDirection::Auto => return TextDirection::Auto,
        TextLayoutDirection::Fixed(direction) => return direction.into(),
        TextLayoutDirection::AutoOr(fallback) => fallback,
    };

    // UAX #9 P2 skips strong characters inside directional isolates.
    let classes = CodePointMapData::<BidiClass>::new();
    let mut isolate_depth = 0;
    for character in text.chars() {
        match classes.get(character) {
            BidiClass::LeftToRightIsolate
            | BidiClass::RightToLeftIsolate
            | BidiClass::FirstStrongIsolate => isolate_depth += 1,
            BidiClass::PopDirectionalIsolate if isolate_depth > 0 => isolate_depth -= 1,
            BidiClass::LeftToRight | BidiClass::RightToLeft | BidiClass::ArabicLetter
                if isolate_depth == 0 =>
            {
                return TextDirection::Auto;
            }
            _ => {}
        }
    }
    fallback.into()
}

/// Lay out GPUI's plain text elements (`div().child("…")`, `StyledText` and
/// the other built-in text elements) with the installed Parley engine, so
/// wrapped right-to-left and mixed text, line clamps, truncation and start/end
/// alignment behave as they do in [`typeset`](super::typeset). Also installs
/// the engine behind `typeset()`, as [`install`] does.
///
/// `parley` must be the text system installed with
/// `Application::with_platform_and_text_system`. Use [`set_text_layout_direction`]
/// for the app default and `typeset()` for an explicit direction or language.
/// Undo with `App::clear_text_layout_provider`.
pub fn install_text_layout(parley: Arc<ParleyTextSystem>, cx: &mut App) {
    install(parley, cx);
    cx.set_text_layout_provider(Rc::new(ParleyTextLayout));
}

struct ParleyTextLayout;

impl TextLayoutProvider for ParleyTextLayout {
    fn measure(
        &self,
        request: &TextLayoutRequest,
        known_dimensions: Size<Option<Pixels>>,
        available_space: Size<AvailableSpace>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Box<dyn ProvidedTextLayout>> {
        let engine = cx.try_global::<TypesetEngine>()?;
        let (mut spec, runs, breaks) = build_spec(
            &request.text,
            request.runs.to_vec(),
            SpecStyle {
                font_size: request.font_size,
                line_height: request.line_height,
                white_space: request.white_space,
                text_overflow: request.text_overflow.as_ref(),
                line_clamp: request.line_clamp,
                text_align: request.text_align,
            },
            TextDirection::Auto,
            None,
            window,
        );
        // build_spec normalizes hard breaks to line separators within one
        // Parley paragraph. Resolve direction against the shaped text.
        spec.options.direction = resolve_direction(text_layout_direction(cx), &spec.text);
        match engine.fit(&spec, known_dimensions, available_space) {
            Ok(fitted) => Some(Box::new(Laid {
                len: request.text.len(),
                fitted,
                runs,
                breaks,
                prepared: None,
                size: Size::default(),
            })),
            Err(error) => {
                engine.parley.report_layout_error(&error);
                None
            }
        }
    }
}

/// One element's fitted text and, after prepaint, its placed layout.
struct Laid {
    /// Byte length of the element's text.
    len: usize,
    fitted: Fitted,
    runs: Arc<[PaintRun]>,
    breaks: Breaks,
    prepared: Option<Prepared>,
    /// The element's final size.
    size: Size<Pixels>,
}

impl Laid {
    /// Height of the lines that show: all of them, or those a clamp allows.
    fn visible_height(&self, prepared: &Prepared) -> Pixels {
        prepared
            .clamp_height
            .map_or(self.size.height, |height| height.min(self.size.height))
    }
}

impl ProvidedTextLayout for Laid {
    fn size(&self) -> Size<Pixels> {
        self.fitted.size
    }

    fn prepaint(&mut self, bounds: Bounds<Pixels>, _: &mut Window, cx: &mut App) {
        self.size = bounds.size;
        self.prepared = cx
            .try_global::<TypesetEngine>()
            .and_then(|engine| prepare(engine, &self.fitted, bounds.size.width));
    }

    fn paint_background(
        &self,
        origin: Point<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) -> anyhow::Result<()> {
        if let Some(prepared) = &self.prepared {
            let bounds = Bounds::new(origin, self.size);
            paint_prepared(prepared, &self.runs, bounds, Pass::Background, window, cx);
        }
        Ok(())
    }

    fn paint_foreground(
        &self,
        origin: Point<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) -> anyhow::Result<()> {
        if let Some(prepared) = &self.prepared {
            let bounds = Bounds::new(origin, self.size);
            paint_prepared(prepared, &self.runs, bounds, Pass::Foreground, window, cx);
        }
        Ok(())
    }

    fn index_for_position(&self, position: Point<Pixels>) -> Result<usize, usize> {
        let Some(prepared) = &self.prepared else {
            return Err(0);
        };
        let local = position - point(prepared.offset, px(0.0));
        let shown = prepared
            .layout
            .hit_test(local)
            .map_or(0, |hit| hit.byte_index);
        let index = self
            .breaks
            .to_source(source_index(&prepared.pieces, shown))
            .min(self.len);
        let inside = position.x >= px(0.0)
            && position.x < self.size.width
            && position.y >= px(0.0)
            && position.y < self.visible_height(prepared);
        if inside && index < self.len {
            Ok(index)
        } else {
            Err(index)
        }
    }

    fn position_for_index(&self, index: usize) -> Option<Point<Pixels>> {
        let prepared = self.prepared.as_ref()?;
        if index > self.len {
            return None;
        }
        let shown = shown_index(&prepared.pieces, self.breaks.to_normalized(index))?;
        let caret = prepared
            .layout
            .caret_bounds(ParagraphPosition {
                byte_index: shown,
                affinity: TextAffinity::Downstream,
            })
            .ok()?;
        if caret.origin.y >= self.visible_height(prepared) {
            return None;
        }
        Some(caret.origin + point(prepared.offset, px(0.0)))
    }

    fn wrapped_text(&self) -> String {
        let Some(prepared) = &self.prepared else {
            return String::new();
        };
        let text = prepared.layout.source();
        let shown = self.fitted.line_clamp.unwrap_or(usize::MAX);
        prepared
            .layout
            .parley_layout()
            .lines()
            .take(shown)
            .map(|line| text[line.text_range()].trim_end_matches(LINE_SEPARATOR))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// The shown index displaying an untruncated index, if truncation kept it.
fn shown_index(pieces: &[Piece], index: usize) -> Option<usize> {
    pieces.iter().find_map(|piece| {
        (piece.copied && (piece.source..=piece.source + piece.shown.len()).contains(&index))
            .then(|| piece.shown.start + (index - piece.source))
    })
}
