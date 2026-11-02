//! Bounded headless comparison. Encoding/submission is timed without a readback;
//! a separate validation render/readback drains work before the next sample.
use super::workload::{Path, Workload, shader_batches};
use anyhow::{Context, Result, ensure};
use clap::Parser;
use gpui::{
    DevicePixels, PlatformHeadlessRenderer,
    content::{ContentError, ContentErrorKind},
    size,
};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Parser)]
#[command(
    about = "Instanced companion to portable-content. UI by default; --benchmark compares both complete rendering paths."
)]
pub struct Options {
    #[arg(long)]
    pub benchmark: bool,
    #[arg(long, default_value_t = 60)]
    pub frames: usize,
    #[arg(long, default_value_t = 10)]
    pub warmup: usize,
    #[arg(long, default_value_t = 1024)]
    pub width: u32,
    #[arg(long, default_value_t = 512)]
    pub height: u32,
    #[arg(long, requires = "benchmark")]
    pub json: Option<PathBuf>,
}
impl Options {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (1..=1000).contains(&self.frames) && self.warmup <= 100,
            "frames must be 1..=1000; warmup at most 100"
        );
        ensure!(
            (320..=4096).contains(&self.width) && (160..=4096).contains(&self.height),
            "benchmark width must be 320..=4096; height 160..=4096"
        );
        Ok(())
    }
}
#[derive(Clone, Copy)]
enum Case {
    Replay512,
    Replay1,
    Replace512,
    Density,
}
impl Case {
    fn name(self) -> &'static str {
        match self {
            Self::Replay512 => "replay-512",
            Self::Replay1 => "replay-1",
            Self::Replace512 => "replace-512",
            Self::Density => "density-512-1",
        }
    }
    fn state(self, frame: usize) -> (bool, bool) {
        match self {
            Self::Replay512 => (true, false),
            Self::Replay1 => (false, false),
            Self::Replace512 => (true, frame % 2 == 1),
            Self::Density => (frame % 2 == 0, false),
        }
    }
}
#[derive(Serialize)]
struct Distribution {
    median: f64,
    p95: f64,
    min: f64,
    max: f64,
}
fn distribution(mut values: Vec<f64>) -> Distribution {
    values.sort_by(f64::total_cmp);
    let n = values.len();
    Distribution {
        median: (values[(n - 1) / 2] + values[n / 2]) / 2.,
        p95: values[(n * 95).div_ceil(100) - 1],
        min: values[0],
        max: values[n - 1],
    }
}
#[derive(Serialize)]
struct Row {
    path: Path,
    workload: &'static str,
    frames: usize,
    min_requested_rectangles: usize,
    max_requested_rectangles: usize,
    max_shader_instances: usize,
    max_shader_batches: usize,
    scene_build_us: Distribution,
    prepare_ready_us: Distribution,
    cpu_encode_submit_us: Distribution,
    validation_render_complete_readback_us: Distribution,
    max_pixel_difference_from_portable: Option<u8>,
}
#[derive(Serialize)]
struct Report {
    schema_version: u32,
    app: &'static str,
    source_revision: Option<String>,
    os: &'static str,
    arch: &'static str,
    optimized_build: bool,
    width: u32,
    height: u32,
    scale_factor: f32,
    warmup: usize,
    measurement_notes: [&'static str; 4],
    rows: Vec<Row>,
}
fn prepare(renderer: &mut dyn PlatformHeadlessRenderer, scene: &gpui::Scene) -> Result<()> {
    let start = Instant::now();
    loop {
        match renderer.prepare_content_snapshot(scene) {
            Ok(()) => return Ok(()),
            Err(error)
                if error
                    .downcast_ref::<ContentError>()
                    .is_some_and(|e| e.kind == ContentErrorKind::Pending) =>
            {
                ensure!(
                    start.elapsed() < Duration::from_secs(20),
                    "content readiness timed out"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(error) => return Err(error),
        }
    }
}
fn micros(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1e6
}
pub fn run(options: &Options) -> Result<()> {
    let workload = Workload::new()?;
    let mut references = BTreeMap::new();
    let mut rows = Vec::new();
    for path in [Path::PortableGroup, Path::Instanced] {
        let mut renderer = gpui_platform::current_headless_renderer()?
            .context("No platform headless renderer available")?;
        for case in [
            Case::Replay512,
            Case::Replay1,
            Case::Replace512,
            Case::Density,
        ] {
            let mut build = Vec::new();
            let mut ready = Vec::new();
            let mut encode = Vec::new();
            let mut validation = Vec::new();
            let mut min_count = usize::MAX;
            let mut max_count = 0;
            let mut max_instances = 0;
            let mut max_batches = 0;
            let mut last_image = None;
            for frame in 0..options.warmup + options.frames {
                let (dense, cool) = case.state(frame);
                let start = Instant::now();
                let scene = workload.scene(path, options.width, options.height, dense, cool);
                let built = micros(start);
                let batches = shader_batches(&scene);
                if matches!(path, Path::Instanced) {
                    ensure!(
                        batches == 1,
                        "instanced workload split into {batches} shader batches"
                    );
                    ensure!(
                        scene.shader_draws.len() == if dense { 512 } else { 1 },
                        "missing shader instances"
                    );
                }
                let start = Instant::now();
                prepare(renderer.as_mut(), &scene)?;
                let prepared = micros(start);
                let target = size(
                    DevicePixels(options.width as i32),
                    DevicePixels(options.height as i32),
                );
                let start = Instant::now();
                renderer.render_scene(&scene, target)?;
                let encoded = micros(start);
                // This second render/readback is outside the encoding timer. It
                // bounds queue depth and proves that each sample actually renders.
                let start = Instant::now();
                let image = renderer.render_scene_to_image(&scene, target)?;
                let validated = micros(start);
                ensure!(
                    image.width() == options.width && image.height() == options.height,
                    "invalid validation image dimensions"
                );
                // Validate an interior pixel every iteration so a blank image or
                // stale palette cannot accidentally count as a successful sample.
                let columns = if dense { 32. } else { 1. };
                let rows = if dense { 16. } else { 1. };
                let x = ((options.width as f32 / columns - 2.) * 0.5).floor() as u32;
                let y = ((options.height as f32 / rows - 2.) * 0.5).floor() as u32;
                let expected: [u8; 4] = if cool {
                    [26, 0, 255, 255]
                } else {
                    [255, 0, 26, 255]
                };
                ensure!(
                    image
                        .get_pixel(x, y)
                        .0
                        .into_iter()
                        .zip(expected)
                        .all(|(a, b)| a.abs_diff(b) <= 1),
                    "missing/stale first-cell pixels in {} ({path:?})",
                    case.name()
                );
                if frame >= options.warmup {
                    build.push(built);
                    ready.push(prepared);
                    encode.push(encoded);
                    validation.push(validated);
                    min_count = min_count.min(if dense { 512 } else { 1 });
                    max_count = max_count.max(if dense { 512 } else { 1 });
                    max_instances = max_instances.max(scene.shader_draws.len());
                    max_batches = max_batches.max(batches);
                }
                last_image = Some(image);
            }
            let image = last_image.unwrap();
            let difference = match path {
                Path::PortableGroup => {
                    references.insert(case.name(), image);
                    None
                }
                Path::Instanced => {
                    let reference = &references[case.name()];
                    let max = image
                        .as_raw()
                        .iter()
                        .zip(reference.as_raw())
                        .map(|(a, b)| a.abs_diff(*b))
                        .max()
                        .unwrap_or(0);
                    ensure!(
                        max <= 2,
                        "{} differs from portable reference by {max} channels (allowed 2)",
                        case.name()
                    );
                    Some(max)
                }
            };
            rows.push(Row {
                path,
                workload: case.name(),
                frames: options.frames,
                min_requested_rectangles: min_count,
                max_requested_rectangles: max_count,
                max_shader_instances: max_instances,
                max_shader_batches: max_batches,
                scene_build_us: distribution(build),
                prepare_ready_us: distribution(ready),
                cpu_encode_submit_us: distribution(encode),
                validation_render_complete_readback_us: distribution(validation),
                max_pixel_difference_from_portable: difference,
            });
        }
    }
    let source_revision = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned());
    let report = Report {
        schema_version: 1,
        app: "portable-content-instanced",
        source_revision,
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        optimized_build: !cfg!(debug_assertions),
        width: options.width,
        height: options.height,
        scale_factor: 1.,
        warmup: options.warmup,
        measurement_notes: [
            "Two complete rendering paths: independent solid/copy producers in an atomic group versus one main-pass instanced shader batch. Not an isolated draw-call microbenchmark.",
            "CPU encode/submit excludes scene construction, readiness polling and pixel readback. It is not GPU duration, FPS or click-to-display latency.",
            "Readiness includes CPU description, queued producer work and 1ms polling; pipeline warmup is excluded from measured samples when warmup > 0.",
            "Validation time includes a second render, GPU completion and readback; it drains the preceding timed submission. Final images must match within 2 channel levels.",
        ],
        rows,
    };
    let json = serde_json::to_string_pretty(&report)?;
    if let Some(path) = &options.json {
        fs::write(path, &json).with_context(|| format!("write {}", path.display()))?;
    }
    println!("{json}");
    Ok(())
}
