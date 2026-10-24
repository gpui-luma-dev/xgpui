//! Editable paragraph probe; uses GPUI caches, Parley geometry and the glyph atlas.
#[path = "paragraph/input.rs"]
mod input;

use anyhow::{Result, ensure};
use gpui::{
    App, Bounds, Context, ElementInputHandler, FocusHandle, Focusable, Font, FontFallbacks,
    FontRun, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, ParagraphPosition, Pixels,
    PlatformParagraphLayout, Point, Render, TextAffinity, TextSystem, VisualDirection, Window,
    WindowTextSystem, canvas, div, fill, paint_paragraph, prelude::*, px, rgb,
};
#[cfg(not(target_family = "wasm"))]
use std::time::Instant;
use std::{hint::black_box, sync::Arc};
#[cfg(target_family = "wasm")]
use web_time::Instant;

pub const FONTS: [&[u8]; 2] = [
    include_bytes!(
        "../../../extensions/xgpui-ext/crates/xgpui-parley/tests/fonts/NotoSansArabic.ttf"
    ),
    include_bytes!(
        "../../../extensions/xgpui-ext/crates/xgpui-parley/tests/fonts/NotoSansHebrew.ttf"
    ),
];
const TEXTS: [(&str, &str); 3] = [
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
];

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

pub struct ParagraphLab {
    sample: usize,
    content: String,
    marked: Option<std::ops::Range<usize>>,
    preferred_x: Option<Pixels>,
    reveal_caret: bool,
    wide: bool,
    timings: Vec<Timing>,
    focus: FocusHandle,
    anchor: ParagraphPosition,
    caret: ParagraphPosition,
    dragging: bool,
    geometry: Option<(Bounds<Pixels>, Arc<dyn PlatformParagraphLayout>)>,
}
impl ParagraphLab {
    pub fn new(timings: Vec<Timing>, cx: &mut Context<Self>) -> Self {
        Self {
            sample: 0,
            content: TEXTS[0].1.to_owned(),
            marked: None,
            preferred_x: None,
            reveal_caret: false,
            wide: false,
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
    fn layout(&self, window: &Window) -> Result<Arc<dyn PlatformParagraphLayout>> {
        window.text_system().layout_paragraph(
            self.content.clone().into(),
            px(24.0),
            &runs(&self.content, window.text_system()),
            self.width(window),
        )
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
        let label = TEXTS[self.sample].0;
        let text = self.content.clone();
        let width = self.width(window);
        let layout = self.layout(window);
        let mut root = div().id("paragraph-lab").size_full().overflow_y_scroll()
            .bg(rgb(0x111827)).text_color(rgb(0xeff4fc)).font_family("IBM Plex Sans")
            .text_size(px(13.0)).p_4().flex().flex_col().gap_3().track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key))
            .child(div().text_size(px(24.0)).child("Parley paragraphs"))
            .child("Click/drag to select. Left/Right move visually; Shift extends selection.")
            .child("Type or use IME; Enter adds a line break. Clipboard shortcuts and grapheme deletion.")
            .child(div().flex().gap_3()
                .child(div().id("paragraph-sample").debug_selector(|| "paragraph-sample".into()).p_2().bg(rgb(0x374151)).cursor_pointer().child("Next sample (reset)")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.sample = (this.sample + 1) % TEXTS.len(); this.content = TEXTS[this.sample].1.to_owned(); this.marked = None; this.preferred_x = None; this.reveal_caret = false; this.anchor = start(); this.caret = start(); this.geometry = None; this.dragging = false; cx.notify();
                    })))
                .child(div().id("paragraph-width").debug_selector(|| "paragraph-width".into()).p_2().bg(rgb(0x374151)).cursor_pointer().child(if self.wide { "Narrow" } else { "Wide" })
                    .on_click(cx.listener(|this, _, _, cx| { this.wide = !this.wide; this.geometry = None; this.dragging = false; cx.notify(); }))))
            .child(format!("{label} | width {:.0}px", f32::from(width)))
            .child(format!("Seed: {}", self.timings[self.sample].label()))
            .child("CPU: first layout / warm cache hit / new-width reflow median. Shared backend state.")
            .child(format!("Anchor {} {:?} | caret {} {:?}", self.anchor.byte_index, self.anchor.affinity, self.caret.byte_index, self.caret.affinity));
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
        root
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
        let mut cx = TestAppContext::build_with_text_system(TestDispatcher::new(0), None, backend);
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
        let button = visual.debug_bounds("paragraph-sample").unwrap();
        visual.simulate_click(button.center(), Modifiers::default());
        draw(&mut visual);
        visual.update(|_, cx| {
            let state = lab.read(cx);
            assert_eq!(state.sample, 1);
            assert_eq!(state.caret, start());
            assert_eq!(state.geometry.as_ref().unwrap().1.source(), TEXTS[1].1);
        });
        Ok(())
    }
}
