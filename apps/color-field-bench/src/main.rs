mod workload;

use anyhow::{Context as _, Result, ensure};
use clap::Parser;
use gpui::{
    App, Bounds, Context, DevicePixels, IntoElement, Pixels, Quad, Render, Scene, Size, Window,
    WindowBounds, WindowOptions, canvas, point, prelude::*, px, size,
};
use serde::Serialize;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    fs,
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};
use workload::Method;

const SOURCE: &str = "gpui-luma/crates/luma-color/src/color_field/field/paint_vector.rs @ b5a4a23ce6ed98731c1e3aef0abec02da4879274";

#[derive(Clone, Parser)]
#[command(
    about = "SV color-field quad rendering, adapted from gpui-luma-color",
    args_override_self = true
)]
struct Options {
    #[arg(long)]
    headless: bool,
    #[arg(long, conflicts_with_all = ["headless", "json"])]
    continuous: bool,
    #[arg(long, requires = "headless", conflicts_with = "png")]
    cpu_only: bool,
    #[arg(long, value_enum, default_value_t = Method::Layered)]
    method: Method,
    /// Sampling target along the longest axis; the source algorithm enforces a minimum 0.25 scale.
    #[arg(long, default_value_t = 128)]
    samples: usize,
    #[arg(long, default_value_t = 210.)]
    hue: f32,
    /// Advance hue by one degree per rendered frame, including warmup.
    #[arg(long)]
    animate: bool,
    #[arg(long, default_value_t = 512)]
    width: u32,
    #[arg(long, default_value_t = 384)]
    height: u32,
    #[arg(long, default_value_t = 600)]
    frames: usize,
    #[arg(long, default_value_t = 120)]
    warmup: usize,
    #[arg(long)]
    json: Option<PathBuf>,
    #[arg(long, requires = "headless")]
    png: Option<PathBuf>,
    #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u64).range(0..=3600), conflicts_with_all = ["headless", "continuous"])]
    exit_delay_secs: u64,
    /// Fail a bounded UI run if frame delivery stops; zero disables the watchdog.
    #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u64).range(0..=3600))]
    timeout_secs: u64,
}
impl Options {
    fn validate(&self) -> Result<()> {
        ensure!(
            (64..=1024).contains(&self.width) && (64..=1024).contains(&self.height),
            "width and height must be 64..=1024"
        );
        ensure!(
            (16..=512).contains(&self.samples),
            "samples must be 16..=512"
        );
        ensure!(
            self.hue.is_finite() && (0. ..360.).contains(&self.hue),
            "hue must be finite and in [0, 360)"
        );
        ensure!(
            (1..=10000).contains(&self.frames) && self.warmup <= 1000,
            "frames must be 1..=10000; warmup at most 1000"
        );
        Ok(())
    }
    fn hue_at(&self, frame: usize) -> f32 {
        (self.hue
            + if self.animate {
                (frame % 360) as f32
            } else {
                0.
            })
        .rem_euclid(360.)
    }
}

#[derive(Serialize)]
struct Distribution {
    median: f64,
    p95: f64,
    min: f64,
    max: f64,
}
fn distribution(mut values: Vec<f64>) -> Option<Distribution> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let n = values.len();
    Some(Distribution {
        median: (values[(n - 1) / 2] + values[n / 2]) / 2.,
        p95: values[(n * 95).div_ceil(100) - 1],
        min: values[0],
        max: values[n - 1],
    })
}
#[derive(Default)]
struct Samples {
    generate_us: Vec<f64>,
    compose_us: Vec<f64>,
    finish_us: Vec<f64>,
    render_readback_ms: Vec<f64>,
    cpu_paint_ms: Vec<f64>,
    callback_interval_ms: Vec<f64>,
}
#[derive(Serialize)]
struct Report {
    schema_version: u32,
    app: &'static str,
    workload_revision: u32,
    source_evidence: &'static str,
    upstream_commit: String,
    mode: &'static str,
    renderer: &'static str,
    optimized_build: bool,
    os: &'static str,
    arch: &'static str,
    method: Method,
    samples_target: usize,
    sampling_grid: Option<[u32; 2]>,
    quads_per_frame: usize,
    primitive_batches: Option<usize>,
    width: f32,
    height: f32,
    scale_factor: f32,
    measured_frames: usize,
    warmup_frames: usize,
    animated: bool,
    first_measured_hue: f32,
    last_measured_hue: f32,
    focused_frames: Option<usize>,
    unfocused_frames: Option<usize>,
    inactive_throttling_disabled: bool,
    metal_hud_enabled: bool,
    callback_fps: Option<f64>,
    timings: BTreeMap<&'static str, Option<Distribution>>,
    timing_scope: &'static str,
}
fn report(
    options: &Options,
    samples: Samples,
    viewport: Size<Pixels>,
    scale: f32,
    quad_count: usize,
    batches: Option<usize>,
    focus: Option<usize>,
) -> Report {
    let width = f32::from(viewport.width);
    let height = f32::from(viewport.height);
    let (cols, rows) = workload::sampling(width, height, options.samples);
    let fps = if samples.callback_interval_ms.is_empty() {
        None
    } else {
        Some(
            samples.callback_interval_ms.len() as f64 * 1000.
                / samples.callback_interval_ms.iter().sum::<f64>(),
        )
    };
    let provenance: serde_json::Value =
        serde_json::from_str(include_str!("../../../upstream.json")).unwrap_or_default();
    Report {
        schema_version: 1,
        app: "color-field-bench",
        workload_revision: 1,
        source_evidence: SOURCE,
        upstream_commit: provenance["commit"].as_str().unwrap_or("unknown").into(),
        mode: if options.headless {
            "headless"
        } else {
            "windowed-callback-cadence"
        },
        renderer: if options.cpu_only {
            "none (CPU only)"
        } else if cfg!(target_os = "macos") {
            "Metal"
        } else {
            "platform renderer"
        },
        optimized_build: !cfg!(debug_assertions),
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        method: options.method,
        samples_target: options.samples,
        sampling_grid: (options.method == Method::Sampled).then_some([cols, rows]),
        quads_per_frame: quad_count,
        primitive_batches: batches,
        width,
        height,
        scale_factor: scale,
        measured_frames: options.frames,
        warmup_frames: options.warmup,
        animated: options.animate,
        first_measured_hue: options.hue_at(options.warmup),
        last_measured_hue: options.hue_at(options.warmup + options.frames - 1),
        focused_frames: focus,
        unfocused_frames: focus.map(|f| options.frames - f),
        inactive_throttling_disabled: !options.headless,
        metal_hud_enabled: std::env::var("MTL_HUD_ENABLED").is_ok_and(|v| v == "1"),
        callback_fps: fps,
        timings: BTreeMap::from([
            ("generate_us", distribution(samples.generate_us)),
            ("compose_us", distribution(samples.compose_us)),
            ("finish_us", distribution(samples.finish_us)),
            (
                "render_readback_ms",
                distribution(samples.render_readback_ms),
            ),
            ("cpu_paint_ms", distribution(samples.cpu_paint_ms)),
            (
                "callback_interval_ms",
                distribution(samples.callback_interval_ms),
            ),
        ]),
        timing_scope: "generate_us includes allocation, color calculation and PaintQuad generation. Headless compose_us converts/inserts generated quads into Scene; finish_us sorts it. render_readback_ms includes target allocation, CPU encoding, GPU completion and image copy, not isolated GPU time. Image checks and file writes are outside measured intervals. UI cpu_paint_ms is Window::paint_quad enqueue time, excluding generation; callback FPS is scheduling cadence, not confirmed presented FPS. UI uses logical pixels/native scale; headless uses physical pixels at scale 1. No control layout, image caching or wide-gamut output is measured.",
    }
}
fn save(options: &Options, report: &Report) -> Result<()> {
    let json = serde_json::to_string_pretty(report)?;
    if let Some(path) = &options.json {
        fs::write(path, &json).with_context(|| format!("Save {}", path.display()))?;
    }
    println!("{json}");
    Ok(())
}
fn scene(quads: Vec<gpui::PaintQuad>, viewport: Size<Pixels>) -> Scene {
    let mask = gpui::ContentMask {
        bounds: Bounds::new(point(px(0.), px(0.)), viewport).scale(1.),
    };
    let mut scene = Scene::default();
    for q in quads {
        scene.insert_primitive(Quad {
            bounds: q.bounds.scale(1.),
            content_mask: mask,
            background: q.background,
            corner_radii: q.corner_radii.scale(1.),
            border_widths: q.border_widths.scale(1.),
            border_color: q.border_color,
            border_style: q.border_style,
            ..Default::default()
        });
    }
    scene
}
fn headless(options: &Options) -> Result<()> {
    let mut renderer = if options.cpu_only {
        None
    } else {
        Some(
            gpui_platform::current_headless_renderer()?
                .context("No headless renderer for this target; use --cpu-only")?,
        )
    };
    let viewport = size(px(options.width as f32), px(options.height as f32));
    let bounds = Bounds::new(point(px(0.), px(0.)), viewport);
    let mut samples = Samples::default();
    let mut counts = (0, 0);
    for frame in 0..options.warmup + options.frames {
        let hue = options.hue_at(frame);
        let start = Instant::now();
        let quads = workload::quads(options.method, bounds, hue, options.samples);
        let generated = start.elapsed().as_secs_f64() * 1e6;
        let start = Instant::now();
        let mut scene = scene(quads, viewport);
        let composed = start.elapsed().as_secs_f64() * 1e6;
        let start = Instant::now();
        scene.finish();
        let finished = start.elapsed().as_secs_f64() * 1e6;
        counts = (
            scene.quads.len(),
            std::hint::black_box(scene.batches().count()),
        );
        if let Some(renderer) = renderer.as_mut() {
            let start = Instant::now();
            let image = renderer.render_scene_to_image(
                &scene,
                size(
                    DevicePixels(options.width as i32),
                    DevicePixels(options.height as i32),
                ),
            )?;
            let elapsed = start.elapsed().as_secs_f64() * 1000.;
            ensure!(
                image.width() == options.width && image.height() == options.height,
                "Unexpected image dimensions"
            );
            // Check nine interior points against the analytical SV plane. The
            // fallback samples cell midpoints; allow its bounded sampling error
            // plus 8-bit quantization/dithering. This is not gamut validation.
            let (cols, rows) =
                workload::sampling(options.width as f32, options.height as f32, options.samples);
            let tolerance = if options.method == Method::Layered {
                6.
            } else {
                255. * (1. / cols as f32 + 1. / rows as f32) + 6.
            };
            for u in [0.1, 0.5, 0.9] {
                for v in [0.1, 0.5, 0.9] {
                    let x = (u * options.width as f32) as u32;
                    let y = (v * options.height as f32) as u32;
                    let expected = workload::hsv(
                        hue,
                        (x as f32 + 0.5) / options.width as f32,
                        1. - (y as f32 + 0.5) / options.height as f32,
                    );
                    let pixel = image.get_pixel(x, y).0;
                    ensure!(pixel[3] == 255, "Non-opaque field at ({x},{y})");
                    for (actual, expected) in
                        pixel[..3].iter().zip([expected.r, expected.g, expected.b])
                    {
                        ensure!(
                            (*actual as f32 - expected * 255.).abs() <= tolerance,
                            "SV color mismatch at ({x},{y}): actual {actual}, expected {}, tolerance {tolerance}",
                            expected * 255.
                        );
                    }
                }
            }
            if frame >= options.warmup {
                samples.render_readback_ms.push(elapsed);
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
            samples.generate_us.push(generated);
            samples.compose_us.push(composed);
            samples.finish_us.push(finished);
        }
    }
    save(
        options,
        &report(
            options,
            samples,
            viewport,
            1.,
            counts.0,
            Some(counts.1),
            None,
        ),
    )
}

#[derive(Default)]
struct UiStats {
    frame: usize,
    last: Option<Instant>,
    samples: Samples,
    focused: usize,
    finished: bool,
    completed: bool,
}
impl UiStats {
    fn record(
        &mut self,
        options: &Options,
        at: Instant,
        generated: f64,
        painted: f64,
        focused: bool,
    ) -> bool {
        if self.finished {
            return false;
        }
        if self.frame >= options.warmup {
            if let Some(last) = self.last {
                self.samples
                    .callback_interval_ms
                    .push(at.duration_since(last).as_secs_f64() * 1000.);
            }
            self.last = Some(at);
            self.samples.generate_us.push(generated);
            self.samples.cpu_paint_ms.push(painted);
            if focused {
                self.focused += 1;
            }
        }
        self.frame += 1;
        self.finished = !options.continuous && self.frame == options.warmup + options.frames;
        self.finished
    }
}
struct Viewer {
    options: Options,
    stats: Rc<RefCell<UiStats>>,
}
impl Render for Viewer {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        if !self.stats.borrow().finished {
            window.request_animation_frame();
        }
        let options = self.options.clone();
        let state = self.stats.clone();
        canvas(
            |_, _, _| (),
            move |bounds, (), window, _| {
                let mut stats = state.borrow_mut();
                let frame = if stats.finished {
                    stats.frame.saturating_sub(1)
                } else {
                    stats.frame
                };
                let started = Instant::now();
                let quads = workload::quads(
                    options.method,
                    bounds,
                    options.hue_at(frame),
                    options.samples,
                );
                let generated = started.elapsed().as_secs_f64() * 1e6;
                let count = quads.len();
                let paint_start = Instant::now();
                for quad in quads {
                    window.paint_quad(quad);
                }
                let painted = paint_start.elapsed().as_secs_f64() * 1000.;
                let done = stats.record(
                    &options,
                    started,
                    generated,
                    painted,
                    window.is_window_active(),
                );
                if options.continuous {
                    if stats.samples.cpu_paint_ms.len() >= 60 {
                        window.set_window_title(&format!(
                            "SV field | {:?} | {count} quads | continuous",
                            options.method
                        ));
                        stats.samples = Samples::default();
                        stats.focused = 0;
                    }
                    return;
                }
                if done {
                    drop(stats);
                    let options = options.clone();
                    let state = state.clone();
                    let viewport = bounds.size;
                    let scale = window.scale_factor();
                    window.on_next_frame(move |window, cx| {
                        let mut stats = state.borrow_mut();
                        if let Some(last) = stats.last.take() {
                            stats
                                .samples
                                .callback_interval_ms
                                .push(last.elapsed().as_secs_f64() * 1000.);
                        }
                        let samples = std::mem::take(&mut stats.samples);
                        let report = report(
                            &options,
                            samples,
                            viewport,
                            scale,
                            count,
                            None,
                            Some(stats.focused),
                        );
                        if let Err(error) = save(&options, &report) {
                            eprintln!("Color-field benchmark failed: {error:#}");
                            std::process::exit(1);
                        }
                        stats.completed = true;
                        drop(stats);
                        window.set_window_title(&format!(
                            "DONE | {:?} | {count} quads | quitting in {} s",
                            options.method, options.exit_delay_secs
                        ));
                        if options.exit_delay_secs == 0 {
                            cx.quit();
                        } else {
                            let delay = Duration::from_secs(options.exit_delay_secs);
                            cx.spawn(async move |cx| {
                                cx.background_executor().timer(delay).await;
                                cx.update(|cx| cx.quit());
                            })
                            .detach();
                        }
                    });
                }
            },
        )
        .size_full()
    }
}

fn main() -> Result<()> {
    let options = Options::parse();
    options.validate()?;
    if options.headless {
        return headless(&options);
    }
    let state = Rc::new(RefCell::new(UiStats::default()));
    gpui_platform::application().run(move |cx: &mut App| {
        if !options.continuous && options.timeout_secs > 0 {
            let state = state.clone();
            let timeout = Duration::from_secs(options.timeout_secs);
            cx.spawn(async move |cx| {
                cx.background_executor().timer(timeout).await;
                if !state.borrow().completed {
                    eprintln!("Color-field benchmark timed out before saving its report");
                    std::process::exit(1);
                }
            })
            .detach();
        }
        let bounds = Bounds::centered(
            None,
            size(px(options.width as f32), px(options.height as f32)),
            cx,
        );
        if let Err(error) = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                inactive_frame_interval: None,
                ..Default::default()
            },
            |_, cx| {
                cx.new(|_| Viewer {
                    options,
                    stats: state,
                })
            },
        ) {
            eprintln!("Could not open color-field benchmark: {error:#}");
            std::process::exit(1);
        }
        cx.activate(true);
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ui_sampling_excludes_warmup_and_finishes_at_the_requested_frame() {
        let options =
            Options::try_parse_from(["color-field-bench", "--warmup", "1", "--frames", "2"])
                .unwrap();
        let mut stats = UiStats::default();
        let start = Instant::now();
        assert!(!stats.record(&options, start, 999., 999., false));
        assert!(!stats.record(&options, start + Duration::from_millis(10), 1., 2., true));
        assert!(stats.record(&options, start + Duration::from_millis(20), 3., 4., false));
        assert!(!stats.record(
            &options,
            start + Duration::from_millis(30),
            999.,
            999.,
            true
        ));
        assert_eq!(stats.samples.generate_us, vec![1., 3.]);
        assert_eq!(stats.samples.cpu_paint_ms, vec![2., 4.]);
        assert_eq!(stats.samples.callback_interval_ms, vec![10.]);
        assert_eq!(stats.focused, 1);
        assert_eq!(stats.frame, 3);
    }

    #[test]
    fn invalid_workloads_and_conflicting_run_modes_are_rejected() {
        let mut options = Options::try_parse_from(["color-field-bench"]).unwrap();
        options.samples = 0;
        assert!(options.validate().is_err());
        options.samples = 128;
        options.hue = f32::NAN;
        assert!(options.validate().is_err());
        assert!(
            Options::try_parse_from(["color-field-bench", "--continuous", "--headless"]).is_err()
        );
        assert!(
            Options::try_parse_from([
                "color-field-bench",
                "--headless",
                "--cpu-only",
                "--png",
                "a.png"
            ])
            .is_err()
        );
    }
    #[test]
    fn animated_hue_and_report_counts_include_warmup_consistently() {
        let options = Options::try_parse_from([
            "color-field-bench",
            "--animate",
            "--hue",
            "350",
            "--warmup",
            "20",
            "--frames",
            "3",
        ])
        .unwrap();
        assert_eq!(options.hue_at(20), 10.);
        assert_eq!(options.hue_at(22), 12.);
        let report = report(
            &options,
            Samples::default(),
            size(px(512.), px(384.)),
            2.,
            3,
            None,
            Some(2),
        );
        assert_eq!(report.first_measured_hue, 10.);
        assert_eq!(report.last_measured_hue, 12.);
        assert_eq!(report.unfocused_frames, Some(1));
        assert_eq!(report.sampling_grid, None);
        assert!(report.timings["render_readback_ms"].is_none());
    }
    #[test]
    fn layered_scene_preserves_three_overlapping_ordered_quads() {
        let viewport = size(px(512.), px(384.));
        let mut scene = scene(
            workload::quads(
                Method::Layered,
                Bounds::new(point(px(0.), px(0.)), viewport),
                210.,
                128,
            ),
            viewport,
        );
        scene.finish();
        assert_eq!(scene.quads.len(), 3);
        assert_eq!(scene.batches().count(), 1);
        assert_eq!(
            scene.quads.iter().map(|q| q.order).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        let report = distribution(vec![3., 1., 2., 4.]).unwrap();
        assert_eq!(report.median, 2.5);
        assert_eq!(report.p95, 4.);
        assert!(distribution(vec![]).is_none());
    }
}
