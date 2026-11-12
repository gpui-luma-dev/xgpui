//! Interop Boids: wgpu's `boids` example running as an independent GPU engine
//! inside an xgpui window, through `xgpui-gpu-interop`.
//!
//! The example is ported almost unchanged (`src/boids.rs`, the WGSL files).
//! It keeps its own wgpu device and thread, a persistent pair of particle
//! buffers, and a compute pass feeding a render pass every step: work a
//! GPUI shader paint cannot do. Each step renders into a slot of an
//! `EngineSurface`; the view paints the newest slot and redraws only when the
//! engine presents.
//!
//! `--run-seconds N` prints a summary and exits; `--pause-at` / `--play-at`
//! script the Pause/Play button for unattended runs.

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn main() {
    eprintln!("interop-boids requires macOS, Linux or Windows");
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
fn main() -> anyhow::Result<()> {
    lab::main()
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
mod boids;
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
mod engine;

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
mod lab {
    use crate::boids::DEFAULT_PARTICLES;
    use crate::engine::{Engine, HEIGHT, Status, WIDTH};
    use anyhow::Result;
    use clap::Parser;
    use futures::StreamExt as _;
    use gpui::{prelude::*, *};
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use std::time::{Duration, Instant};
    use xgpui_ext_lab::{LabApp, LabWindowTitle};

    #[cfg(target_os = "macos")]
    const TRANSPORT: &str = "IOSurface";
    #[cfg(target_os = "linux")]
    const TRANSPORT: &str = "opaque-fd Vulkan memory";
    #[cfg(target_os = "windows")]
    const TRANSPORT: &str = "shared D3D12/D3D11 textures";

    /// Particle-count choices; wgpu's default is 1,500. The compute pass is
    /// O(n²): 100,000 boids is about 50 ms of GPU time per step on an M4 Max.
    const PARTICLE_CHOICES: [u32; 5] = [500, DEFAULT_PARTICLES, 15_000, 50_000, 100_000];
    /// Speed slider limits, as multiples of wgpu's speed. The slider is
    /// logarithmic, so 1× sits in the middle.
    const SPEED_MIN: f32 = 0.25;
    const SPEED_MAX: f32 = 4.;
    const TRACK_WIDTH: f32 = 160.;
    /// Timings start after the window's first frames.
    const WARM_UP: Duration = Duration::from_millis(500);
    /// UI frame intervals kept for the live readout.
    const LIVE_FRAMES: usize = 120;

    #[derive(Parser)]
    #[command(about = "wgpu's boids example as an independent engine, presented into xgpui")]
    struct Options {
        /// Show the window for this many seconds, print a summary, exit.
        #[arg(long)]
        run_seconds: Option<f32>,
        /// Number of boids to start with.
        #[arg(long, default_value_t = DEFAULT_PARTICLES)]
        particles: u32,
        /// Press Pause after this many seconds.
        #[arg(long)]
        pause_at: Option<f32>,
        /// Press Play after this many seconds.
        #[arg(long)]
        play_at: Option<f32>,
        /// Resize the window continuously (scripted, for measuring UI
        /// responsiveness while the engine animates).
        #[arg(long)]
        resize_test: bool,
        /// Open the window on this display (an index from the list printed
        /// at startup).
        #[arg(long)]
        display: Option<usize>,
        /// Start with the engine on the UI thread instead of its own thread.
        #[arg(long)]
        ui_thread: bool,
        /// Switch the engine between its own thread and the UI thread at
        /// each of these times, in seconds (comma-separated).
        #[arg(long, value_delimiter = ',')]
        switch_at: Vec<f32>,
        /// Start the speed slider at this multiple of wgpu's speed (0.25 to 4).
        #[arg(long, default_value_t = 1.)]
        speed: f32,
    }

    struct BoidsView {
        engine: Engine,
        status: Status,
        paused: bool,
        started: Instant,
        /// Intervals between UI frames while the engine runs, in ms.
        frame_times: Vec<f32>,
        /// UI-thread time per frame, from render to paint, in ms (includes
        /// the engine step when it runs on the UI thread).
        ui_work: Rc<RefCell<Vec<f32>>>,
        last_frame: Option<Instant>,
        renders: u64,
        /// Evidence for the pause: UI frames and engine steps while paused.
        paused_at: Option<u64>,
        renders_while_paused: u64,
        steps_while_paused: u64,
        pauses: u32,
        /// Smoothness: UI frames (while running) that showed no new engine
        /// frame, or skipped one, and the engine's present count at the
        /// last UI frame.
        repeated: u64,
        skipped: u64,
        last_presents: Option<u64>,
        speed: f32,
        /// The window's display refresh interval, as last sent to the engine.
        refresh: Option<Duration>,
        dragging_speed: bool,
        /// Where the speed track was laid out, for mapping the pointer.
        speed_track: Rc<Cell<Bounds<Pixels>>>,
    }

    impl BoidsView {
        fn new(engine: Engine, cx: &mut Context<Self>) -> Self {
            // Redraw when the engine presents (on the engine thread), not
            // every display frame.
            let (tx, mut rx) = futures::channel::mpsc::unbounded::<()>();
            engine.surface().on_present(move || {
                tx.unbounded_send(()).ok();
            });
            cx.spawn(async move |this, cx| {
                while rx.next().await.is_some() {
                    if this.update(cx, |_, cx| cx.notify()).is_err() {
                        break;
                    }
                }
            })
            .detach();
            let status = engine.status();
            Self {
                engine,
                paused: status.paused,
                status,
                started: Instant::now(),
                frame_times: Vec::new(),
                ui_work: Rc::default(),
                last_frame: None,
                renders: 0,
                paused_at: None,
                renders_while_paused: 0,
                steps_while_paused: 0,
                pauses: 0,
                repeated: 0,
                skipped: 0,
                last_presents: None,
                speed: 1.,
                refresh: None,
                dragging_speed: false,
                speed_track: Rc::new(Cell::new(Bounds::default())),
            }
        }

        fn set_speed(&mut self, speed: f32, cx: &mut Context<Self>) {
            // Two decimals, snapping to 1× near the middle.
            let speed = (speed.clamp(SPEED_MIN, SPEED_MAX) * 100.).round() / 100.;
            let speed = if (speed - 1.).abs() < 0.04 { 1. } else { speed };
            if speed != self.speed {
                self.speed = speed;
                self.engine.set_speed(speed);
                cx.notify();
            }
        }

        /// Set the speed from a pointer x position over the track.
        fn scrub_speed(&mut self, x: Pixels, cx: &mut Context<Self>) {
            let track = self.speed_track.get();
            if track.size.width > px(0.) {
                let fraction = ((x - track.origin.x) / track.size.width).clamp(0., 1.);
                self.set_speed(speed_at(fraction), cx);
            }
        }

        fn speed_slider(&self, cx: &mut Context<Self>) -> impl IntoElement {
            let fraction = speed_fraction(self.speed);
            let track = self.speed_track.clone();
            const KNOB: f32 = 14.;
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(div().ml_4().text_color(rgb(0x9a9a9a)).child("Speed"))
                .child(
                    div()
                        .id("speed")
                        .relative()
                        .w(px(TRACK_WIDTH + KNOB))
                        .h(px(24.))
                        .cursor_pointer()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, event: &MouseDownEvent, _, cx| {
                                this.dragging_speed = true;
                                this.scrub_speed(event.position.x, cx);
                                cx.stop_propagation();
                            }),
                        )
                        // The usable track, inset by half a knob on each side.
                        .child(
                            canvas(
                                move |mut bounds, _, _| {
                                    bounds.origin.x += px(KNOB / 2.);
                                    bounds.size.width -= px(KNOB);
                                    track.set(bounds);
                                },
                                |_, _, _, _| (),
                            )
                            .absolute()
                            .top_0()
                            .left_0()
                            .size_full(),
                        )
                        .child(
                            div()
                                .absolute()
                                .left(px(KNOB / 2.))
                                .top(px(10.))
                                .w(px(TRACK_WIDTH))
                                .h(px(4.))
                                .rounded_full()
                                .bg(rgb(0x4a5465)),
                        )
                        .child(
                            div()
                                .absolute()
                                .left(px(KNOB / 2.))
                                .top(px(10.))
                                .w(px(TRACK_WIDTH * fraction))
                                .h(px(4.))
                                .rounded_full()
                                .bg(rgb(0x82bfff)),
                        )
                        // A tick at 1×.
                        .child(
                            div()
                                .absolute()
                                .left(px(KNOB / 2. + TRACK_WIDTH * 0.5 - 1.))
                                .top(px(6.))
                                .w(px(2.))
                                .h(px(12.))
                                .bg(rgb(0x6a7485)),
                        )
                        .child(
                            div()
                                .absolute()
                                .left(px(TRACK_WIDTH * fraction))
                                .top(px(5.))
                                .size(px(KNOB))
                                .rounded_full()
                                .bg(white()),
                        ),
                )
                // Fixed-width readouts, so dragging never shifts the row.
                .child(
                    div()
                        .w(px(44.))
                        .flex()
                        .justify_end()
                        .font_features(tabular_numbers())
                        .child(format!("{:.2}×", self.speed)),
                )
                .child(
                    button("speed-reset", "1×", self.speed == 1.)
                        .w(px(36.))
                        .on_click(cx.listener(|this, _, _, cx| this.set_speed(1., cx))),
                )
        }

        fn set_paused(&mut self, paused: bool, cx: &mut Context<Self>) {
            if paused == self.paused {
                return;
            }
            self.paused = paused;
            self.engine.set_paused(paused);
            // The gap between the last frame before a pause and the first
            // after it is not a UI frame time.
            self.last_frame = None;
            let steps = self.engine.status().steps;
            if paused {
                self.pauses += 1;
                // Count steps from 100 ms after Pause, once a step that was
                // already in flight has landed.
                cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(100))
                        .await;
                    let _ = this.update(cx, |view, _| {
                        if view.paused {
                            view.paused_at = Some(view.engine.status().steps);
                        }
                    });
                })
                .detach();
            } else if let Some(at) = self.paused_at.take() {
                self.steps_while_paused += steps.saturating_sub(at);
            }
            println!(
                "{} at {:.1}s after {steps} steps",
                if paused { "paused" } else { "playing" },
                self.started.elapsed().as_secs_f32()
            );
            cx.notify();
        }

        fn set_threaded(&mut self, threaded: bool, cx: &mut Context<Self>) {
            if let Err(error) = self.engine.set_threaded(threaded) {
                self.status.error = Some(format!("{error:#}"));
            }
            // Timings from one mode don't describe the other.
            self.frame_times.clear();
            self.ui_work.borrow_mut().clear();
            self.last_frame = None;
            println!(
                "engine on {} at {:.1}s after {} steps",
                if threaded {
                    "its own thread"
                } else {
                    "the UI thread"
                },
                self.started.elapsed().as_secs_f32(),
                self.engine.status().steps
            );
            cx.notify();
        }

        fn set_particles(&mut self, particles: u32, cx: &mut Context<Self>) {
            self.engine.set_particles(particles);
            self.status.particles = particles;
            cx.notify();
        }

        fn summary(&self) -> String {
            let status = self.engine.status();
            let surface = self.engine.surface().stats();
            let seconds = self.started.elapsed().as_secs_f32();
            let ui_work = self.ui_work.borrow();
            format!(
                "{seconds:.1}s: engine on {}, {} boids on {} ({}), {} engine steps ({:.1}/s, {:.0}/s in the last second), record+present {:.2} ms, GPU {:.2} ms, {}; \
                 {} UI frames, UI work ms p50 {:.2} p95 {:.2} max {:.2}, frame interval ms p50 {:.2} p95 {:.2} max {:.2}, {} hitches (> 1.5× p50), {} repeated, {} skipped; display {:.2} ms, refresh {:.2} ms, {} show timeouts; \
                 {} presents, {} composited, {} unshown, composite latency {:.2} ms, ring full {}; \
                 {} pauses, {} UI frames and {} steps while paused{}",
                if status.threaded {
                    "its own thread"
                } else {
                    "the UI thread"
                },
                status.particles,
                status.adapter,
                status.format,
                status.steps,
                status.steps as f32 / seconds,
                status.steps_per_second,
                status.record_ms,
                status.gpu_ms,
                if status.observe_only {
                    "observe-only"
                } else {
                    "GPU wait"
                },
                self.renders,
                percentile(&ui_work, 0.5),
                percentile(&ui_work, 0.95),
                percentile(&ui_work, 1.),
                percentile(&self.frame_times, 0.5),
                percentile(&self.frame_times, 0.95),
                percentile(&self.frame_times, 1.),
                hitches(&self.frame_times),
                self.repeated,
                self.skipped,
                status.display_ms,
                status.refresh_ms,
                status.show_timeouts,
                surface.presents,
                surface.composited,
                surface.unshown,
                surface.composite_latency_ms,
                status.ring_full,
                self.pauses,
                self.renders_while_paused,
                self.steps_while_paused,
                status
                    .error
                    .map(|error| format!("; engine error: {error}"))
                    .unwrap_or_default(),
            )
        }

        /// Live stats as fixed-width cells: labels never change, and each
        /// value is padded and right-aligned in a box of fixed width with
        /// tabular digits, so changing numbers never shift the layout.
        fn stats_view(&self) -> impl IntoElement {
            let ui_work = self.ui_work.borrow();
            let live = &ui_work[ui_work.len().saturating_sub(LIVE_FRAMES)..];
            let running = !self.paused && !live.is_empty();
            let value = |shown: bool, text: String| if shown { text } else { "—".to_owned() };
            div()
                .flex_none()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_x_4()
                        .gap_y_1()
                        .child(stat(
                            "Engine",
                            86.,
                            if self.paused {
                                "paused".to_owned()
                            } else if self.status.threaded {
                                "own thread".to_owned()
                            } else {
                                "UI thread".to_owned()
                            },
                        ))
                        .child(stat(
                            "Steps/s",
                            40.,
                            value(!self.paused, format!("{:.0}", self.status.steps_per_second)),
                        ))
                        .child(stat("Steps", 80., self.status.steps.to_string()))
                        .child(stat(
                            "Engine step CPU",
                            64.,
                            value(!self.paused, format!("{:.2} ms", self.status.record_ms)),
                        ))
                        .child(stat(
                            "GPU",
                            64.,
                            value(
                                !self.paused && self.status.threaded && self.status.gpu_ms > 0.,
                                format!("{:.1} ms", self.status.gpu_ms),
                            ),
                        ))
                        .child(stat(
                            "UI work per frame p50",
                            64.,
                            value(running, format!("{:.1} ms", percentile(live, 0.5))),
                        ))
                        .child(stat(
                            "p95",
                            64.,
                            value(running, format!("{:.1} ms", percentile(live, 0.95))),
                        )),
                )
                .child(line(
                    rgb(0x8a8a8a),
                    if self.paused {
                        "Paused: the engine does no GPU work and the UI does not redraw.".to_owned()
                    } else {
                        format!(
                            "{} · {WIDTH}×{HEIGHT} {} slots over {TRANSPORT}, zero copies · {}",
                            self.status.adapter,
                            self.status.format,
                            if self.status.observe_only {
                                "observe-only presentation (long steps)"
                            } else {
                                "presented with a GPU wait"
                            }
                        )
                    },
                ))
                // Always present, empty without an error, so an error never
                // resizes the canvas.
                .child(line(
                    rgb(0xff7a6a),
                    self.status.error.clone().unwrap_or_default(),
                ))
        }

        fn controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .child(
                    button(
                        "pause",
                        if self.paused { "▶ Play" } else { "⏸ Pause" },
                        true,
                    )
                    .w(px(90.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        let paused = !this.paused;
                        this.set_paused(paused, cx);
                    })),
                )
                .child(div().ml_4().text_color(rgb(0x9a9a9a)).child("Engine on"))
                .child(
                    button("own-thread", "Own thread", self.status.threaded)
                        .on_click(cx.listener(|this, _, _, cx| this.set_threaded(true, cx))),
                )
                .child(
                    button("ui-thread", "UI thread", !self.status.threaded)
                        .on_click(cx.listener(|this, _, _, cx| this.set_threaded(false, cx))),
                )
                .child(div().ml_4().text_color(rgb(0x9a9a9a)).child("Boids"))
                .children(PARTICLE_CHOICES.iter().map(|&count| {
                    button(
                        ("particles", count as usize),
                        group_thousands(count),
                        self.status.particles == count,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_particles(count, cx);
                    }))
                }))
                .child(self.speed_slider(cx))
        }
    }

    impl Render for BoidsView {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let now = Instant::now();
            // Tell the engine the display's refresh rate (and when the window
            // moves to a display with another one).
            let refresh = window.refresh_interval(cx);
            if refresh != self.refresh {
                self.refresh = refresh;
                if let Some(refresh) = refresh {
                    self.engine.set_refresh_interval(refresh);
                }
            }
            // On the UI thread the engine steps here, inside the UI's frame,
            // and keeps the UI animating.
            if self.engine.step_inline() {
                window.request_animation_frame();
            }
            self.renders += 1;
            // Leave the window's first frames out of the timings.
            let warm = self.started.elapsed() >= WARM_UP;
            if !warm {
                self.last_frame = None;
            }
            if self.paused {
                self.renders_while_paused += 1;
            } else if let Some(last) = self.last_frame.replace(now) {
                self.frame_times
                    .push(now.duration_since(last).as_secs_f32() * 1000.);
            }
            // Each UI frame while running should show exactly one new step.
            let presents = self.engine.surface().stats().presents;
            if let Some(last) = self.last_presents.replace(presents)
                && !self.paused
                && warm
            {
                match presents - last {
                    0 => self.repeated += 1,
                    1 => {}
                    _ => self.skipped += 1,
                }
            }
            self.status = self.engine.status();
            let surface = self.engine.surface().clone();
            let ui_work = warm.then(|| self.ui_work.clone());
            div()
                .size_full()
                .bg(rgb(0x1b1b1b))
                .text_color(rgb(0xe8e8e8))
                .text_sm()
                .p_4()
                .flex()
                .flex_col()
                .gap_3()
                // Keep dragging the speed knob anywhere in the window.
                .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                    if this.dragging_speed {
                        if event.pressed_button == Some(MouseButton::Left) {
                            this.scrub_speed(event.position.x, cx);
                        } else {
                            this.dragging_speed = false;
                        }
                    }
                }))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseUpEvent, _, _| this.dragging_speed = false),
                )
                .child(div().text_color(rgb(0xb0b0b0)).child(
                    "wgpu's boids example on its own wgpu device. Every step is a compute pass \
                         and a render pass into an xgpui-gpu-interop slot; this window shows the \
                         newest one. Try 100,000 boids on its own thread, then on the UI thread, \
                         and drag the speed slider or resize the window.",
                ))
                .child(self.controls(cx))
                .child(
                    div().flex_1().min_h_0().child(
                        canvas(
                            |_, _, _| (),
                            move |bounds, _, window, _| {
                                surface.paint(window, letterbox(bounds), Corners::all(px(8.)));
                                if let Some(ui_work) = ui_work {
                                    ui_work
                                        .borrow_mut()
                                        .push(now.elapsed().as_secs_f32() * 1000.);
                                }
                            },
                        )
                        .size_full(),
                    ),
                )
                .child(self.stats_view())
        }
    }

    /// A fixed label and a value right-aligned in a box `width` wide.
    fn stat(label: &'static str, width: f32, value: String) -> Div {
        div()
            .flex()
            .gap_2()
            .whitespace_nowrap()
            .child(div().text_color(rgb(0x9a9a9a)).child(label))
            .child(
                div()
                    .w(px(width))
                    .flex()
                    .justify_end()
                    .font_features(tabular_numbers())
                    .child(value),
            )
    }

    /// One line of fixed height, truncated rather than wrapped.
    fn line(color: Rgba, text: String) -> Div {
        div()
            .h(px(18.))
            .w_full()
            .truncate()
            .text_color(color)
            .child(text)
    }

    fn tabular_numbers() -> FontFeatures {
        FontFeatures(std::sync::Arc::new(vec![("tnum".into(), 1)]))
    }

    /// UI frame intervals longer than 1.5× the median: a display frame that
    /// showed no new engine frame.
    fn hitches(intervals: &[f32]) -> usize {
        let limit = percentile(intervals, 0.5) * 1.5;
        intervals.iter().filter(|&&ms| ms > limit).count()
    }

    /// Slider position (0..=1) of a speed: logarithmic, 1× in the middle.
    fn speed_fraction(speed: f32) -> f32 {
        (speed / SPEED_MIN).log2() / (SPEED_MAX / SPEED_MIN).log2()
    }
    fn speed_at(fraction: f32) -> f32 {
        SPEED_MIN * (SPEED_MAX / SPEED_MIN).powf(fraction)
    }

    /// The largest 4:3 rectangle centred in `bounds`.
    fn letterbox(bounds: Bounds<Pixels>) -> Bounds<Pixels> {
        let aspect = WIDTH as f32 / HEIGHT as f32;
        let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
        let fitted = if w / h > aspect {
            size(h * aspect, h)
        } else {
            size(w, w / aspect)
        };
        Bounds::new(
            point(
                bounds.origin.x + px((w - fitted.width) / 2.),
                bounds.origin.y + px((h - fitted.height) / 2.),
            ),
            size(px(fitted.width), px(fitted.height)),
        )
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
            .flex()
            .justify_center()
            .bg(if active { rgb(0x4a6fa5) } else { rgb(0x343a45) })
            .text_color(white())
            .cursor_pointer()
            .child(text.into())
    }

    fn group_thousands(n: u32) -> String {
        if n >= 1000 {
            format!("{},{:03}", n / 1000, n % 1000)
        } else {
            n.to_string()
        }
    }

    fn percentile(values: &[f32], q: f32) -> f32 {
        if values.is_empty() {
            return 0.;
        }
        let mut sorted = values.to_vec();
        sorted.sort_by(f32::total_cmp);
        sorted[((sorted.len() - 1) as f32 * q) as usize]
    }

    /// Run `f` on the view after `seconds`.
    fn after(
        view: &Entity<BoidsView>,
        seconds: f32,
        cx: &mut App,
        f: impl FnOnce(&mut BoidsView, &mut Context<BoidsView>) + 'static,
    ) {
        let view = view.downgrade();
        cx.spawn(async move |cx| {
            cx.background_executor()
                .timer(Duration::from_secs_f32(seconds))
                .await;
            let _ = view.update(cx, f);
        })
        .detach();
    }

    pub fn main() -> Result<()> {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
        let options = Options::parse();
        anyhow::ensure!(options.particles > 0, "--particles must be positive");
        let engine = Engine::new(options.particles, !options.ui_thread)?;
        gpui_platform::application().run(move |cx: &mut App| {
            let displays = cx.displays();
            for (index, display) in displays.iter().enumerate() {
                let bounds = display.bounds();
                println!(
                    "display {index}: {:.0}×{:.0} at ({:.0}, {:.0})",
                    f32::from(bounds.size.width),
                    f32::from(bounds.size.height),
                    f32::from(bounds.origin.x),
                    f32::from(bounds.origin.y)
                );
            }
            let display = options
                .display
                .and_then(|index| displays.get(index))
                .map(|display| (display.id(), display.bounds().size));
            // Fit the window on smaller displays, leaving room for the menu
            // bar and dock.
            let height =
                display.map_or(900., |(_, size)| (f32::from(size.height) - 100.).min(900.));
            let bounds =
                Bounds::centered(display.map(|(id, _)| id), size(px(1000.), px(height)), cx);
            let mut created = None;
            let handle = cx
                .open_lab_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(bounds)),
                        display_id: display.map(|(id, _)| id),
                        // Show engine frames at full rate when unfocused too.
                        inactive_frame_interval: None,
                        ..Default::default()
                    },
                    |window, cx| {
                        window.set_lab_title("XGPUI Interop Boids");
                        window.activate_window();
                        let view = cx.new(|cx| {
                            let mut view = BoidsView::new(engine, cx);
                            view.set_speed(options.speed, cx);
                            view
                        });
                        created = Some(view.clone());
                        view
                    },
                )
                .unwrap();
            let view = created.expect("view");
            if options.resize_test {
                cx.spawn(async move |cx| {
                    let started = Instant::now();
                    loop {
                        cx.background_executor()
                            .timer(Duration::from_millis(8))
                            .await;
                        let t = started.elapsed().as_secs_f32();
                        let resized = handle.update(cx, |_, window, _| {
                            window.resize(size(
                                px(1000. - 300. * (0.5 - 0.5 * (t * 2.1).cos())),
                                px(900. - 250. * (0.5 - 0.5 * (t * 1.3).cos())),
                            ))
                        });
                        if resized.is_err() {
                            break;
                        }
                    }
                })
                .detach();
            }
            if let Some(at) = options.pause_at {
                after(&view, at, cx, |view, cx| view.set_paused(true, cx));
            }
            for at in options.switch_at {
                after(&view, at, cx, |view, cx| {
                    let threaded = !view.engine.threaded();
                    view.set_threaded(threaded, cx);
                });
            }
            if let Some(at) = options.play_at {
                after(&view, at, cx, |view, cx| view.set_paused(false, cx));
            }
            if let Some(seconds) = options.run_seconds {
                after(&view, seconds, cx, |view, cx| {
                    println!("{}", view.summary());
                    cx.quit();
                });
            }
            cx.activate(true);
        });
        Ok(())
    }
}
