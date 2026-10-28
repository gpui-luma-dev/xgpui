//! Shared native/browser acceptance demo for issue #28's static and motion milestones.
use gpui::{prelude::*, *};
use web_time::Instant;
use xgpui_shader::typography::{KineticEffect, SelectableText, TextShadow};
use xgpui_shader::{Brush, BrushStop};

#[derive(Clone, Copy, PartialEq)]
enum MotionMode {
    Static,
    Typewriter,
    Scramble,
    Wave,
}

const SAMPLES: [(&str, &str, &str); 4] = [
    (
        "Editorial",
        "Good typography keeps its meaning.",
        "A shaped line, an authentic source, and a selection you can trust.",
    ),
    (
        "Ligatures",
        "office · affine · efficient · café",
        "Select inside fi / ffi: caret positions interpolate within the shaped ligature.",
    ),
    (
        "Unicode",
        "Cafe\u{301} · naïve · 👩🏽‍💻 · 🇺🇸",
        "Combining accents, skin tones, joined emoji and flags stay together when selecting.",
    ),
    (
        "Overflow",
        "The whole sentence remains selectable, even when the viewport is deliberately narrow.",
        "Drag beyond the edge, or use Shift + End. The caret scrolls into view.",
    ),
];
pub struct TypographyDemo {
    samples: Vec<Entity<SelectableText>>,
    active: usize,
    narrow: bool,
    large: bool,
    light: bool,
    status: SharedString,
    _subscriptions: Vec<Subscription>,
    motion: MotionMode,
    playing: bool,
    progress: f32,
    phase: f32,
    reduced: bool,
    last_tick: Instant,
    seed: u64,
    fill: usize,
    shadow: usize,
    softness: usize,
}
impl TypographyDemo {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let samples: Vec<_> = SAMPLES
            .iter()
            .map(|(_, text, _)| {
                cx.new(|cx| SelectableText::new(*text, cx).expect("LTR demo sample"))
            })
            .collect();
        let subscriptions = samples
            .iter()
            .map(|sample| cx.observe(sample, |_, _, cx| cx.notify()))
            .collect();
        let mut demo = Self {
            samples,
            active: 0,
            narrow: false,
            large: false,
            light: false,
            status: "Select a phrase to inspect its authentic source.".into(),
            _subscriptions: subscriptions,
            motion: MotionMode::Static,
            playing: false,
            progress: 0.45,
            phase: 0.,
            reduced: false,
            last_tick: Instant::now(),
            seed: 42,
            fill: 1,
            shadow: 1,
            softness: 1,
        };
        demo.apply_style(cx);
        demo
    }
    fn apply_style(&mut self, cx: &mut Context<Self>) {
        let fill = match self.fill {
            1 => Some(
                Brush::linear(
                    0.,
                    vec![
                        BrushStop::new(
                            0.,
                            if self.light {
                                [0.02, 0.15, 0.6, 1.]
                            } else {
                                [0.25, 0.65, 1., 1.]
                            },
                        ),
                        BrushStop::new(
                            1.,
                            if self.light {
                                [0.0, 0.45, 0.25, 1.]
                            } else {
                                [0.5, 1., 0.8, 1.]
                            },
                        ),
                    ],
                )
                .unwrap(),
            ),
            2 => Some(
                Brush::linear(
                    0.,
                    vec![
                        BrushStop::new(
                            0.,
                            if self.light {
                                [0.65, 0.02, 0.25, 1.]
                            } else {
                                [1., 0.35, 0.6, 1.]
                            },
                        ),
                        BrushStop::new(
                            1.,
                            if self.light {
                                [0.5, 0.2, 0., 1.]
                            } else {
                                [1., 0.85, 0.35, 1.]
                            },
                        ),
                    ],
                )
                .unwrap(),
            ),
            _ => None,
        };
        let radius = px([4., 8., 12.][self.softness]);
        let shadow = match self.shadow {
            1 => Some(
                TextShadow::glow(
                    rgba(if self.light { 0x286de580 } else { 0x56dac8e0 }).into(),
                    radius,
                )
                .unwrap(),
            ),
            2 => Some(
                TextShadow::new(rgba(0x000000b0).into(), point(px(3.), px(5.)), radius).unwrap(),
            ),
            _ => None,
        };
        for sample in &self.samples {
            sample.update(cx, |text, cx| {
                text.set_fill(fill.clone(), cx).unwrap();
                text.set_shadow(shadow.clone(), cx);
            });
        }
        cx.notify();
    }
    fn choose(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.active = index;
        self.status = "Sample changed. Selection is retained independently for each sample.".into();
        window.focus(&self.samples[index].focus_handle(cx), cx);
        cx.notify();
    }
    fn select_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.samples[self.active].update(cx, |text, cx| text.select_all(cx));
        window.focus(&self.samples[self.active].focus_handle(cx), cx);
    }
    fn copy(&mut self, cx: &mut Context<Self>) {
        let copied = self.samples[self.active].update(cx, |text, cx| text.copy_selection(cx));
        self.status = if copied {
            "Copy requested for the selected source. Paste into another app to verify."
        } else {
            "Select some text first; the clipboard was left unchanged."
        }
        .into();
        cx.notify();
    }
}
fn button(id: impl Into<ElementId>, label: impl Into<SharedString>, active: bool) -> Stateful<Div> {
    let label = label.into();
    div()
        .id(id)
        .aria_label(label.clone())
        .px_3()
        .py_2()
        .rounded_md()
        .border_1()
        .border_color(if active { rgb(0x36b8a7) } else { rgb(0x53647c) })
        .bg(if active { rgb(0x154d4b) } else { rgb(0x253348) })
        .text_color(rgb(0xe8f3f5))
        .text_size(px(13.))
        .cursor_pointer()
        .role(Role::Button)
        .tab_index(0)
        .focus(|style| style.border_color(rgb(0x8ef5db)))
        .child(label)
}
impl Render for TypographyDemo {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let reduced = self.reduced || cx.reduce_motion();
        let now = Instant::now();
        if self.playing && !reduced && self.motion != MotionMode::Static {
            let dt = now.duration_since(self.last_tick).as_secs_f32().min(0.1);
            if matches!(self.motion, MotionMode::Typewriter | MotionMode::Scramble) {
                if self.motion == MotionMode::Scramble {
                    self.phase += dt * 12.;
                }
                self.progress = (self.progress + dt / 4.).min(1.);
                if self.progress >= 1. {
                    self.playing = false;
                }
            } else {
                self.phase = (self.phase + dt * 2.2).rem_euclid(std::f32::consts::TAU);
            }
            if self.playing {
                window.request_animation_frame();
            }
        }
        self.last_tick = now;
        let effect = if reduced {
            KineticEffect::Static
        } else {
            match self.motion {
                MotionMode::Static => KineticEffect::Static,
                MotionMode::Typewriter => KineticEffect::Typewriter {
                    progress: self.progress,
                },
                MotionMode::Scramble => KineticEffect::Scramble {
                    progress: self.progress,
                    seed: self.seed,
                    tick: self.phase.floor() as u64,
                },
                MotionMode::Wave => KineticEffect::Wave {
                    amplitude: px(8.),
                    phase: self.phase,
                    cluster_phase: 0.65,
                },
            }
        };
        self.samples[self.active].update(cx, |text, cx| text.set_effect(effect, cx).unwrap());
        let text = self.samples[self.active].read(cx);
        let range = text.selection();
        let selected: SharedString = if range.is_empty() {
            "No selection yet".into()
        } else {
            text.selected_text().to_owned().into()
        };
        let count = text.selected_text().chars().count();
        let background = if self.light { 0xf1f3ed } else { 0x0c1422 };
        let surface = if self.light { 0xffffff } else { 0x131f30 };
        let foreground = if self.light { 0x142238 } else { 0xeaf1f6 };
        let muted = if self.light { 0x50617a } else { 0x9fb0c5 };
        div().id("typography-demo").size_full().overflow_y_scroll()
            .bg(rgb(background)).text_color(rgb(foreground)).font_family("IBM Plex Sans")
            .child(div().max_w(px(1040.)).mx_auto().p_8().flex().flex_col().gap_6()
                .child(div().flex().justify_between().items_center()
                    .child(div().text_size(px(12.)).text_color(rgb(0x36b8a7)).child("TYPE / 04     •     LIGHT, COLOUR & MEANING"))
                    .child(div().text_size(px(12.)).text_color(rgb(muted)).child("xgpui-shader / issue #28")))
                .child(div().flex().flex_col().gap_2()
                    .child(div().text_size(px(42.)).line_height(px(50.)).child("Bright ideas. Real words."))
                    .child(div().text_size(px(16.)).line_height(px(25.)).text_color(rgb(muted))
                        .child("Only the painted glyphs move or disappear. Select across the original line, then copy: the source stays complete.")))
                .child(div().flex().flex_wrap().gap_2().children(SAMPLES.iter().enumerate().map(|(index, (label, _, _))| {
                    button(("sample", index), *label, self.active == index)
                        .on_click(cx.listener(move |this, _, window, cx| this.choose(index, window, cx)))

                })))
                .child(div().flex().flex_col().gap_3()
                    .child(div().flex().flex_wrap().gap_2().children([
                        (MotionMode::Static, "Static"), (MotionMode::Typewriter, "Typewriter"), (MotionMode::Scramble, "Scramble"), (MotionMode::Wave, "Wave")
                    ].into_iter().map(|(mode, label)| button(label, label, self.motion == mode)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.motion = mode; this.playing = mode == MotionMode::Wave;
                            this.last_tick = Instant::now(); cx.notify();
                        })))))
                    .child(div().flex().flex_wrap().gap_2()
                        .child(button("motion-play", if self.playing { "Pause" } else { "Play" }, self.playing)
                            .on_click(cx.listener(|this, _, _, cx| {
                                if this.motion == MotionMode::Static { this.motion = MotionMode::Wave; }
                                if this.progress >= 1. && matches!(this.motion, MotionMode::Typewriter | MotionMode::Scramble) { this.progress = 0.; }
                                this.playing = !this.playing; this.last_tick = Instant::now(); cx.notify();
                            })))
                        .child(button("restart", "Restart", false).on_click(cx.listener(|this, _, _, cx| {
                            this.progress = 0.; this.phase = 0.; this.last_tick = Instant::now(); cx.notify();
                        })))
                        .child(button("motion-step-back", "−10%", false).on_click(cx.listener(|this, _, _, cx| {
                            if this.motion != MotionMode::Scramble { this.motion = MotionMode::Typewriter; } this.playing = false; this.progress = (this.progress - 0.1).max(0.); cx.notify();
                        })))
                        .child(button("motion-step-next", "+10%", false).on_click(cx.listener(|this, _, _, cx| {
                            if this.motion != MotionMode::Scramble { this.motion = MotionMode::Typewriter; } this.playing = false; this.progress = (this.progress + 0.1).min(1.); cx.notify();
                        })))
                        .when(self.motion == MotionMode::Scramble, |el| el
                            .child(button("scramble-seed", "New seed", false).on_click(cx.listener(|this, _, _, cx| {
                                this.seed = this.seed.wrapping_add(1); this.playing = false; cx.notify();
                            })))
                            .child(button("scramble-frame", "Next frame", false).on_click(cx.listener(|this, _, _, cx| {
                                this.phase += 1.; this.playing = false; cx.notify();
                            }))))
                        .child(button("reduce-motion", if reduced { "Reduced motion: on" } else { "Reduced motion: off" }, reduced)
                            .on_click(cx.listener(|this, _, _, cx| { this.reduced = !this.reduced; cx.notify(); }))))
                    .child(div().text_size(px(12.)).text_color(rgb(muted)).child(if reduced {
                        "Reduced motion shows the complete static source and stops animation requests.".to_owned()
                    } else if self.motion == MotionMode::Scramble {
                        format!("Decoded: {:.0}% · seed {} / frame {} · spaces and emoji stay original. Pause or step to inspect.", self.progress * 100., self.seed, self.phase.floor() as u64)
                    } else if self.motion == MotionMode::Typewriter {
                        format!("Reveal: {:.0}% · complete shaped clusters appear together. Selection can include unrevealed text.", self.progress * 100.)
                    } else {
                        if self.motion == MotionMode::Static { "Static reference · select the unchanged source.".to_owned() } else { "Wave: 8 px amplitude · selection follows the original baseline. Pause to inspect a frame.".to_owned() }
                    })))
                .child(div().flex().flex_col().gap_2()
                    .child(div().flex().flex_wrap().gap_2()
                        .child(button("text-fill", ["Fill: native", "Fill: cool gradient", "Fill: warm gradient"][self.fill], self.fill != 0)
                            .on_click(cx.listener(|this, _, _, cx| { this.fill = (this.fill + 1) % 3; this.apply_style(cx); })))
                        .child(button("text-shadow", ["Shadow: off", "Shadow: glow", "Shadow: drop"][self.shadow], self.shadow != 0)
                            .on_click(cx.listener(|this, _, _, cx| { this.shadow = (this.shadow + 1) % 3; this.apply_style(cx); })))
                        .child(button("text-softness", format!("Softness: {} px", [4,8,12][self.softness]), false)
                            .on_click(cx.listener(|this, _, _, cx| { this.softness = (this.softness + 1) % 3; this.apply_style(cx); }))))
                    .child(div().text_size(px(12.)).text_color(rgb(muted)).child("Gradient fill and a glyph-only halo. Selection stays sharp; emoji keep their original colors.")))
                .child(div().p_6().rounded_xl().bg(rgb(surface)).border_1().border_color(rgb(0x42546b)).flex().flex_col().gap_4()
                    .child(div().flex().justify_between().flex_wrap().gap_2().text_size(px(12.)).text_color(rgb(muted))
                        .child("VISUAL LAYER  /  CANONICAL SELECTION")
                        .child(if self.narrow { "320 px viewport • horizontal overflow" } else { "Fluid viewport • resize the window" }))
                    .child(div().w(if self.narrow { Length::from(px(320.)) } else { Length::from(relative(1.)) }).max_w_full()
                        .text_size(px(if self.large { 36. } else { 27. })).line_height(px(if self.large { 52. } else { 42. }))
                        .child(self.samples[self.active].clone()))
                    .child(div().text_size(px(13.)).line_height(px(21.)).text_color(rgb(muted)).child(SAMPLES[self.active].2))
                    .child(div().flex().flex_wrap().gap_2()
                        .child(button("select-all", "Select all", false)
                            .on_click(cx.listener(|this, _, window, cx| this.select_all(window, cx)))
                            )
                        .child(button("copy-source", "Copy selection ↗", false)
                            .on_click(cx.listener(|this, _, _, cx| this.copy(cx)))
                            )
                        .child(button("width", if self.narrow { "Width: narrow" } else { "Width: fluid" }, self.narrow)
                            .on_click(cx.listener(|this, _, _, cx| { this.narrow = !this.narrow; cx.notify(); }))
                            )
                        .child(button("type-size", if self.large { "Type: 36 px" } else { "Type: 27 px" }, self.large)
                            .on_click(cx.listener(|this, _, _, cx| { this.large = !this.large; cx.notify(); }))
                            )
                        .child(button("theme", if self.light { "Theme: light" } else { "Theme: ink" }, self.light)
                            .on_click(cx.listener(|this, _, _, cx| { this.light = !this.light; this.apply_style(cx); }))
                            ))
                    .child(div().id("copy-status").role(Role::Status).text_size(px(12.)).text_color(rgb(muted)).child(self.status.clone())))
                .child(div().flex().flex_col().gap_3().p_5().rounded_lg().bg(rgb(surface))
                    .child(div().flex().justify_between().text_size(px(12.)).text_color(rgb(muted))
                        .child("LOGICAL LAYER  /  ORIGINAL SOURCE")
                        .child(format!("UTF-8 {}..{}  /  {} Unicode scalars", range.start, range.end, count)))
                    .child(div().text_size(px(15.)).line_height(px(24.)).text_color(rgb(muted)).child(SAMPLES[self.active].1))
                    .child(div().text_size(px(20.)).line_height(px(30.)).child(selected))
                    .child(div().text_size(px(12.)).text_color(rgb(muted)).child("The inspector and clipboard read the original string, even where glyphs are hidden or displaced.")))
                .child(div().flex().flex_col().gap_2().text_size(px(13.)).line_height(px(22.)).text_color(rgb(muted))
                    .child("TRY IT  /  Tab to focus · Shift + arrows to select · Home / End · Cmd or Ctrl + A / C · double-click a word · triple-click the line")
                    .child(if cfg!(target_family = "wasm") { "ACCESSIBILITY  /  The web platform has no canvas accessibility bridge yet. Use the native demo to inspect read-only text and selection semantics." } else { "ACCESSIBILITY  /  Read-only text field with the full source, a text run and selection endpoints. Motion does not change the accessible source or selection." })
                    .child("SCOPE  /  Single-line LTR text. Wrapping, bidirectional selection and rich text editing remain outside this milestone.")))
    }
}
