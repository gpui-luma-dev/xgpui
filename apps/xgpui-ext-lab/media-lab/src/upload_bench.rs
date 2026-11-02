//! Portable source readiness and cached color-change CPU measurements.
//! Schema 2 replaces producer staging counters; these are not GPU timestamps.
use anyhow::{Context as _, Result, bail, ensure};
use gpui::*;
use image::ImageEncoder as _;
use serde::Serialize;
use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
use xgpui_media::{ImageFilter, LoadStatus, MediaImage, MediaLimits, MediaLoader};
const MIB: usize = 1024 * 1024;
const SOURCES: usize = 6;
const SOURCE_SIDE: u32 = 1024;
const WAVES: usize = 3;
const CACHED_FRAMES: usize = 120;
const FRAME_INTERVAL: Duration = Duration::from_nanos(16_666_667);
const COLORS: [[u8; 3]; SOURCES] = [
    [220, 60, 40],
    [48, 192, 64],
    [48, 96, 224],
    [224, 184, 48],
    [176, 64, 208],
    [48, 192, 192],
];
#[derive(Serialize)]
struct Summary {
    samples: usize,
    mean_ms: f64,
    p50_ms: f64,
    p95_ms: f64,
    p99_ms: f64,
    max_ms: f64,
    cpu_calls_over_16_67_ms: usize,
}
impl Summary {
    fn new(values: &[f64]) -> Result<Self> {
        ensure!(
            !values.is_empty() && values.iter().all(|v| v.is_finite() && *v >= 0.),
            "invalid benchmark samples"
        );
        let mut sorted = values.to_vec();
        sorted.sort_by(f64::total_cmp);
        let rank = |p: f64| sorted[((sorted.len() as f64 * p).ceil() as usize).saturating_sub(1)];
        Ok(Self {
            samples: values.len(),
            mean_ms: values.iter().sum::<f64>() / values.len() as f64,
            p50_ms: rank(0.5),
            p95_ms: rank(0.95),
            p99_ms: rank(0.99),
            max_ms: *sorted.last().unwrap(),
            cpu_calls_over_16_67_ms: values
                .iter()
                .filter(|v| **v > FRAME_INTERVAL.as_secs_f64() * 1000.)
                .count(),
        })
    }
}
fn rect(x: f32, y: f32, w: f32, h: f32) -> Bounds<ScaledPixels> {
    Bounds::new(
        point(ScaledPixels(x), ScaledPixels(y)),
        size(ScaledPixels(w), ScaledPixels(h)),
    )
}
fn scene(images: &[MediaImage], filter: ImageFilter) -> Scene {
    let mut scene = Scene::default();
    let bounds = rect(0., 0., 900., 600.);
    scene.insert_primitive(Quad {
        bounds,
        content_mask: ContentMask { bounds },
        background: rgb(0x111619).into(),
        ..Default::default()
    });
    for (index, image) in images.iter().enumerate() {
        let x = (index % 3) as f32 * 300.;
        let y = (index / 3) as f32 * 300.;
        let content: Arc<dyn CustomContent> = image
            .filtered(filter)
            .draw()
            .expect("RGBA8 benchmark filter")
            .into_scene_content();
        for bounds in [
            rect(x + 12., y + 12., 256., 256.),
            rect(x + 224., y + 224., 64., 64.),
        ] {
            scene.insert_primitive(CustomDraw {
                order: 0,
                bounds,
                content_mask: ContentMask { bounds },
                corner_radii: Corners::all(ScaledPixels(12.)),
                opacity: 1.,
                coverage: None,
                content: content.clone(),
            });
        }
    }
    scene.finish();
    scene
}
fn fixture(path: &Path, dimension: u32, color: [u8; 3], seed: u32) -> Result<()> {
    let mut rgba = image::RgbaImage::new(dimension, dimension);
    let mut random = seed;
    for (x, y, pixel) in rgba.enumerate_pixels_mut() {
        random ^= random << 13;
        random ^= random >> 17;
        random ^= random << 5;
        let rgb = if x >= dimension / 4
            && x < dimension * 3 / 4
            && y >= dimension / 4
            && y < dimension * 3 / 4
        {
            color
        } else {
            [
                (random & 255) as u8,
                ((random >> 8) & 255) as u8,
                ((random >> 16) & 255) as u8,
            ]
        };
        *pixel = image::Rgba([rgb[0], rgb[1], rgb[2], 255]);
    }
    image::codecs::png::PngEncoder::new_with_quality(
        std::fs::File::create(path)?,
        image::codecs::png::CompressionType::Fast,
        image::codecs::png::FilterType::NoFilter,
    )
    .write_image(
        rgba.as_raw(),
        dimension,
        dimension,
        image::ExtendedColorType::Rgba8,
    )?;
    Ok(())
}
fn decode(loader: &MediaLoader, path: &Path) -> Result<(MediaImage, f64)> {
    let start = Instant::now();
    let mut job = loader.load(path)?;
    loop {
        if let Some(image) = job.try_take() {
            return Ok((image, start.elapsed().as_secs_f64() * 1000.));
        }
        if let LoadStatus::Failed(error) = job.status() {
            bail!(error);
        }
        ensure!(
            start.elapsed() < Duration::from_secs(60),
            "benchmark decode timeout"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[derive(Serialize)]
struct FrameSample {
    snapshot_cpu_ms: f64,
    encode_submit_cpu_ms: f64,
    pending: bool,
}
#[derive(Serialize)]
struct Wave {
    index: usize,
    prepare_ready_wall_ms: f64,
    frames: Vec<FrameSample>,
}
#[derive(Serialize)]
struct Report {
    schema_version: u32,
    profile: &'static str,
    os: &'static str,
    architecture: &'static str,
    backend: &'static str,
    source_count: usize,
    source_dimensions: [u32; 2],
    placements_per_source: usize,
    retained_mip_payload_bytes: usize,
    decode_and_mip_wall_ms: Vec<f64>,
    prepare_snapshot_cpu: Summary,
    cold_encode_submit_cpu: Summary,
    cached_color_change_cpu: Summary,
    waves: Vec<Wave>,
    cached_frames: Vec<FrameSample>,
    validated_capture: String,
    caveats: Vec<&'static str>,
}
fn new_renderer(portable: bool) -> Result<Box<dyn PlatformHeadlessRenderer>> {
    if portable {
        #[cfg(feature = "wgpu")]
        {
            return Ok(Box::new(gpui_wgpu::WgpuHeadlessRenderer::new()?));
        }
        #[cfg(not(feature = "wgpu"))]
        bail!("--wgpu requires --features wgpu");
    }
    gpui_platform::current_headless_renderer()?.context("native headless renderer unavailable")
}
fn draw(renderer: &mut dyn PlatformHeadlessRenderer, scene: &Scene) -> Result<FrameSample> {
    let start = Instant::now();
    let pending = match renderer.prepare_content_snapshot(scene) {
        Ok(()) => false,
        Err(error)
            if error
                .downcast_ref::<content::ContentError>()
                .is_some_and(|e| e.kind == content::ContentErrorKind::Pending) =>
        {
            true
        }
        Err(error) => return Err(error),
    };
    let snapshot_cpu_ms = start.elapsed().as_secs_f64() * 1000.;
    let start = Instant::now();
    renderer.render_scene(scene, size(DevicePixels(900), DevicePixels(600)))?;
    Ok(FrameSample {
        snapshot_cpu_ms,
        encode_submit_cpu_ms: start.elapsed().as_secs_f64() * 1000.,
        pending,
    })
}
fn ready(renderer: &mut dyn PlatformHeadlessRenderer, scene: &Scene) -> Result<Vec<FrameSample>> {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut samples = Vec::new();
    loop {
        let sample = draw(renderer, scene)?;
        let pending = sample.pending;
        samples.push(sample);
        if !pending {
            return Ok(samples);
        }
        ensure!(
            Instant::now() < deadline,
            "portable source readiness timeout"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn validate(image: &image::RgbaImage) -> Result<()> {
    for (index, color) in COLORS.iter().enumerate() {
        let x = (index % 3) as u32 * 300;
        let y = (index / 3) as u32 * 300;
        for (x, y) in [(x + 140, y + 140), (x + 256, y + 256)] {
            let pixel = image.get_pixel(x, y).0;
            ensure!(
                pixel[..3]
                    .iter()
                    .zip(color)
                    .all(|(a, b)| a.abs_diff(*b) <= 2)
                    && pixel[3] == 255,
                "capture mismatch for source {index}: {pixel:?}"
            );
        }
    }
    Ok(())
}
pub fn run(report_path: &Path, png: &Path, portable: bool) -> Result<()> {
    ensure!(report_path != png, "JSON and PNG outputs must differ");
    ensure!(
        !cfg!(debug_assertions),
        "upload benchmark requires --release"
    );
    let loader = MediaLoader::new(MediaLimits {
        max_jobs: 2,
        max_resident_bytes: 256 * MIB,
        max_image_bytes: 32 * MIB,
        max_encoded_bytes: 32 * MIB,
    })?;
    let directory = tempfile::Builder::new()
        .prefix("xgpui-portable-upload-bench-")
        .tempdir()?;
    let warm_path = directory.path().join("warm.png");
    fixture(&warm_path, 256, COLORS[0], 1)?;
    let (warm, _) = decode(&loader, &warm_path)?;
    let warm_scene = scene(std::slice::from_ref(&warm), ImageFilter::new());
    let mut images = Vec::new();
    let mut decode_ms = Vec::new();
    for (index, color) in COLORS.into_iter().enumerate() {
        let path = directory.path().join(format!("source-{index}.png"));
        fixture(&path, SOURCE_SIDE, color, index as u32 + 17)?;
        let (image, millis) = decode(&loader, &path)?;
        images.push(image);
        decode_ms.push(millis);
    }
    let base = scene(&images, ImageFilter::new());
    let adjusted = scene(&images, ImageFilter::new().brightness(1.1).saturation(0.8));
    let mut waves = Vec::new();
    let mut cached = Vec::new();
    for index in 0..WAVES {
        let mut renderer = new_renderer(portable)?;
        ready(&mut *renderer, &warm_scene)?;
        renderer.render_scene_to_image(&warm_scene, size(DevicePixels(900), DevicePixels(600)))?;
        let start = Instant::now();
        let frames = ready(&mut *renderer, &base)?;
        let prepare_ready_wall_ms = start.elapsed().as_secs_f64() * 1000.;
        let capture =
            renderer.render_scene_to_image(&base, size(DevicePixels(900), DevicePixels(600)))?;
        validate(&capture)?;
        println!(
            "Portable source wave {}: {} polls, {:.2} ms ready wall time; pixels verified",
            index + 1,
            frames.len(),
            prepare_ready_wall_ms
        );
        waves.push(Wave {
            index,
            prepare_ready_wall_ms,
            frames,
        });
        if index + 1 == WAVES {
            for frame in 0..CACHED_FRAMES {
                let tick = Instant::now();
                let sample = draw(
                    &mut *renderer,
                    if frame % 2 == 0 { &base } else { &adjusted },
                )?;
                ensure!(
                    !sample.pending,
                    "color changes unexpectedly restarted source realization"
                );
                cached.push(sample);
                std::thread::sleep(FRAME_INTERVAL.saturating_sub(tick.elapsed()));
            }
            let capture = renderer
                .render_scene_to_image(&base, size(DevicePixels(900), DevicePixels(600)))?;
            validate(&capture)?;
            capture.save(png)?;
        }
    }
    let cold: Vec<f64> = waves
        .iter()
        .flat_map(|w| w.frames.iter().map(|f| f.encode_submit_cpu_ms))
        .collect();
    let snapshot: Vec<f64> = waves
        .iter()
        .flat_map(|w| w.frames.iter().map(|f| f.snapshot_cpu_ms))
        .collect();
    let hot: Vec<f64> = cached
        .iter()
        .map(|f| f.snapshot_cpu_ms + f.encode_submit_cpu_ms)
        .collect();
    let report = Report {
        schema_version: 2,
        profile: "release",
        os: std::env::consts::OS,
        architecture: std::env::consts::ARCH,
        backend: if portable { "wgpu" } else { "native" },
        source_count: SOURCES,
        source_dimensions: [SOURCE_SIDE, SOURCE_SIDE],
        placements_per_source: 2,
        retained_mip_payload_bytes: images.iter().map(|i| i.payload_bytes()).sum(),
        decode_and_mip_wall_ms: decode_ms,
        prepare_snapshot_cpu: Summary::new(&snapshot)?,
        cold_encode_submit_cpu: Summary::new(&cold)?,
        cached_color_change_cpu: Summary::new(&hot)?,
        waves,
        cached_frames: cached,
        validated_capture: png.display().to_string(),
        caveats: vec![
            "CPU wall measurements, not GPU timestamps, UI frame rate, or input latency.",
            "Each wave uses a fresh window renderer after pipeline/target warmup. Device and driver caches may remain warm.",
            "Readiness wall time includes asynchronous execution, host polling and 1 ms sleeps; it is not isolated upload time.",
            "Cached frames alternate encoded filters on shared sources; final readback is outside timed samples.",
            "Schema 2 removes producer preparation counters and loader staging-pool assertions. Renderer content-executor defaults govern GPU admission.",
        ],
    };
    std::fs::write(report_path, serde_json::to_vec_pretty(&report)?)?;
    println!(
        "Saved portable source benchmark to {} and {}",
        report_path.display(),
        png.display()
    );
    Ok(())
}
