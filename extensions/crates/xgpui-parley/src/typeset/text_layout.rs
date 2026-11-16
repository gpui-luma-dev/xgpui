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
use std::{rc::Rc, sync::Arc};

/// Lay out GPUI's plain text elements (`div().child("…")`, `StyledText` and
/// the other built-in text elements) with the installed Parley engine, so
/// wrapped right-to-left and mixed text, line clamps, truncation and start/end
/// alignment behave as they do in [`typeset`](super::typeset). Also installs
/// the engine behind `typeset()`, as [`install`] does.
///
/// `parley` must be the text system installed with
/// `Application::with_platform_and_text_system`. Each paragraph's direction
/// comes from its first strong character; use `typeset()` for an explicit
/// direction or language. Undo with `App::clear_text_layout_provider`.
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
        let (spec, runs, breaks) = build_spec(
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
