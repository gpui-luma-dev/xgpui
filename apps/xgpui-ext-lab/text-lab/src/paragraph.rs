//! Editable paragraph probe; uses Parley shaping/geometry through the extension
//! API, a small lab-owned layout cache and GPUI's glyph atlas.
#[path = "paragraph/input.rs"]
mod input;
#[path = "paragraph/strings.rs"]
mod strings;

use anyhow::{Result, ensure};
use gpui::{
    App, Bounds, Context, ElementInputHandler, FocusHandle, Focusable, Font, FontFallbacks,
    FontRun, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, ParagraphPosition, Pixels,
    PlatformParagraphLayout, Point, Render, TextAffinity, TextSystem, VisualDirection, Window,
    WindowTextSystem, canvas, div, fill, paint_paragraph, prelude::*, px, rgb,
};
use strings::{UI, UiStrings};
use xgpui_parley::{
    ParagraphOptions, ParagraphShape, ParleyTextSystem, TextDirection, TextLanguage, Typeset,
    typeset,
};
#[cfg(not(target_family = "wasm"))]
use std::time::Instant;
use std::{cell::RefCell, hint::black_box, sync::Arc};
#[cfg(target_family = "wasm")]
use web_time::Instant;

pub const FONTS: [&[u8]; 2] = [
    include_bytes!(
        "../../../../extensions/crates/xgpui-parley/tests/fonts/NotoSansArabic.ttf"
    ),
    include_bytes!(
        "../../../../extensions/crates/xgpui-parley/tests/fonts/NotoSansHebrew.ttf"
    ),
];
const TEXTS: [(&str, &str); 4] = [
    (
        "LTR base / mixed scripts",
        "office café אבג 123 مرحبا e\u{301} — mixed text wraps across lines, with Latin, Hebrew and Arabic fallback. Select across script boundaries.",
    ),
    (
        "RTL base / Latin and numbers",
        "אבג שלום 123 office مرحبا — mixed text and numbers wrap together. café e\u{301} אבג 456 مرحبا",
    ),
    (
        "Explicit break / NBSP / ZWSP",
        "abc אבג مرحبا\u{2028}Keep A\u{a0}B together; allow zero\u{200b}width breaks. office affine e\u{301} אבג 123 مرحبا\u{2028}",
    ),
    (
        "Language-sensitive digits",
        "۴۵۶۷ اعداد ۱۲۳ — choose fa or ur to compare Persian and Urdu digit forms.",
    ),
];
/// Base directions in button order; the default comes first.
const DIRECTIONS: [TextDirection; 3] = [
    TextDirection::Auto,
    TextDirection::RightToLeft,
    TextDirection::LeftToRight,
];
/// Control-label samples for the `typeset()` section: Latin, Hebrew, Arabic, mixed.
const CELLS: [&str; 4] = [
    "Quarterly revenue report for the northern region",
    "דוח הכנסות רבעוני לאזור הצפון והמרכז",
    "تقرير الإيرادات الفصلي للمنطقة الشمالية",
    "office אבג 123 مرحبا — mixed cell label",
];
const CELL_WIDTH: f32 = 150.0;
const TOOLTIP: &str = "Tooltips wrap within a maximum width: שלום עולם, مرحبا بالعالم, and Latin text share one paragraph.";
/// Content languages in button order; `None` leaves the language unspecified.
const LANGUAGES: [Option<&str>; 5] = [None, Some("ar"), Some("fa"), Some("ur"), Some("he")];

fn font() -> Font {
    let mut font = gpui::font("IBM Plex Sans");
    font.fallbacks = Some(FontFallbacks::from_fonts(vec![
        "Noto Sans Hebrew".into(),
        "Noto Sans Arabic".into(),
    ]));
    font
}
fn runs(text: &str, system: &TextSystem) -> [FontRun; 1] {
    [FontRun {
        len: text.len(),
        font_id: system.resolve_font(&font()),
    }]
}
fn start() -> ParagraphPosition {
    ParagraphPosition {
        byte_index: 0,
        affinity: TextAffinity::Downstream,
    }
}

#[derive(Clone)]
pub struct Timing {
    first: f64,
    warm: f64,
    reflow: f64,
}
impl Timing {
    fn label(&self) -> String {
        format!(
            "First {:.1} us | warm {:.2} us | reflow {:.1} us",
            self.first, self.warm, self.reflow
        )
    }
}
fn micros(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1e6
}
fn median(mut times: Vec<f64>) -> f64 {
    times.sort_by(f64::total_cmp);
    times[times.len() / 2]
}

/// A separate window cache keeps diagnostics from prewarming the live UI cache.
pub fn measure(system: Arc<TextSystem>, iterations: usize) -> Result<Vec<Timing>> {
    ensure!(
        (1..=1000).contains(&iterations),
        "expected 1..=1000 timing iterations"
    );
    let window = WindowTextSystem::new(system.clone());
    let mut results = Vec::new();
    for (label, text) in TEXTS {
        let runs = runs(text, &system);
        let now = Instant::now();
        let first = window.layout_paragraph(text.into(), px(24.0), &runs, px(320.0))?;
        let first_time = micros(now);
        let mut warm = Vec::new();
        let mut reflow = Vec::new();
        for _ in 0..iterations {
            let now = Instant::now();
            let cached = window.layout_paragraph(text.into(), px(24.0), &runs, px(320.0))?;
            warm.push(micros(now));
            ensure!(Arc::ptr_eq(&first, &cached), "expected paragraph cache hit");
            black_box(cached);
        }
        // Every width is new; measure real reflow with retained shaping, not cache hits.
        for i in 0..iterations {
            let width = px(180.0 + i as f32 * 0.1);
            let now = Instant::now();
            black_box(window.layout_paragraph(text.into(), px(24.0), &runs, width)?);
            reflow.push(micros(now));
        }
        let timing = Timing {
            first: first_time,
            warm: median(warm),
            reflow: median(reflow),
        };
        println!("Paragraph {label}: {}", timing.label());
        results.push(timing);
    }
    println!(
        "24px; first request per source at 320px, shared backend state; warm median exact Arc cache hit; reflow median distinct widths 180 + i*0.1px. Font resolution excluded. CPU only, no raster/GPU/frame timing. Diagnostic cache is separate from the UI."
    );
    Ok(results)
}

/// Shaping is reused until the text or options change; reflow until the width does.
struct LayoutCache {
    content: String,
    options: ParagraphOptions,
    shape: ParagraphShape,
    layout: Option<(Pixels, Arc<dyn PlatformParagraphLayout>)>,
}

pub struct ParagraphLab {
    parley: Arc<ParleyTextSystem>,
    cache: RefCell<Option<LayoutCache>>,
    sample: usize,
    content: String,
    marked: Option<std::ops::Range<usize>>,
    preferred_x: Option<Pixels>,
    reveal_caret: bool,
    wide: bool,
    direction: usize,
    language: usize,
    /// Index into `UI`: the language of the panel's own labels and controls.
    ui: usize,
    timings: Vec<Timing>,
    focus: FocusHandle,
    anchor: ParagraphPosition,
    caret: ParagraphPosition,
    dragging: bool,
    geometry: Option<(Bounds<Pixels>, Arc<dyn PlatformParagraphLayout>)>,
}
impl ParagraphLab {
    /// `parley` must be the text backend installed in this application, so its
    /// font IDs match GPUI's resolved fonts and glyph atlas.
    pub fn new(timings: Vec<Timing>, parley: Arc<ParleyTextSystem>, cx: &mut Context<Self>) -> Self {
        // Back `typeset()` elements with the same installed engine.
        xgpui_parley::typeset::install(parley.clone(), cx);
        Self {
            parley,
            cache: RefCell::new(None),
            sample: 0,
            content: TEXTS[0].1.to_owned(),
            marked: None,
            preferred_x: None,
            reveal_caret: false,
            wide: false,
            direction: 0,
            language: 0,
            ui: 0,
            timings,
            focus: cx.focus_handle(),
            anchor: start(),
            caret: start(),
            dragging: false,
            geometry: None,
        }
    }
    fn width(&self, window: &Window) -> Pixels {
        px(if self.wide { 480.0 } else { 240.0 })
            .min((window.viewport_size().width - px(64.0)).max(px(80.0)))
    }
    fn options(&self) -> ParagraphOptions {
        ParagraphOptions {
            direction: DIRECTIONS[self.direction],
            language: LANGUAGES[self.language].map(|tag| TextLanguage::parse(tag).unwrap()),
            ..Default::default()
        }
    }
    fn layout(&self, window: &Window) -> Result<Arc<dyn PlatformParagraphLayout>> {
        let width = self.width(window);
        let options = self.options();
        let mut cache = self.cache.borrow_mut();
        if cache
            .as_ref()
            .is_none_or(|cached| cached.content != self.content || cached.options != options)
        {
            let runs = runs(&self.content, window.text_system());
            let shape =
                self.parley
                    .prepare_paragraph_with_options(&self.content, px(24.0), &runs, options)?;
            *cache = Some(LayoutCache {
                content: self.content.clone(),
                options,
                shape,
                layout: None,
            });
        }
        let cached = cache.as_mut().unwrap();
        if let Some((cached_width, layout)) = &cached.layout
            && *cached_width == width
        {
            return Ok(layout.clone());
        }
        let layout: Arc<dyn PlatformParagraphLayout> = Arc::new(cached.shape.layout(width)?);
        cached.layout = Some((width, layout.clone()));
        Ok(layout)
    }
    fn select_point(&mut self, point: Point<Pixels>) {
        if let Some((bounds, layout)) = &self.geometry
            && let Ok(hit) = layout.hit_test(point - bounds.origin)
        {
            self.caret = hit;
            self.preferred_x = None;
        }
    }
    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.marked.is_some() {
            return;
        } // Native IME owns composition keys.
        if self.edit_key(event, window, cx) {
            cx.stop_propagation();
            return;
        }
        let modifiers = event.keystroke.modifiers;
        if modifiers.platform || modifiers.control || modifiers.alt {
            return;
        }
        let direction = match event.keystroke.key.as_str() {
            "left" => VisualDirection::Left,
            "right" => VisualDirection::Right,
            _ => return,
        };
        if let Ok(layout) = self.layout(window) {
            let next = if self.anchor.byte_index != self.caret.byte_index && !modifiers.shift {
                let Ok(a) = layout.caret_bounds(self.anchor) else {
                    return;
                };
                let Ok(b) = layout.caret_bounds(self.caret) else {
                    return;
                };
                let anchor_first = a.top() < b.top() || (a.top() == b.top() && a.left() < b.left());
                if anchor_first == matches!(direction, VisualDirection::Left) {
                    self.anchor
                } else {
                    self.caret
                }
            } else {
                let Ok(next) = layout.move_visual(self.caret, direction) else {
                    return;
                };
                next
            };
            self.caret = next;
            self.preferred_x = None;
            self.reveal_caret = true;
            cx.stop_propagation();
            if !event.keystroke.modifiers.shift {
                self.anchor = next;
            }
            cx.notify();
        }
    }
}
impl Focusable for ParagraphLab {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
impl Render for ParagraphLab {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ui = self.strings();
        let label = ui.samples[self.sample];
        let text = self.content.clone();
        let width = self.width(window);
        let layout = self.layout(window);
        let timing = &self.timings[self.sample];
        let direction = match DIRECTIONS[self.direction] {
            TextDirection::Auto => ui.auto,
            TextDirection::RightToLeft => ui.rtl_direction,
            TextDirection::LeftToRight => ui.ltr_direction,
        };
        let controls = self
            .row()
            .gap_3()
            .child(self.button("paragraph-ui", format!("{}: {}", ui.ui, ui.name)).on_click(
                cx.listener(|this, _, _, cx| {
                    this.ui = (this.ui + 1) % UI.len();
                    this.geometry = None;
                    this.dragging = false;
                    cx.notify();
                }),
            ))
            .child(self.button("paragraph-sample", ui.next_sample).on_click(cx.listener(
                |this, _, _, cx| {
                    this.sample = (this.sample + 1) % TEXTS.len();
                    this.content = TEXTS[this.sample].1.to_owned();
                    this.marked = None;
                    this.preferred_x = None;
                    this.reveal_caret = false;
                    this.anchor = start();
                    this.caret = start();
                    this.geometry = None;
                    this.dragging = false;
                    cx.notify();
                },
            )))
            .child(
                self.button("paragraph-width", if self.wide { ui.narrow } else { ui.wide })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.wide = !this.wide;
                        this.geometry = None;
                        this.dragging = false;
                        cx.notify();
                    })),
            )
            .child(
                self.button("paragraph-direction", format!("{}: {direction}", ui.direction))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.direction = (this.direction + 1) % DIRECTIONS.len();
                        this.preferred_x = None;
                        this.geometry = None;
                        this.dragging = false;
                        cx.notify();
                    })),
            )
            .child(
                self.button(
                    "paragraph-language",
                    format!("{}: {}", ui.language, LANGUAGES[self.language].unwrap_or(ui.none)),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.language = (this.language + 1) % LANGUAGES.len();
                    this.preferred_x = None;
                    this.geometry = None;
                    this.dragging = false;
                    cx.notify();
                })),
            );
        let mut root = div()
            .id("paragraph-lab")
            .size_full()
            .overflow_y_scroll()
            .bg(rgb(0x111827))
            .text_color(rgb(0xeff4fc))
            .font_family("IBM Plex Sans")
            .text_size(px(15.0))
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key))
            .child(div().text_size(px(24.0)).child(self.text(ui.title).with_id("paragraph-title")))
            .child(self.text(ui.select_help))
            .child(self.text(ui.edit_help))
            .child(controls)
            .child(self.text(format!("{label} | {} {:.0}px", ui.width, f32::from(width))))
            .child(self.text(format!(
                "{} {:.1} us | {} {:.2} us | {} {:.1} us",
                ui.first, timing.first, ui.warm, timing.warm, ui.reflow, timing.reflow
            )))
            .child(self.text(ui.cpu_note))
            .child(self.text(format!(
                "{} {} {:?} | {} {} {:?}",
                ui.anchor,
                self.anchor.byte_index,
                self.anchor.affinity,
                ui.caret,
                self.caret.byte_index,
                self.caret.affinity
            )));
        match layout {
            Ok(layout) => {
                let height = layout.size().height;
                let entity = cx.entity();
                let paint_entity = entity.clone();
                let marked = self.marked.clone();
                let focus = self.focus.clone();
                let reveal = self.reveal_caret;
                let anchor = self.anchor;
                let caret = self.caret;
                let color_boundary = text
                    .char_indices()
                    .nth(7)
                    .map_or(text.len(), |(index, _)| index);
                root = root.child(
                    div()
                        .id("paragraph-surface")
                        .p_3()
                        .bg(rgb(0xffffff))
                        .flex_none()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, event: &MouseDownEvent, window, cx| {
                                window.focus(&this.focus, cx);
                                this.select_point(event.position);
                                if !event.modifiers.shift {
                                    this.anchor = this.caret;
                                }
                                this.dragging = true;
                                cx.notify();
                            }),
                        )
                        .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                            if this.dragging {
                                this.select_point(event.position);
                                cx.notify();
                            }
                        }))
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(|this, _, _, _| {
                                this.dragging = false;
                            }),
                        )
                        .on_mouse_up_out(
                            MouseButton::Left,
                            cx.listener(|this, _, _, _| {
                                this.dragging = false;
                            }),
                        )
                        .child(
                            canvas(
                                move |bounds, window, cx| {
                                    if reveal && let Ok(rect) = layout.caret_bounds(caret) {
                                        window.request_autoscroll(Bounds::new(
                                            bounds.origin + rect.origin,
                                            rect.size,
                                        ));
                                    }
                                    entity.update(cx, |this, _| {
                                        this.reveal_caret = false;
                                        this.geometry = Some((bounds, layout.clone()));
                                    });
                                    layout
                                },
                                move |bounds, layout, window, cx| {
                                    window.handle_input(
                                        &focus,
                                        ElementInputHandler::new(bounds, paint_entity),
                                        cx,
                                    );
                                    if let Ok(rects) = layout.selection_bounds(anchor, caret) {
                                        for rect in rects {
                                            window.paint_quad(fill(
                                                Bounds::new(bounds.origin + rect.origin, rect.size),
                                                rgb(0xbdd7ff),
                                            ));
                                        }
                                    }
                                    if let Err(error) = paint_paragraph(
                                        layout.as_ref(),
                                        bounds.origin,
                                        |index| {
                                            if index < color_boundary {
                                                rgb(0x1d4ed8).into()
                                            } else {
                                                rgb(0x111827).into()
                                            }
                                        },
                                        window,
                                    ) {
                                        eprintln!("Paragraph paint: {error:#}");
                                    }
                                    if let Some(marked) = marked {
                                        let pos = |byte_index| ParagraphPosition {
                                            byte_index,
                                            affinity: TextAffinity::Downstream,
                                        };
                                        if let Ok(rects) = layout
                                            .selection_bounds(pos(marked.start), pos(marked.end))
                                        {
                                            for rect in rects {
                                                window.paint_quad(fill(
                                                    Bounds::new(
                                                        bounds.origin
                                                            + gpui::point(
                                                                rect.left(),
                                                                rect.bottom() - px(1.0),
                                                            ),
                                                        gpui::size(rect.size.width, px(1.0)),
                                                    ),
                                                    rgb(0x2563eb),
                                                ));
                                            }
                                        }
                                    }
                                    if let Ok(rect) = layout.caret_bounds(caret) {
                                        window.paint_quad(fill(
                                            Bounds::new(bounds.origin + rect.origin, rect.size),
                                            rgb(0xdc2626),
                                        ));
                                    }
                                },
                            )
                            .w(width)
                            .h(height),
                        ),
                );
            }
            Err(error) => {
                self.geometry = None;
                root = root.child(format!("Paragraph unavailable: {error:#}"));
            }
        }
        root.child(self.typeset_section())
    }
}

impl ParagraphLab {
    fn strings(&self) -> &'static UiStrings {
        &UI[self.ui]
    }

    /// A chrome label in the UI language, with the UI's base direction.
    fn text(&self, text: impl Into<gpui::SharedString>) -> Typeset {
        let ui = self.strings();
        let label = typeset(text).language(TextLanguage::parse(ui.tag).unwrap());
        if ui.rtl { label.rtl() } else { label.ltr() }
    }

    /// A horizontal row that mirrors for right-to-left UI languages.
    fn row(&self) -> gpui::Div {
        let row = div().flex();
        if self.strings().rtl {
            row.flex_row_reverse()
        } else {
            row
        }
    }

    fn button(
        &self,
        id: &'static str,
        label: impl Into<gpui::SharedString>,
    ) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .debug_selector(move || id.into())
            .p_2()
            .bg(rgb(0x374151))
            .cursor_pointer()
            .child(self.text(label))
    }

    /// One `typeset()` element with the panel's direction and language.
    fn typeset(&self, text: &'static str) -> xgpui_parley::Typeset {
        let options = self.options();
        let element = typeset(text).direction(options.direction);
        match options.language {
            Some(language) => element.language(language),
            None => element,
        }
    }

    /// Luma-style labels: truncated table cells, a wrapping tooltip and a
    /// two-line clamp, each beside native GPUI text in the same style.
    fn typeset_section(&self) -> impl IntoElement {
        let ui = self.strings();
        let cell = |selector: String| {
            div()
                .debug_selector(move || selector.clone())
                .w(px(CELL_WIDTH))
                .flex_none()
                .px_1()
                .bg(rgb(0x1f2937))
                .text_size(px(14.0))
        };
        let header = |cut: &'static str| {
            div()
                .w(px(CELL_WIDTH))
                .flex_none()
                .text_color(rgb(0x9ca3af))
                .child(self.text(format!("typeset · {cut}")))
        };
        let mut table = div().flex().flex_col().gap_1().child(
            self.row()
                .gap_2()
                .child(header(ui.cut_end))
                .child(header(ui.cut_start))
                .child(header(ui.cut_middle)),
        );
        for (row, text) in CELLS.into_iter().enumerate() {
            table = table.child(
                self.row()
                    .items_start()
                    .gap_2()
                    .child(
                        cell(format!("typeset-cell-{row}-end"))
                            .truncate()
                            .child(self.typeset(text)),
                    )
                    .child(
                        cell(format!("typeset-cell-{row}-start"))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis_start()
                            .child(self.typeset(text)),
                    )
                    .child(
                        cell(format!("typeset-cell-{row}-middle"))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis_middle()
                            .child(self.typeset(text)),
                    ),
            );
        }
        let tooltip = div()
            .debug_selector(|| "typeset-tooltip".into())
            .max_w(px(220.0))
            .p_2()
            .rounded_md()
            .bg(rgb(0x0b1220))
            .text_size(px(15.0))
            .child(self.typeset(TOOLTIP));
        let clamp = div()
            .debug_selector(|| "typeset-clamp".into())
            .w(px(220.0))
            .line_clamp(2)
            .text_ellipsis()
            .p_1()
            .bg(rgb(0x1f2937))
            .text_size(px(15.0))
            .child(self.typeset(TOOLTIP));
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(div().text_size(px(18.0)).child(self.text(ui.typeset_title)))
            .child(self.text(ui.typeset_note))
            .child(self.text(ui.chrome_note))
            .child(table)
            .child(
                // Wrap instead of shrinking: typeset() reports a real min-content
                // width and would otherwise narrow first.
                self.row()
                    .flex_wrap()
                    .items_start()
                    .gap_3()
                    .child(tooltip)
                    .child(clamp),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Modifiers, TestAppContext, TestDispatcher, VisualTestContext, point, size};

    #[test]
    fn paragraph_lab_click_drag_visual_keys_and_width_controls() -> Result<()> {
        let mut fonts = parley::FontContext::new();
        for data in [crate::samples::PLEX, FONTS[0], FONTS[1]] {
            fonts
                .collection
                .register_fonts(parley::fontique::Blob::new(Arc::new(data.to_vec())), None);
        }
        let backend = Arc::new(xgpui_parley::ParleyTextSystem::new(fonts, "IBM Plex Sans")?);
        let mut cx =
            TestAppContext::build_with_text_system(TestDispatcher::new(0), None, backend.clone());
        let window = cx.open_window(size(px(580.0), px(1000.0)), |_, cx| {
            ParagraphLab::new(
                vec![
                    Timing {
                        first: 0.0,
                        warm: 0.0,
                        reflow: 0.0
                    };
                    TEXTS.len()
                ],
                backend.clone(),
                cx,
            )
        });
        let lab = window.root(&mut cx).unwrap();
        let mut visual = VisualTestContext::from_window(window.into(), &cx);
        let draw = |visual: &mut VisualTestContext| {
            visual.draw(
                point(px(80.0), px(60.0)),
                size(px(580.0), px(1000.0)),
                |_, _| div().size_full().child(lab.clone()),
            );
        };
        draw(&mut visual);
        let (bounds, layout) = visual.update(|_, cx| lab.read(cx).geometry.clone().unwrap());
        assert_eq!(layout.source(), TEXTS[0].1);
        assert!(layout.runs().len() > 3);
        // Every typeset() cell has the cell width and one line's height.
        let latin = visual.debug_bounds("typeset-cell-0-end").unwrap().size;
        assert_eq!(latin.width, px(CELL_WIDTH));
        for row in 0..CELLS.len() {
            for cut in ["end", "start", "middle"] {
                let selector = format!("typeset-cell-{row}-{cut}").leak();
                assert_eq!(visual.debug_bounds(selector).unwrap().size, latin, "{selector}");
            }
        }
        let line = latin.height;
        let tooltip = visual.debug_bounds("typeset-tooltip").unwrap();
        assert!(tooltip.size.width <= px(220.0));
        assert!(tooltip.size.width > px(200.0), "shown at its maximum width");
        assert!(tooltip.size.height > line * 2.0, "tooltip wraps");
        let clamp = visual.debug_bounds("typeset-clamp").unwrap();
        assert!(clamp.size.height < tooltip.size.height, "clamped to two lines");
        assert!(clamp.size.height > line, "two lines");
        let click = bounds.origin + point(px(1.0), px(12.0));
        visual.simulate_click(click, Modifiers::default());
        visual.simulate_keystrokes("right shift-right");
        draw(&mut visual);
        visual.update(|_, cx| {
            let state = lab.read(cx);
            assert_ne!(state.anchor, state.caret);
            assert!(layout.source().is_char_boundary(state.caret.byte_index));
        });
        let end = layout.caret_bounds(ParagraphPosition {
            byte_index: "office café אבג".len(),
            affinity: TextAffinity::Upstream,
        })?;
        visual.simulate_mouse_down(click, MouseButton::Left, Modifiers::default());
        visual.simulate_mouse_move(
            bounds.origin + end.origin + point(px(0.0), end.size.height / 2.0),
            MouseButton::Left,
            Modifiers::default(),
        );
        visual.simulate_mouse_up(
            bounds.origin + end.origin,
            MouseButton::Left,
            Modifiers::default(),
        );
        visual.update(|_, cx| {
            let state = lab.read(cx);
            assert!(!state.dragging);
            assert!(
                !layout
                    .selection_bounds(state.anchor, state.caret)
                    .unwrap()
                    .is_empty()
            );
        });
        let button = visual.debug_bounds("paragraph-width").unwrap();
        visual.simulate_click(button.center(), Modifiers::default());
        draw(&mut visual);
        visual.update(|_, cx| {
            let state = lab.read(cx);
            assert!(state.wide);
            assert!(state.geometry.as_ref().unwrap().1.size().height < layout.size().height);
        });
        // Forcing RTL moves the Latin start of the first sample to the right side.
        let caret_x = |visual: &mut VisualTestContext| {
            visual.update(|_, cx| {
                let geometry = lab.read(cx).geometry.clone().unwrap().1;
                geometry.caret_bounds(start()).unwrap().origin.x
            })
        };
        let ltr_start = caret_x(&mut visual);
        let button = visual.debug_bounds("paragraph-direction").unwrap();
        visual.simulate_click(button.center(), Modifiers::default());
        draw(&mut visual);
        visual.update(|_, cx| {
            assert_eq!(
                lab.read(cx).options().direction,
                TextDirection::RightToLeft
            );
        });
        assert!(caret_x(&mut visual) > ltr_start + px(100.0));
        let button = visual.debug_bounds("paragraph-language").unwrap();
        visual.simulate_click(button.center(), Modifiers::default());
        draw(&mut visual);
        visual.update(|_, cx| {
            let state = lab.read(cx);
            assert_eq!(state.options().language.unwrap().as_str(), "ar");
            assert!(state.geometry.is_some());
        });
        // The UI button cycles the chrome language; right-to-left UIs mirror rows.
        let x = |visual: &mut VisualTestContext, id: &'static str| {
            visual.debug_bounds(id).unwrap().origin.x
        };
        assert!(x(&mut visual, "paragraph-ui") < x(&mut visual, "paragraph-language"));
        let button = visual.debug_bounds("paragraph-ui").unwrap();
        visual.simulate_click(button.center(), Modifiers::default());
        draw(&mut visual);
        visual.update(|_, cx| assert_eq!(lab.read(cx).strings().tag, "he"));
        assert!(x(&mut visual, "paragraph-ui") > x(&mut visual, "paragraph-language"));
        let first_cell = x(&mut visual, "typeset-cell-0-end");
        assert!(first_cell > x(&mut visual, "typeset-cell-0-middle"), "table mirrors");
        for _ in 1..UI.len() {
            let button = visual.debug_bounds("paragraph-ui").unwrap();
            visual.simulate_click(button.center(), Modifiers::default());
            draw(&mut visual);
        }
        visual.update(|_, cx| assert_eq!(lab.read(cx).strings().tag, "en"));
        assert!(x(&mut visual, "paragraph-ui") < x(&mut visual, "paragraph-language"));

        let button = visual.debug_bounds("paragraph-sample").unwrap();
        visual.simulate_click(button.center(), Modifiers::default());
        draw(&mut visual);
        visual.update(|_, cx| {
            let state = lab.read(cx);
            assert_eq!(state.sample, 1);
            assert_eq!(state.caret, start());
            assert_eq!(state.geometry.as_ref().unwrap().1.source(), TEXTS[1].1);
        });

        // Off-panel parity check: a Latin typeset() cell has the same size as
        // native GPUI text in the same style.
        visual.draw(
            point(px(0.0), px(0.0)),
            size(px(580.0), px(200.0)),
            |_, _| {
                let cell = |id: &'static str| {
                    div()
                        .debug_selector(move || id.into())
                        .w(px(CELL_WIDTH))
                        .px_1()
                        .font_family("IBM Plex Sans")
                        .text_size(px(14.0))
                        .truncate()
                };
                div()
                    .flex()
                    .items_start()
                    .child(cell("native-reference").child(CELLS[0]))
                    .child(cell("typeset-reference").child(typeset(CELLS[0])))
            },
        );
        assert_eq!(
            visual.debug_bounds("typeset-reference").unwrap().size,
            visual.debug_bounds("native-reference").unwrap().size
        );
        Ok(())
    }
}
