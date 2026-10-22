use anyhow::{Context as _, Result, ensure};
use clap::{Parser, ValueEnum};
use gpui::{
    App, Bounds, Context, Hsla, IntoElement, Pixels, Quad, Render, Scene, Size, Window,
    WindowBounds, WindowOptions, canvas, fill, point, prelude::*, px, rgb, size,
};
use serde::Serialize;
use std::{cell::RefCell, fs, path::PathBuf, rc::Rc, time::Instant};

#[derive(Clone, Copy, Debug, ValueEnum, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Pattern {
    Grid,
    Overlap,
}

#[derive(Clone, Parser)]
#[command(
    args_override_self = true,
    about = "One workload: GPUI quad composition and drawing. UI by default; --headless for automation."
)]
struct Options {
    #[arg(long)]
    headless: bool,
    /// Keep the UI drawing until its window is closed.
    #[arg(long, conflicts_with_all = ["headless", "json"])]
    continuous: bool,
    /// Seconds to leave the completed UI visible before quitting (default: immediate).
    #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u64).range(0..=3600), conflicts_with_all = ["headless", "continuous"])]
    exit_delay_secs: u64,
    #[arg(long, requires = "headless", conflicts_with = "png")]
    cpu_only: bool,
    #[arg(long, default_value_t = 1000)]
    count: usize,
    #[arg(long, value_enum, default_value_t = Pattern::Grid)]
    pattern: Pattern,
    #[arg(long, default_value_t = 0.65)]
    opacity: f32,
    #[arg(long, default_value_t = 960)]
    width: u32,
    #[arg(long, default_value_t = 640)]
    height: u32,
    #[arg(long, default_value_t = 60)]
    frames: usize,
    #[arg(long, default_value_t = 10)]
    warmup: usize,
    #[arg(long)]
    json: Option<PathBuf>,
    #[arg(long, requires = "headless")]
    png: Option<PathBuf>,
}
impl Options {
    fn validate(&self) -> Result<()> {
        ensure!(
            (1..=20_000).contains(&self.count),
            "count must be 1..=20000"
        );
        ensure!(
            (64..=4096).contains(&self.width) && (64..=4096).contains(&self.height),
            "width and height must be 64..=4096"
        );
        ensure!(
            self.opacity.is_finite() && self.opacity > 0.0 && self.opacity <= 1.0,
            "opacity must be finite and in (0, 1]"
        );
        ensure!(
            (1..=10_000).contains(&self.frames) && self.warmup <= 1000,
            "frames must be 1..=10000; warmup at most 1000"
        );
        Ok(())
    }
}

// Both modes use exactly this geometry and color generator. UI uses logical
// pixels; headless fixes the scale factor at 1 for controlled comparisons.
fn workload(options: &Options, viewport: Size<Pixels>) -> Vec<(Bounds<Pixels>, Hsla)> {
    let columns = (options.count as f32).sqrt().ceil() as usize;
    let rows = options.count.div_ceil(columns);
    let cell_w = f32::from(viewport.width) / columns as f32;
    let cell_h = f32::from(viewport.height) / rows as f32;
    (0..options.count)
        .map(|i| {
            let (x, y, w, h) = match options.pattern {
                Pattern::Grid => (
                    (i % columns) as f32 * cell_w,
                    (i / columns) as f32 * cell_h,
                    cell_w * 0.85,
                    cell_h * 0.85,
                ),
                Pattern::Overlap => (
                    f32::from(viewport.width) * 0.3,
                    f32::from(viewport.height) * 0.3,
                    f32::from(viewport.width) * 0.4,
                    f32::from(viewport.height) * 0.4,
                ),
            };
            let colors = [0x286b8a, 0xb85543, 0x4c8858, 0x8664a0, 0xba8d37, 0x3b8381];
            (
                Bounds::new(point(px(x), px(y)), size(px(w), px(h))),
                rgb(colors[i % colors.len()]).alpha(options.opacity).into(),
            )
        })
        .collect()
}

fn make_scene(options: &Options) -> Scene {
    let viewport = size(px(options.width as f32), px(options.height as f32));
    let mask = Bounds::new(point(px(0.), px(0.)), viewport).scale(1.);
    let mut scene = Scene::default();
    scene.insert_primitive(Quad {
        bounds: mask,
        content_mask: gpui::ContentMask { bounds: mask },
        background: rgb(0xf1f0e8).into(),
        ..Default::default()
    });
    for (bounds, color) in workload(options, viewport) {
        scene.insert_primitive(Quad {
            bounds: bounds.scale(1.),
            content_mask: gpui::ContentMask { bounds: mask },
            background: color.into(),
            ..Default::default()
        });
    }
    scene
}

#[derive(Serialize)]
struct Distribution {
    median: f64,
    p95: f64,
    min: f64,
    max: f64,
}
fn distribution(mut samples: Vec<f64>) -> Distribution {
    samples.sort_by(f64::total_cmp);
    let len = samples.len();
    Distribution {
        median: (samples[(len - 1) / 2] + samples[len / 2]) / 2.0,
        p95: samples[(len * 95).div_ceil(100).saturating_sub(1)],
        min: samples[0],
        max: samples[len - 1],
    }
}
#[derive(Serialize)]
struct Report {
    schema_version: u32,
    app: &'static str,
    upstream_commit: String,
    os: &'static str,
    arch: &'static str,
    optimized_build: bool,
    renderer: &'static str,
    pattern: Pattern,
    requested_quads: usize,
    scene_quads_including_background: usize,
    batches: usize,
    opacity: f32,
    width: u32,
    height: u32,
    measured_frames: usize,
    warmup_frames: usize,
    compose_us: Distribution,
    finish_us: Distribution,
    batch_traversal_us: Distribution,
    render_and_readback_ms: Option<Distribution>,
    timing_scope: &'static str,
}
fn headless(options: &Options) -> Result<()> {
    let mut renderer = if options.cpu_only {
        None
    } else {
        Some(gpui_platform::current_headless_renderer()?.context(
            "No headless renderer for this target; use --cpu-only to measure composition",
        )?)
    };
    let mut compose = Vec::new();
    let mut finish = Vec::new();
    let mut batches_time = Vec::new();
    let mut rendering = Vec::new();
    let mut final_counts = (0, 0);
    let target = size(
        gpui::DevicePixels(options.width as i32),
        gpui::DevicePixels(options.height as i32),
    );
    for frame in 0..options.warmup + options.frames {
        let started = Instant::now();
        let mut scene = make_scene(options);
        let composed = started.elapsed().as_secs_f64() * 1e6;
        let started = Instant::now();
        scene.finish();
        let finished = started.elapsed().as_secs_f64() * 1e6;
        let started = Instant::now();
        let batches = std::hint::black_box(scene.batches().count());
        let batched = started.elapsed().as_secs_f64() * 1e6;
        final_counts = (scene.quads.len(), batches);
        if let Some(renderer) = renderer.as_mut() {
            let started = Instant::now();
            let image = renderer.render_scene_to_image(&scene, target)?;
            let elapsed = started.elapsed().as_secs_f64() * 1000.;
            ensure!(
                image.width() == options.width && image.height() == options.height,
                "Unexpected image dimensions"
            );
            ensure!(
                image.pixels().any(|p| p.0[3] == 255),
                "Renderer returned no opaque pixels"
            );
            let sample = match options.pattern {
                Pattern::Grid => image.get_pixel(0, 0),
                Pattern::Overlap => image.get_pixel(options.width / 2, options.height / 2),
            };
            ensure!(
                options.opacity < 0.1
                    || sample.0[..3]
                        .iter()
                        .zip([241u8, 240, 232])
                        .any(|(actual, background)| actual.abs_diff(background) > 2),
                "Renderer returned background where a workload quad was expected"
            );
            if frame >= options.warmup {
                rendering.push(elapsed);
            }
            if frame + 1 == options.warmup + options.frames
                && let Some(path) = &options.png
            {
                image
                    .save(path)
                    .with_context(|| format!("Save {}", path.display()))?;
            }
        }
        if frame >= options.warmup {
            compose.push(composed);
            finish.push(finished);
            batches_time.push(batched);
        }
    }
    let provenance: serde_json::Value =
        serde_json::from_str(include_str!("../../../upstream.json"))?;
    let report = Report {
        schema_version: 1,
        app: "quad-bench",
        upstream_commit: provenance["commit"].as_str().unwrap_or("unknown").into(),
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        optimized_build: !cfg!(debug_assertions),
        renderer: if options.cpu_only {
            "none (CPU only)"
        } else if cfg!(target_os = "macos") {
            "Metal headless"
        } else {
            "platform headless"
        },
        pattern: options.pattern,
        requested_quads: options.count,
        scene_quads_including_background: final_counts.0,
        batches: final_counts.1,
        opacity: options.opacity,
        width: options.width,
        height: options.height,
        measured_frames: options.frames,
        warmup_frames: options.warmup,
        compose_us: distribution(compose),
        finish_us: distribution(finish),
        batch_traversal_us: distribution(batches_time),
        render_and_readback_ms: if rendering.is_empty() {
            None
        } else {
            Some(distribution(rendering))
        },
        timing_scope: "CPU composition includes geometry generation, allocation and Scene insertion; finish is sorting; batch traversal counts batches. Offscreen render/readback includes CPU encoding, target allocation, GPU completion and image copy, not isolated GPU time. No window presentation is measured.",
    };
    let json = serde_json::to_string_pretty(&report)?;
    if let Some(path) = &options.json {
        fs::write(path, &json)?;
    }
    println!("{json}");
    Ok(())
}

struct Viewer {
    options: Options,
    stats: Rc<RefCell<UiStats>>,
}
#[derive(Default)]
struct UiStats {
    warmup_done: usize,
    focused_frames: usize,
    unfocused_frames: usize,
    last_focus: Option<bool>,
    focus_transitions: usize,
    last: Option<Instant>,
    paint_ms: Vec<f64>,
    cadence_ms: Vec<f64>,
    finished: bool,
}
impl UiStats {
    fn record(&mut self, options: &Options, focused: bool, at: Instant, paint_ms: f64) -> bool {
        if self.finished {
            return false;
        }
        if self.warmup_done < options.warmup {
            self.warmup_done += 1;
            return false;
        }
        if self.last_focus.is_some_and(|last| last != focused) {
            self.focus_transitions += 1;
        }
        self.last_focus = Some(focused);
        if focused {
            self.focused_frames += 1;
        } else {
            self.unfocused_frames += 1;
        }
        if let Some(last) = self.last {
            self.cadence_ms
                .push(at.duration_since(last).as_secs_f64() * 1000.);
        }
        self.last = Some(at);
        self.paint_ms.push(paint_ms);
        self.finished = !options.continuous && self.paint_ms.len() == options.frames;
        self.finished
    }

    fn finish(&mut self, at: Instant) {
        if let Some(last) = self.last.take() {
            self.cadence_ms
                .push(at.duration_since(last).as_secs_f64() * 1000.);
        }
    }
}

#[derive(Serialize)]
struct UiReport {
    schema_version: u32,
    app: &'static str,
    mode: &'static str,
    upstream_commit: String,
    optimized_build: bool,
    os: &'static str,
    arch: &'static str,
    pattern: Pattern,
    requested_quads: usize,
    opacity: f32,
    logical_width: f32,
    logical_height: f32,
    scale_factor: f32,
    measured_frames: usize,
    warmup_frames: usize,
    callback_fps: f64,
    cpu_paint_ms: Distribution,
    callback_interval_ms: Distribution,
    metal_hud_enabled: bool,
    inactive_throttling_disabled: bool,
    focused_frames: usize,
    unfocused_frames: usize,
    sampled_focus_transitions: usize,
    timing_scope: &'static str,
}

impl Render for Viewer {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        if !self.stats.borrow().finished {
            window.request_animation_frame();
        }
        let options = self.options.clone();
        let stats = self.stats.clone();
        canvas(
            |_, _, _| (),
            move |bounds, (), window, _| {
                let started = Instant::now();
                window.paint_quad(fill(bounds, rgb(0xf1f0e8)));
                for (mut rect, color) in workload(&options, bounds.size) {
                    rect.origin.x += bounds.origin.x;
                    rect.origin.y += bounds.origin.y;
                    window.paint_quad(fill(rect, color));
                }
                let elapsed = started.elapsed().as_secs_f64() * 1000.;
                let mut samples = stats.borrow_mut();
                let done = samples.record(&options, window.is_window_active(), started, elapsed);
                if options.continuous && samples.paint_ms.len() >= 60 {
                    let paint = samples.paint_ms.iter().sum::<f64>() / samples.paint_ms.len() as f64;
                    let cadence = samples.cadence_ms.iter().sum::<f64>() / samples.cadence_ms.len().max(1) as f64;
                    window.set_window_title(&format!(
                        "Quad bench | {:?} | {} quads | CPU paint {paint:.2} ms | callback cadence {cadence:.2} ms",
                        options.pattern, options.count,
                    ));
                    samples.paint_ms.clear();
                    samples.cadence_ms.clear();
                }
                drop(samples);
                if done {
                    let options = options.clone();
                    let stats = stats.clone();
                    let viewport = bounds.size;
                    let scale_factor = window.scale_factor();
                    // This callback follows the final paint's submission. It is
                    // a scheduling boundary, not a GPU-completion timestamp.
                    window.on_next_frame(move |window, cx| {
                        let mut samples = stats.borrow_mut();
                        samples.finish(Instant::now());
                        let fps = samples.cadence_ms.len() as f64 * 1000.
                            / samples.cadence_ms.iter().sum::<f64>();
                        let provenance: serde_json::Value = serde_json::from_str(include_str!("../../../upstream.json"))
                            .unwrap_or_default();
                        let report = UiReport {
                            schema_version: 2, app: "quad-bench", mode: "windowed-callback-cadence",
                            upstream_commit: provenance["commit"].as_str().unwrap_or("unknown").into(),
                            optimized_build: !cfg!(debug_assertions), os: std::env::consts::OS, arch: std::env::consts::ARCH,
                            pattern: options.pattern, requested_quads: options.count, opacity: options.opacity,
                            logical_width: f32::from(viewport.width), logical_height: f32::from(viewport.height), scale_factor,
                            measured_frames: samples.paint_ms.len(), warmup_frames: options.warmup,
                            inactive_throttling_disabled: true,
                            focused_frames: samples.focused_frames,
                            unfocused_frames: samples.unfocused_frames,
                            sampled_focus_transitions: samples.focus_transitions,
                            callback_fps: fps,
                            cpu_paint_ms: distribution(std::mem::take(&mut samples.paint_ms)),
                            callback_interval_ms: distribution(std::mem::take(&mut samples.cadence_ms)),
                            metal_hud_enabled: std::env::var("MTL_HUD_ENABLED").is_ok_and(|v| v == "1"),
                            timing_scope: "Window paint callback cadence, including platform pacing; not confirmed displayed FPS or isolated GPU time. CPU paint includes geometry generation and paint enqueueing. The final interval ends at the next frame callback after submission. Metal HUD metrics are separate and are not captured in this JSON. Keep the window size and display settings fixed during comparisons.",
                        };
                        let result = (|| -> Result<()> {
                            let json = serde_json::to_string_pretty(&report)?;
                            if let Some(path) = &options.json {
                                fs::write(path, &json).with_context(|| format!("Save {}", path.display()))?;
                            }
                            println!("{json}");
                            Ok(())
                        })();
                        match result {
                            Ok(()) => window.set_window_title(&format!(
                                "DONE | {} frames | callback {:.1} fps | CPU paint median {:.3} ms | quitting in {} s",
                                report.measured_frames, fps, report.cpu_paint_ms.median, options.exit_delay_secs,
                            )),
                            Err(error) => {
                                eprintln!("Could not save benchmark report: {error:#}");
                                window.set_window_title("FAILED to save report | see terminal");
                                // Cocoa termination does not return through main; preserve
                                // a failing exit status for suite runners.
                                std::process::exit(1);
                            }
                        }
                        drop(samples);
                        if options.exit_delay_secs == 0 {
                            cx.quit();
                        } else {
                            let delay = std::time::Duration::from_secs(options.exit_delay_secs);
                            cx.spawn(async move |cx| {
                                cx.background_executor().timer(delay).await;
                                cx.update(|cx| cx.quit());
                            }).detach();
                        }
                    });
                }
            },
        ).size_full()
    }
}
fn main() -> Result<()> {
    let options = Options::parse();
    options.validate()?;
    if options.headless {
        return headless(&options);
    }
    let stats = Rc::new(RefCell::new(UiStats::default()));
    gpui_platform::application().run(move |cx: &mut App| {
        let bounds = Bounds::centered(
            None,
            size(px(options.width as f32), px(options.height as f32)),
            cx,
        );
        if let Err(error) = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                // A benchmark must not inherit the normal inactive-window 30 Hz cap.
                inactive_frame_interval: None,
                ..Default::default()
            },
            |_, cx| cx.new(|_| Viewer { options, stats }),
        ) {
            eprintln!("Could not open quad benchmark window: {error:#}");
            std::process::exit(1);
        }
        // A native benchmark should start visible, rather than waiting for
        // frame delivery to a background/occluded command-line app.
        cx.activate(true);
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn options(pattern: Pattern) -> Options {
        Options {
            headless: true,
            continuous: false,
            exit_delay_secs: 0,
            cpu_only: true,
            count: 16,
            pattern,
            opacity: 0.65,
            width: 128,
            height: 128,
            frames: 2,
            warmup: 0,
            json: None,
            png: None,
        }
    }
    #[test]
    fn ui_stops_at_the_requested_frame_and_excludes_warmup() {
        let mut options = options(Pattern::Grid);
        options.headless = false;
        options.warmup = 1;
        let start = Instant::now();
        let mut stats = UiStats::default();
        assert!(!stats.record(&options, true, start, 999.));
        assert!(!stats.record(
            &options,
            true,
            start + std::time::Duration::from_millis(10),
            1.
        ));
        assert!(stats.record(
            &options,
            true,
            start + std::time::Duration::from_millis(20),
            2.
        ));
        assert!(!stats.record(
            &options,
            true,
            start + std::time::Duration::from_millis(25),
            999.
        ));
        stats.finish(start + std::time::Duration::from_millis(30));
        assert_eq!(stats.paint_ms, vec![1., 2.]);
        assert_eq!(stats.cadence_ms, vec![10., 10.]);
        assert!(stats.finished);
    }

    #[test]
    fn ui_focus_counts_exclude_warmup_and_ignore_paints_after_completion() {
        let mut options = options(Pattern::Grid);
        options.warmup = 1;
        let at = Instant::now();
        let mut stats = UiStats::default();
        stats.record(&options, false, at, 1.);
        stats.record(&options, true, at, 1.);
        assert!(stats.record(&options, false, at, 1.));
        stats.record(&options, true, at, 1.);
        assert_eq!(stats.focused_frames, 1);
        assert_eq!(stats.unfocused_frames, 1);
        assert_eq!(stats.focus_transitions, 1);
    }

    #[test]
    fn continuous_mode_does_not_stop_at_the_frame_limit() {
        let mut options = options(Pattern::Grid);
        options.continuous = true;
        let start = Instant::now();
        let mut stats = UiStats::default();
        for frame in 0..4 {
            assert!(!stats.record(
                &options,
                true,
                start + std::time::Duration::from_millis(frame * 10),
                1.
            ));
        }
        assert!(!stats.finished);
    }

    #[test]
    fn recipe_defaults_can_be_overridden_and_invalid_modes_are_rejected() {
        let options =
            Options::try_parse_from(["quad-bench", "--frames", "600", "--frames", "2"]).unwrap();
        assert_eq!(options.frames, 2);
        assert_eq!(options.exit_delay_secs, 0);
        assert_eq!(
            Options::try_parse_from(["quad-bench", "--exit-delay-secs", "10"])
                .unwrap()
                .exit_delay_secs,
            10
        );
        assert!(
            Options::try_parse_from(["quad-bench", "--headless", "--exit-delay-secs", "10"])
                .is_err()
        );
        assert!(
            Options::try_parse_from(["quad-bench", "--continuous", "--exit-delay-secs", "10"])
                .is_err()
        );
        assert!(Options::try_parse_from(["quad-bench", "--continuous", "--headless"]).is_err());
        assert!(
            Options::try_parse_from(["quad-bench", "--continuous", "--json", "out.json"]).is_err()
        );
        assert!(Options::try_parse_from(["quad-bench", "--json", "out.json"]).is_ok());
    }

    #[test]
    fn geometry_is_in_bounds_and_both_patterns_emit_the_requested_work() {
        for pattern in [Pattern::Grid, Pattern::Overlap] {
            let options = options(pattern);
            let rects = workload(&options, size(px(128.), px(128.)));
            assert_eq!(rects.len(), 16);
            assert!(rects.iter().all(|(b, _)| b.origin.x >= px(0.)
                && b.origin.y >= px(0.)
                && b.right() <= px(128.)
                && b.bottom() <= px(128.)));
            let mut scene = make_scene(&options);
            scene.finish();
            assert_eq!(scene.quads.len(), 17);
            assert_eq!(scene.batches().count(), 1);
            let max_order = scene.quads.iter().map(|q| q.order).max().unwrap();
            match pattern {
                Pattern::Grid => assert_eq!(max_order, 2),
                Pattern::Overlap => assert_eq!(max_order, 17),
            }
        }
    }
    #[test]
    fn percentile_uses_nearest_rank_and_single_samples_work() {
        let summary = distribution((1..=100).map(f64::from).collect());
        assert_eq!(summary.p95, 95.);
        assert_eq!(summary.min, 1.);
        assert_eq!(summary.max, 100.);
        assert_eq!(distribution(vec![3.]).p95, 3.);
    }
    #[test]
    fn invalid_inputs_fail_before_allocation_or_renderer_creation() {
        let mut options = options(Pattern::Grid);
        options.count = 0;
        assert!(options.validate().is_err());
        options.count = 16;
        options.opacity = f32::NAN;
        assert!(options.validate().is_err());
    }
}
