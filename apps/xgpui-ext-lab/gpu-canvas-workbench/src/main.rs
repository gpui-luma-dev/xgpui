//! GPU Canvas Workbench (#197): an editable GPU workload in an engine on its
//! own wgpu device, presented through `xgpui-direct-gpu` beside ordinary
//! controls.
//!
//! - The engine keeps a persistent working image. Pointer strokes append
//!   dabs; a compute pass edits only the touched region.
//! - Each edit revision is published as an immutable RGBA16F linear-P3
//!   snapshot into an engine surface slot. Frames with no edits publish
//!   nothing, and the published snapshot is never modified.
//! - The canvas and the thumbnail place the same published revision. A wide
//!   window shows it in wide gamut, any other window as an sRGB preview.
//! - Right-click inspects a 4×4 region asynchronously; the values carry the
//!   revision they belong to.
//!
//! Modes: interactive (default); `--demo` paints scripted strokes;
//! `--run-seconds N` prints a summary and exits; `--check` replays an edit
//! script against a CPU reference without a window.

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn main() {
    eprintln!("gpu-canvas-workbench requires macOS, Linux or Windows; see issue #197");
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
fn main() -> anyhow::Result<()> {
    workbench::main()
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
mod check;
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
mod engine;
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
mod oracle;

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
mod workbench {
    use crate::check::percentile;
    use crate::engine::{BACKEND, Engine, Inspection};
    use crate::oracle::{Dab, HEIGHT, WIDTH, published};
    use anyhow::{Result, ensure};
    use clap::Parser;
    use gpui::{prelude::*, *};
    use std::{cell::Cell, rc::Rc, time::Instant};
    use xgpui_ext_lab::{LabApp, LabWindowTitle};

    #[cfg(target_os = "macos")]
    const TRANSPORT: &str = "zero-copy IOSurface ring";
    #[cfg(target_os = "linux")]
    const TRANSPORT: &str = "zero-copy opaque-fd ring";
    #[cfg(target_os = "windows")]
    const TRANSPORT: &str = "zero-copy shared-handle ring";
    const RADIUS: f32 = 8.;

    #[derive(Parser)]
    #[command(about = "Editable GPU canvas in an independent wgpu engine, presented into xgpui")]
    struct Options {
        /// Replay an edit script against the CPU reference, without a window.
        #[arg(long)]
        check: bool,
        /// Paint scripted strokes continuously.
        #[arg(long)]
        demo: bool,
        /// Use a standard window (RGBA16F slots are shown as an sRGB preview).
        #[arg(long)]
        sdr: bool,
        /// Show the window for this many seconds, print a summary, exit.
        #[arg(long)]
        run_seconds: Option<f32>,
        /// Ring length (2..=8).
        #[arg(long, default_value_t = 3)]
        slots: usize,
    }

    /// Brush colours, linear Display P3, with an approximate sRGB swatch.
    const COLORS: [(&str, [f32; 3], u32); 6] = [
        ("P3 red", [1., 0., 0.], 0xff2a1a),
        ("P3 green", [0., 1., 0.], 0x2aff3a),
        ("P3 blue", [0., 0., 1.], 0x2a40ff),
        ("orange", [0.8, 0.3, 0.05], 0xe89540),
        ("near-black", [0.004, 0.004, 0.004], 0x0e0e0e),
        ("white", [1., 1., 1.], 0xffffff),
    ];
    const RADII: [f32; 3] = [4., 10., 24.];
    const OPACITIES: [f32; 3] = [0.25, 0.6, 1.];
    const EXPOSURES: [f32; 5] = [-1., -0.5, 0., 0.5, 1.];

    #[derive(Clone, Copy, PartialEq)]
    enum Presentation {
        Auto,
        Preview,
    }

    struct Workbench {
        engine: Engine,
        color: usize,
        radius: f32,
        opacity: f32,
        presentation: Presentation,
        wide: bool,
        notice: Option<String>,
        canvas_bounds: Rc<Cell<Bounds<Pixels>>>,
        last_point: Option<Point<f32>>,
        inspection: Option<Inspection>,
        inspect_rejected: bool,
        /// Snapshots held back on purpose (pressure test): each pins a slot.
        held: Vec<std::sync::Arc<dyn CustomContent>>,
        demo: bool,
        demo_step: u32,
        error: Option<String>,
        started: Instant,
        run_seconds: Option<f32>,
        frame_times: Vec<f32>,
        last_frame: Option<Instant>,
    }

    impl Workbench {
        fn slot_point(&self, position: Point<Pixels>) -> Option<Point<f32>> {
            let bounds = self.canvas_bounds.get();
            if bounds.size.width <= px(0.) {
                return None;
            }
            let x = f32::from(position.x - bounds.origin.x) * WIDTH as f32
                / f32::from(bounds.size.width);
            let y = f32::from(position.y - bounds.origin.y) * HEIGHT as f32
                / f32::from(bounds.size.height);
            Some(point(x, y))
        }

        fn dab_at(&mut self, at: Point<f32>) {
            self.engine.dab(Dab {
                x: at.x,
                y: at.y,
                radius: self.radius,
                opacity: self.opacity,
                color: COLORS[self.color].1,
            });
        }

        /// Dabs every third of a radius from the last point to `to`.
        fn stroke_to(&mut self, to: Point<f32>) {
            let Some(from) = self.last_point.replace(to) else {
                self.dab_at(to);
                return;
            };
            let (dx, dy) = (to.x - from.x, to.y - from.y);
            let steps = ((dx * dx + dy * dy).sqrt() / (self.radius / 3.).max(1.)).ceil() as u32;
            for i in 1..=steps.max(1) {
                let t = i as f32 / steps.max(1) as f32;
                self.dab_at(point(from.x + dx * t, from.y + dy * t));
            }
        }

        fn demo_frame(&mut self) {
            let t = self.demo_step as f32 * 0.05;
            self.demo_step += 1;
            let (color, radius, opacity) = (self.color, self.radius, self.opacity);
            self.color = (self.demo_step / 120) as usize % COLORS.len();
            self.radius = RADII[(self.demo_step / 120) as usize % RADII.len()];
            self.opacity = 0.6;
            let to = point(
                WIDTH as f32 * (0.5 + 0.42 * (t * 1.3).sin()),
                HEIGHT as f32 * (0.5 + 0.4 * (t * 2.1).cos()),
            );
            if self.demo_step % 120 == 0 {
                self.last_point = None;
            }
            self.stroke_to(to);
            if self.demo_step % 30 == 0 {
                self.inspect_rejected = !self.engine.inspect(to.x as u32, to.y as u32);
            }
            (self.color, self.radius, self.opacity) = (color, radius, opacity);
        }

        fn placement(&self, width: f32, height: f32, track: bool) -> impl IntoElement {
            let surface = self.engine.surface().clone();
            let bounds_cell = self.canvas_bounds.clone();
            let preview = self.presentation == Presentation::Preview;
            div()
                .w(px(width))
                .h(px(height))
                .rounded(px(RADIUS))
                .border_1()
                .border_color(rgb(0x3a3a3a))
                .child(
                    canvas(
                        move |bounds, _, _| {
                            if track {
                                bounds_cell.set(bounds);
                            }
                        },
                        move |bounds, _, window, _| {
                            let corners = Corners::all(px(RADIUS - 1.));
                            if preview {
                                if let Some(content) = surface.preview_content() {
                                    window.paint_custom(
                                        CustomGeometry::RoundedRect {
                                            bounds,
                                            corner_radii: corners,
                                        },
                                        content,
                                    );
                                }
                            } else {
                                surface.paint(window, bounds, corners);
                            }
                        },
                    )
                    .size_full(),
                )
        }

        fn summary(&self) -> String {
            let stats = self.engine.stats();
            let surface = self.engine.surface().stats();
            format!(
                "{:.1}s shown: {} window frames, frame interval ms p50 {:.2} p95 {:.2} p99 {:.2} max {:.2}; revision {} published {}, {} snapshots ({} blocked), {} dabs in {} paint passes ({} tiles), edit-to-publish ms p50 {:.2} p95 {:.2}, {} submissions, {:.1} MiB GPU publish, {:.1} KiB uploaded, {} B read back, {} GPU objects, ring full {}; {} window, {} presentation",
                self.started.elapsed().as_secs_f32(),
                self.frame_times.len() + 1,
                percentile(&self.frame_times, 0.5),
                percentile(&self.frame_times, 0.95),
                percentile(&self.frame_times, 0.99),
                percentile(&self.frame_times, 1.),
                stats.revision,
                stats.published_revision,
                stats.publishes,
                stats.publish_blocked,
                stats.dabs_applied,
                stats.paint_passes,
                stats.tiles_touched,
                percentile(&self.engine.latencies_ms, 0.5),
                percentile(&self.engine.latencies_ms, 0.95),
                stats.submissions,
                stats.gpu_bytes_published as f64 / (1024. * 1024.),
                stats.cpu_bytes_uploaded as f64 / 1024.,
                stats.cpu_bytes_read,
                stats.resources_created,
                surface.would_block,
                if self.wide { "wide" } else { "standard" },
                self.presentation_label(),
            )
        }

        fn presentation_label(&self) -> &'static str {
            match (self.presentation, self.wide) {
                (Presentation::Auto, true) => "wide-gamut",
                (Presentation::Auto, false) => "sRGB preview (window is not wide)",
                (Presentation::Preview, _) => "sRGB preview (forced)",
            }
        }

        fn controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
            let row = || div().flex().flex_wrap().gap_1().items_center();
            let label = |text: &'static str| div().w(px(70.)).text_color(rgb(0x9a9a9a)).child(text);
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    row()
                        .child(label("Colour"))
                        .children(COLORS.iter().enumerate().map(|(i, (_name, _, swatch))| {
                            div()
                                .id(("color", i))
                                .size(px(24.))
                                .rounded_sm()
                                .bg(rgb(*swatch))
                                .border_2()
                                .border_color(if i == self.color {
                                    rgb(0xffffff)
                                } else {
                                    rgb(0x3a3a3a)
                                })
                                .cursor_pointer()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.color = i;
                                    cx.notify();
                                }))
                        })),
                )
                .child(
                    row()
                        .child(label("Radius"))
                        .children(RADII.iter().map(|&r| {
                            button(
                                ("radius", r as usize),
                                format!("{r:.0}px"),
                                self.radius == r,
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.radius = r;
                                    cx.notify();
                                },
                            ))
                        })),
                )
                .child(
                    row()
                        .child(label("Opacity"))
                        .children(OPACITIES.iter().map(|&o| {
                            button(
                                ("opacity", (o * 100.) as usize),
                                format!("{:.0}%", o * 100.),
                                self.opacity == o,
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.opacity = o;
                                    cx.notify();
                                },
                            ))
                        })),
                )
                .child(
                    row()
                        .child(label("Exposure"))
                        .children(EXPOSURES.iter().enumerate().map(|(i, &stops)| {
                            button(
                                ("exposure", i),
                                format!("{stops:+.1}"),
                                self.engine.exposure() == stops,
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.engine.set_exposure(stops);
                                    cx.notify();
                                },
                            ))
                        })),
                )
                .child(
                    row()
                        .child(label("Show"))
                        .child(
                            button(
                                "present-auto",
                                "Auto",
                                self.presentation == Presentation::Auto,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.presentation = Presentation::Auto;
                                cx.notify();
                            })),
                        )
                        .child(
                            button(
                                "present-preview",
                                "sRGB preview",
                                self.presentation == Presentation::Preview,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.presentation = Presentation::Preview;
                                cx.notify();
                            })),
                        ),
                )
                .child(
                    row()
                        .child(label(""))
                        .child(
                            button("demo", "Demo strokes", self.demo).on_click(cx.listener(
                                |this, _, _, cx| {
                                    this.demo = !this.demo;
                                    this.last_point = None;
                                    cx.notify();
                                },
                            )),
                        )
                        .child(button("reset", "Reset", false).on_click(cx.listener(
                            |this, _, _, cx| {
                                this.engine.reset();
                                this.inspection = None;
                                cx.notify();
                            },
                        ))),
                )
                .child(
                    row()
                        .child(label("Pressure"))
                        .child(
                            button(
                                "hold",
                                format!(
                                    "Hold snapshot ({}/{})",
                                    self.held.len(),
                                    self.engine.surface().desc().slots
                                ),
                                false,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                // Like a retained old scene: the held snapshot
                                // keeps its slot until released.
                                if this.held.len() < this.engine.surface().desc().slots
                                    && let Some(content) = this.engine.surface().content()
                                {
                                    this.held.push(content);
                                }
                                cx.notify();
                            })),
                        )
                        .child(button("release", "Release", false).on_click(cx.listener(
                            |this, _, _, cx| {
                                this.held.clear();
                                cx.notify();
                            },
                        ))),
                )
        }

        fn inspection_view(&self) -> impl IntoElement {
            let stats = self.engine.stats();
            let text = match &self.inspection {
                None => "Right-click the canvas to inspect a 4×4 region.".to_string(),
                Some(found) => {
                    let p = found.pixels[0];
                    let out = published([p[0], p[1], p[2]], self.engine.exposure());
                    format!(
                        "({}, {}) at revision {}{}\nworking  linear P3 {:.4} {:.4} {:.4}  α {:.2}\npublished (exposure {:+.1}) {:.4} {:.4} {:.4}",
                        found.x,
                        found.y,
                        found.revision,
                        if found.revision == stats.revision {
                            " (current)".to_string()
                        } else {
                            format!(" (now {})", stats.revision)
                        },
                        p[0],
                        p[1],
                        p[2],
                        p[3],
                        self.engine.exposure(),
                        out[0],
                        out[1],
                        out[2],
                    )
                }
            };
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().text_color(rgb(0x9a9a9a)).child("Inspect"))
                .children(text.lines().map(|line| div().child(line.to_string())))
                .children(self.inspect_rejected.then(|| {
                    div()
                        .text_color(rgb(0xffcc66))
                        .child("inspection busy; retry")
                }))
        }

        fn status_view(&self, window: &Window) -> impl IntoElement {
            let stats = self.engine.stats();
            let surface = self.engine.surface().stats();
            let desc = self.engine.surface().desc();
            let color = window.color_status();
            let lines = [
                format!(
                    "Window: requested {:?}, effective {:?}; showing {}",
                    color.requested(),
                    color.effective(),
                    self.presentation_label()
                ),
                format!(
                    "Engine: own {} device, {}; {}×{} {:?} slots, storage writes {}",
                    BACKEND.1,
                    if self.engine.gpu_sync() {
                        "GPU-side wait"
                    } else {
                        "engine waits on CPU"
                    },
                    desc.width,
                    desc.height,
                    desc.format,
                    if desc.format.supports_storage() {
                        "yes"
                    } else {
                        "no"
                    },
                ),
                format!(
                    "Transport: {TRANSPORT}; {:?}; no CPU copy of frames",
                    self.engine.surface().transport()
                ),
                format!(
                    "Revision {} · published {} · pending dabs {} · inspections in flight {}",
                    stats.revision,
                    stats.published_revision,
                    self.engine.pending_dabs(),
                    self.engine.inspections_in_flight()
                ),
                format!(
                    "Ring {} slots: free {} / writing {} / published {} · held {} · waiting {} · ring full {} · publish deferred {}",
                    desc.slots,
                    surface.free,
                    surface.writing,
                    surface.published,
                    self.held.len(),
                    surface.waiting,
                    surface.would_block,
                    stats.publish_blocked
                ),
                format!(
                    "Work: {} dabs in {} paint passes ({} tiles touched) · {} snapshots · {} submissions",
                    stats.dabs_applied,
                    stats.paint_passes,
                    stats.tiles_touched,
                    stats.publishes,
                    stats.submissions
                ),
                format!(
                    "Bytes: GPU publish {:.1} MiB · uploaded {:.1} KiB · read back {} B · GPU objects {}",
                    stats.gpu_bytes_published as f64 / (1024. * 1024.),
                    stats.cpu_bytes_uploaded as f64 / 1024.,
                    stats.cpu_bytes_read,
                    stats.resources_created
                ),
                format!(
                    "Edit to publish: p50 {:.2} ms · p95 {:.2} ms",
                    percentile(&self.engine.latencies_ms, 0.5),
                    percentile(&self.engine.latencies_ms, 0.95)
                ),
            ];
            div()
                .flex()
                .flex_col()
                .gap_1()
                .text_xs()
                .text_color(rgb(0xb0b0b0))
                .children(lines.into_iter().map(|line| div().child(line)))
        }
    }

    fn button(
        id: impl Into<ElementId>,
        text: impl Into<SharedString>,
        active: bool,
    ) -> Stateful<Div> {
        div()
            .id(id)
            .px_2()
            .py_1()
            .rounded_md()
            .bg(if active { rgb(0x4a6fa5) } else { rgb(0x343a45) })
            .text_color(white())
            .cursor_pointer()
            .child(text.into())
    }

    impl Render for Workbench {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let now = Instant::now();
            if let Some(last) = self.last_frame.replace(now) {
                self.frame_times
                    .push(now.duration_since(last).as_secs_f32() * 1000.);
            }
            if let Some(limit) = self.run_seconds
                && self.started.elapsed().as_secs_f32() >= limit
            {
                println!("{}", self.summary());
                if let Some(error) = &self.error {
                    println!("engine error: {error}");
                }
                cx.quit();
            }
            if self.demo {
                self.demo_frame();
            }
            if let Err(error) = self.engine.frame() {
                self.error = Some(format!("{error:#}"));
            }
            if let Some(found) = self.engine.poll_inspections().pop() {
                self.inspection = Some(found);
            }
            let stats = self.engine.stats();
            if self.demo
                || self.run_seconds.is_some()
                || self.engine.pending_dabs() > 0
                || self.engine.inspections_in_flight() > 0
                || stats.revision != stats.published_revision
            {
                window.request_animation_frame();
            }
            div()
                .size_full()
                .bg(rgb(0x1b1b1b))
                .text_color(rgb(0xe8e8e8))
                .text_sm()
                .p_4()
                .flex()
                .flex_col()
                .gap_3()
                .child(div().text_lg().child("GPU Canvas Workbench"))
                .children(
                    self.notice
                        .clone()
                        .map(|notice| div().text_color(rgb(0xffcc66)).child(notice)),
                )
                .children(self.error.clone().map(|error| {
                    div()
                        .text_color(rgb(0xff6666))
                        .child(format!("engine error: {error}"))
                }))
                .child(
                    div()
                        .flex()
                        .gap_4()
                        .items_start()
                        .child(
                            div()
                                .id("canvas")
                                .cursor_crosshair()
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, event: &MouseDownEvent, _, cx| {
                                        this.last_point = None;
                                        if let Some(at) = this.slot_point(event.position) {
                                            this.stroke_to(at);
                                        }
                                        cx.notify();
                                    }),
                                )
                                .on_mouse_move(cx.listener(
                                    |this, event: &MouseMoveEvent, _, cx| {
                                        if event.pressed_button == Some(MouseButton::Left)
                                            && this.last_point.is_some()
                                            && let Some(at) = this.slot_point(event.position)
                                        {
                                            this.stroke_to(at);
                                            cx.notify();
                                        }
                                    },
                                ))
                                .on_mouse_up(
                                    MouseButton::Left,
                                    cx.listener(|this, _: &MouseUpEvent, _, _| {
                                        this.last_point = None;
                                    }),
                                )
                                .on_mouse_down(
                                    MouseButton::Right,
                                    cx.listener(|this, event: &MouseDownEvent, _, cx| {
                                        if let Some(at) = this.slot_point(event.position) {
                                            this.inspect_rejected = !this
                                                .engine
                                                .inspect(at.x.max(0.) as u32, at.y.max(0.) as u32);
                                        }
                                        cx.notify();
                                    }),
                                )
                                .child(self.placement(WIDTH as f32, HEIGHT as f32, true)),
                        )
                        .child(
                            div()
                                .w(px(330.))
                                .flex()
                                .flex_col()
                                .gap_3()
                                .child(
                                    div()
                                        .text_color(rgb(0x9a9a9a))
                                        .child("Same revision, second placement"),
                                )
                                .child(self.placement(240., 150., false))
                                .child(self.controls(cx))
                                .child(self.inspection_view()),
                        ),
                )
                .child(self.status_view(window))
        }
    }

    pub fn main() -> Result<()> {
        let options = Options::parse();
        ensure!((2..=8).contains(&options.slots), "slots must be 2..=8");
        if options.check {
            return crate::check::run(options.slots);
        }
        gpui_platform::application().run(move |cx: &mut App| {
            let bounds = Bounds::centered(None, size(px(1040.), px(800.)), cx);
            cx.open_lab_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                |window, cx| {
                    let notice = if options.sdr {
                        None
                    } else {
                        xgpui_ext_lab::negotiate_wide(window)
                    };
                    let wide = window.color_status().effective() == WindowColorMode::WideGamutSdr;
                    let engine = Engine::new(options.slots).expect("workbench engine");
                    window.set_lab_title("XGPUI GPU Canvas Workbench");
                    window.activate_window();
                    cx.new(|_| Workbench {
                        engine,
                        color: 0,
                        radius: RADII[1],
                        opacity: OPACITIES[1],
                        presentation: Presentation::Auto,
                        wide,
                        notice,
                        canvas_bounds: Rc::new(Cell::new(Bounds::default())),
                        last_point: None,
                        inspection: None,
                        inspect_rejected: false,
                        held: Vec::new(),
                        demo: options.demo,
                        demo_step: 0,
                        error: None,
                        started: Instant::now(),
                        run_seconds: options.run_seconds,
                        frame_times: Vec::new(),
                        last_frame: None,
                    })
                },
            )
            .unwrap();
            cx.activate(true);
        });
        Ok(())
    }
}
