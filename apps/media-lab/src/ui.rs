use crate::model::{Lab, Phase};
use gpui::{prelude::*, *};
use xgpui_media::image_view;

const BG: u32 = 0x111619;
const PANEL: u32 = 0x191f23;
const LINE: u32 = 0x30393f;
const TEXT: u32 = 0xe8edeb;
const MUTED: u32 = 0x94a19f;
const MINT: u32 = 0x9ee3bd;
pub(crate) fn button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    active: bool,
) -> Stateful<Div> {
    let label = label.into();
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label.clone())
        .focusable()
        .tab_stop(true)
        .px_3()
        .py_2()
        .rounded_md()
        .border_1()
        .border_color(rgb(if active { MINT } else { LINE }))
        .bg(rgb(if active { MINT } else { PANEL }))
        .text_color(rgb(if active { BG } else { TEXT }))
        .text_sm()
        .cursor_pointer()
        .hover(|s| s.border_color(rgb(MINT)))
        .child(label)
}
fn eyebrow(label: impl Into<SharedString>) -> Div {
    div()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(MUTED))
        .child(label.into())
}
fn metric(label: impl Into<SharedString>, value: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .justify_between()
        .gap_4()
        .py_1()
        .child(div().text_sm().text_color(rgb(MUTED)).child(label.into()))
        .child(div().text_sm().text_color(rgb(TEXT)).child(value.into()))
}
fn mib(bytes: usize) -> String {
    format!("{:.1} MiB", bytes as f32 / (1024. * 1024.))
}
pub(crate) fn checker() -> impl IntoElement {
    canvas(
        |_, _, _| (),
        |bounds, (), window, _| {
            let tile = 24.;
            for y in 0..(f32::from(bounds.size.height) / tile).ceil() as i32 {
                for x in 0..(f32::from(bounds.size.width) / tile).ceil() as i32 {
                    let origin = bounds.origin + point(px(x as f32 * tile), px(y as f32 * tile));
                    let rect = Bounds::new(
                        origin,
                        size(
                            px(tile).min(bounds.right() - origin.x),
                            px(tile).min(bounds.bottom() - origin.y),
                        ),
                    );
                    window.paint_quad(fill(
                        rect,
                        rgb(if (x + y) % 2 == 0 { 0x20282b } else { 0x252e31 }),
                    ));
                }
            }
        },
    )
    .absolute()
    .size_full()
}
impl Lab {
    fn comparison(
        &self,
        media: bool,
        width: f32,
        height: f32,
        zoom: f32,
        cx: &mut Context<Self>,
    ) -> Div {
        let sample = &self.fixtures.samples[self.selected];
        let fit = (width / sample.width as f32).min(height / sample.height as f32);
        let art_w = sample.width as f32 * fit * zoom;
        let art_h = sample.height as f32 * fit * zoom;
        let x = (width - art_w) * 0.5 + f32::from(self.pan.x);
        let y = (height - art_h) * 0.5 + f32::from(self.pan.y);
        let mut viewport = div()
            .id(if media {
                "media-viewport"
            } else {
                "atlas-viewport"
            })
            .relative()
            .w(px(width))
            .h(px(height))
            .overflow_hidden()
            .cursor_crosshair()
            .child(checker())
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                let delta = f32::from(event.delta.pixel_delta(px(24.)).y);
                this.zoom = (this.zoom * (delta * 0.002).clamp(-1., 1.).exp()).clamp(0.1, 4.);
                cx.stop_propagation();
                cx.notify();
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    this.drag = Some((event.position, this.pan));
                    cx.stop_propagation();
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                if event.pressed_button == Some(MouseButton::Left) {
                    if let Some((anchor, pan)) = this.drag {
                        this.pan = pan + (event.position - anchor);
                        cx.notify();
                    }
                } else {
                    this.drag = None;
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.drag = None),
            );
        if media {
            if let Some(image) = &self.primary.image {
                viewport = viewport.child(
                    image_view(image.clone())
                        .filter(self.image_filter())
                        .absolute()
                        .left(px(x))
                        .top(px(y))
                        .w(px(art_w))
                        .h(px(art_h))
                        .rounded(px(self.radius))
                        .opacity(self.opacity),
                );
            } else {
                let label = if self.primary.phase == Phase::Failed {
                    "Source could not be loaded"
                } else {
                    "Preparing image…"
                };
                viewport = viewport.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_sm()
                        .text_color(rgb(MUTED))
                        .child(label),
                );
            }
        } else {
            viewport = viewport.child(
                img(sample.path.clone())
                    .absolute()
                    .left(px(x))
                    .top(px(y))
                    .w(px(art_w))
                    .h(px(art_h))
                    .object_fit(ObjectFit::Fill)
                    .rounded(px(self.radius))
                    .opacity(self.opacity),
            );
        }
        let status = if media {
            self.primary
                .image
                .as_ref()
                .map(|i| format!("{:?}", i.upload_status()))
                .unwrap_or_else(|| self.primary.phase.label().into())
        } else {
            "GPUI img".into()
        };
        div()
            .flex()
            .flex_col()
            .w(px(width + 2.))
            .bg(rgb(PANEL))
            .border_1()
            .border_color(rgb(LINE))
            .rounded_lg()
            .overflow_hidden()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .px_4()
                    .py_3()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(if media { MINT } else { TEXT }))
                                    .child(if media {
                                        "02  /  MIPMAPPED"
                                    } else {
                                        "01  /  ATLAS"
                                    }),
                            )
                            .child(div().text_xs().text_color(rgb(MUTED)).child(if media {
                                "Full mip chain · background staging"
                            } else {
                                "Single texture level · native baseline"
                            })),
                    )
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .bg(rgb(BG))
                            .text_xs()
                            .text_color(rgb(if media { MINT } else { MUTED }))
                            .child(status),
                    ),
            )
            .child(viewport)
            .child(
                div()
                    .flex()
                    .justify_between()
                    .px_4()
                    .py_3()
                    .text_xs()
                    .text_color(rgb(MUTED))
                    .child(if media {
                        "Dedicated source texture"
                    } else {
                        "SpriteAtlas texture"
                    })
                    .child(if media {
                        self.primary
                            .image
                            .as_ref()
                            .map(|i| {
                                format!(
                                    "{} mip levels · {} upload(s)",
                                    i.mip_count(),
                                    i.upload_count()
                                )
                            })
                            .unwrap_or_else(|| "Waiting for decode".into())
                    } else {
                        format!("{} × {} source pixels", sample.width, sample.height)
                    }),
            )
    }
    fn diagnostics(&self) -> Div {
        let usage = self.loader.upload_stats();
        let pool = self.loader.staging_pool_stats();
        div().flex().flex_col().flex_1().p_4().bg(rgb(PANEL)).border_1().border_color(rgb(LINE)).rounded_lg()
            .child(eyebrow("PIPELINE / LIVE"))
            .child(metric("Background staging workers",format!("{} / 2",usage.active_jobs)))
            .child(metric("Staging reservations",mib(usage.staging_bytes)))
            .child(metric("Pool allocated / idle",format!("{} / {}",mib(pool.allocated_bytes),mib(pool.cached_bytes))))
            .child(metric("Buffer allocations / reuse",format!("{} / {}",pool.allocations,pool.reuses)))
            .child(metric("Redraw interval p95",self.p95().map(|v|format!("{v:.1} ms")).unwrap_or_else(||"—".into())))
            .child(div().mt_2().text_xs().text_color(rgb(MUTED)).child("Redraw intervals include UI work; they are not GPU timings. Native atlas memory is excluded."))
    }
    fn stress(&self) -> Div {
        let submitted = self
            .burst
            .iter()
            .filter(|s| matches!(s.phase, Phase::Ready | Phase::Retired))
            .count();
        let mut chips = div().flex().gap_2();
        for (index, slot) in self.burst.iter().enumerate() {
            let color = match slot.phase {
                Phase::Ready | Phase::Retired => MINT,
                Phase::Failed => 0xf49b8d,
                Phase::Staging => 0x9bbaf2,
                _ => MUTED,
            };
            chips = chips.child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .w(px(48.))
                    .h(px(48.))
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(LINE))
                    .bg(rgb(BG))
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(color))
                            .child(format!("{:02}", index + 1)),
                    )
                    .child(
                        div()
                            .text_size(px(8.))
                            .text_color(rgb(color))
                            .child(slot.phase.label()),
                    ),
            );
        }
        let mut previews = div().flex().gap_3().h(px(72.));
        for (index, slot) in self.burst.iter().enumerate() {
            if let Some(image) = &slot.image {
                let w = 72. * image.width() as f32 / image.height() as f32;
                previews = previews.child(
                    div()
                        .relative()
                        .w(px(w))
                        .h(px(72.))
                        .overflow_hidden()
                        .child(checker())
                        .child(
                            image_view(image.clone())
                                .w(px(w))
                                .h(px(72.))
                                .rounded(px(6.)),
                        )
                        .child(
                            div()
                                .absolute()
                                .bottom(px(3.))
                                .left(px(3.))
                                .px_1()
                                .rounded_sm()
                                .bg(rgb(BG))
                                .text_xs()
                                .text_color(rgb(TEXT))
                                .child(format!("{:02}", index + 1)),
                        ),
                );
            }
        }
        let message = if self.burst.is_empty() {
            "Start a burst to watch 12 fresh snapshots pass through bounded loading and uploads."
                .into()
        } else if !self.admission.is_empty() {
            format!(
                "Backpressure: {}. Completed previews retire to release capacity.",
                self.admission
            )
        } else {
            "READY = host-submitted. DONE = a submitted preview retired to make room.".into()
        };
        div()
            .flex()
            .flex_col()
            .gap_3()
            .w(px(750.))
            .p_4()
            .bg(rgb(PANEL))
            .border_1()
            .border_color(rgb(LINE))
            .rounded_lg()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .child(eyebrow("UPLOAD RUN / 12 SNAPSHOTS"))
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(MINT))
                            .child(format!("{submitted} / {} submitted", self.burst.len())),
                    ),
            )
            .child(chips)
            .child(previews)
            .child(div().text_xs().text_color(rgb(MUTED)).child(message))
    }
}
impl Render for Lab {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.video_mode {
            return crate::video_lab::render(self, window, cx);
        }
        if self.path_mode {
            return crate::paths::render(self, window, cx);
        }
        if let Some(mut run) = self.ui_perf.take()
            && run.tick(self)
        {
            self.ui_perf = Some(run);
        }
        let pending = self.poll();
        let sweep = self.sweep && !cx.reduce_motion();
        self.continuous_redraw = pending || sweep || self.ui_perf.is_some();
        if self.continuous_redraw {
            window.request_animation_frame();
        }
        let zoom = self.zoom
            * if sweep {
                1. + 0.10 * (self.started.elapsed().as_secs_f32() * 0.9).sin()
            } else {
                1.
            };
        let viewport = window.viewport_size();
        let width = ((f32::from(viewport.width) - 48. - 20. - 4.) * 0.5).max(280.);
        let height = (f32::from(viewport.height) * 0.28).clamp(230., 280.);
        let source = &self.fixtures.samples[self.selected];
        let mut samples = div().flex().gap_2();
        for (index, sample) in self.fixtures.samples.iter().enumerate() {
            samples = samples.child(
                button(
                    ("sample", index),
                    sample.name.clone(),
                    index == self.selected,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.select(index);
                    cx.notify();
                })),
            );
        }
        let header = div()
            .flex()
            .justify_between()
            .items_center()
            .child(
                div()
                    .flex()
                    .gap_3()
                    .items_center()
                    .child(
                        div()
                            .w(px(44.))
                            .h(px(44.))
                            .rounded_lg()
                            .bg(rgb(MINT))
                            .text_color(rgb(BG))
                            .text_xl()
                            .font_weight(FontWeight::BOLD)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child("m"),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(26.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Media lab"),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(MUTED))
                                    .child("Source-sized textures & bounded uploads"),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .items_center()
                    .child(div().w(px(6.)).h(px(6.)).rounded_full().bg(rgb(MINT)))
                    .child(eyebrow(if cfg!(target_os = "macos") {
                        "METAL / NATIVE"
                    } else if cfg!(target_os = "windows") {
                        "D3D11 / NATIVE"
                    } else {
                        "WGPU / NATIVE"
                    })),
            );
        let toolbar = div()
            .flex()
            .justify_between()
            .items_center()
            .gap_3()
            .flex_wrap()
            .child(samples)
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        button("reload", "Reload media", false).on_click(cx.listener(
                            |this, _, _, cx| {
                                this.reload();
                                cx.notify();
                            },
                        )),
                    )
                    .child(button("burst", "Burst ×12", true).on_click(cx.listener(
                        |this, _, _, cx| {
                            this.burst();
                            cx.notify();
                        },
                    ))),
            );
        let controls = div()
            .flex()
            .justify_between()
            .items_center()
            .gap_3()
            .flex_wrap()
            .child(
                div()
                    .flex()
                    .gap_2()
                    .items_center()
                    .child(button("zoom-out", "−", false).on_click(cx.listener(
                        |this, _, _, cx| {
                            this.zoom = (this.zoom / 1.25).max(0.1);
                            cx.notify();
                        },
                    )))
                    .child(
                        div()
                            .w(px(86.))
                            .text_sm()
                            .text_center()
                            .text_color(rgb(TEXT))
                            .child(format!("Fit × {zoom:.2}")),
                    )
                    .child(
                        button("zoom-in", "+", false).on_click(cx.listener(|this, _, _, cx| {
                            this.zoom = (this.zoom * 1.25).min(4.);
                            cx.notify();
                        })),
                    )
                    .child(
                        button("reset-view", "Reset view", false).on_click(cx.listener(
                            |this, _, _, cx| {
                                this.reset_view();
                                cx.notify();
                            },
                        )),
                    )
                    .child(
                        button(
                            "sweep",
                            if self.sweep {
                                "Stop sweep"
                            } else {
                                "Sweep zoom"
                            },
                            self.sweep,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.sweep = !this.sweep;
                            cx.notify();
                        })),
                    )
                    .child(
                        button(
                            "opacity",
                            format!("Opacity {:.0}%", self.opacity * 100.),
                            false,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.opacity = if this.opacity > 0.9 {
                                0.65
                            } else if this.opacity > 0.5 {
                                0.3
                            } else {
                                1.
                            };
                            cx.notify();
                        })),
                    )
                    .child(
                        button("corners", format!("Corners {:.0}", self.radius), false).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.radius = if this.radius == 0. {
                                    16.
                                } else if this.radius == 16. {
                                    40.
                                } else {
                                    0.
                                };
                                cx.notify();
                            }),
                        ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        button(
                            "count-budget",
                            format!("{} / frame", self.count_limit),
                            false,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            let count = match this.count_limit {
                                1 => 2,
                                2 => 4,
                                _ => 1,
                            };
                            this.policy(count, this.byte_limit);
                            cx.notify();
                        })),
                    )
                    .child(
                        button(
                            "byte-budget",
                            format!("{} / frame", mib(self.byte_limit)),
                            false,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            let bytes = match this.byte_limit {
                                16777216 => 33554432,
                                33554432 => 67108864,
                                _ => 16777216,
                            };
                            this.policy(this.count_limit, bytes);
                            cx.notify();
                        })),
                    ),
            );
        let mut filter_controls = div().flex().items_center().gap_2().child(
            div()
                .text_xs()
                .text_color(rgb(MUTED))
                .child("MEDIA FILTERS"),
        );
        let labels = [
            ["Brightness 1.0", "Brightness 0.6", "Brightness 1.4"],
            ["Contrast 1.0", "Contrast 0.6", "Contrast 1.5"],
            ["Saturation 1.0", "Grayscale", "Saturation 1.8"],
            ["Hue 0°", "Hue 90°", "Hue 180°"],
            ["Tint off", "Warm tint", "Cool tint"],
        ];
        for (i, choices) in labels.iter().enumerate() {
            filter_controls = filter_controls.child(
                button(
                    format!("filter-{i}"),
                    choices[self.filters[i]],
                    self.filters[i] != 0,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.filters[i] = (this.filters[i] + 1) % 3;
                    cx.notify();
                })),
            );
        }
        filter_controls =
            filter_controls.child(button("reset-filters", "Reset filters", false).on_click(
                cx.listener(|this, _, _, cx| {
                    this.filters = [0; 5];
                    cx.notify();
                }),
            ));
        let mut root=div().id("media-lab").size_full().overflow_y_scroll().bg(rgb(BG)).text_color(rgb(TEXT)).font_family(if cfg!(target_os="windows") {"Segoe UI"}else{"Helvetica"})
            .p_6().flex().flex_col().gap_4().child(header).child(div().flex().gap_2().child(button("open-paths", "Path effects →", false).on_click(cx.listener(|this,_,_,cx| { this.path_mode=true; cx.notify(); }))).child(button("open-video", "Video surfaces →", false).on_click(cx.listener(|this,_,_,cx| { this.video_mode=true; cx.notify(); })))).child(toolbar).child(controls).child(filter_controls)
            .child(div().text_sm().text_color(rgb(MUTED)).child(source.description))
            .child(div().flex().gap_5().child(self.comparison(false,width,height,zoom,cx)).child(self.comparison(true,width,height,zoom,cx)))
            .child(div().flex().justify_between().text_xs().text_color(rgb(MUTED))
                .child("Scroll to zoom · Drag either view to pan · Zoom and pan stay in sync")
                .child("Mips use linear-light filtering; judge detail stability, not identical brightness."))
            .child(div().flex().gap_5().child(self.stress()).child(self.diagnostics()));
        if let Some(error) = &self.primary.error {
            root = root.child(
                div()
                    .text_sm()
                    .text_color(rgb(0xf49b8d))
                    .child(error.clone()),
            );
        }
        root.child(div().text_xs().text_color(rgb(MUTED)).child(if cx.reduce_motion(){"Reduced motion is enabled; zoom sweep is paused."}else{"Generated offline samples · SDR sRGB · Native atlas is the baseline · No GPU-time or total-memory claim"}))
    }
}
