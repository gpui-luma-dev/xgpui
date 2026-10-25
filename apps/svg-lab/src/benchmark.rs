//! One isolated process per document/resolution, driven by bench/run.py.
use super::*;
use clap::Parser;
use serde_json::json;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
use xgpui_svg::RasterQueue;

#[derive(Parser)]
#[command(about = "SVG rendering comparison and isolated CPU benchmark")]
pub struct Options {
    /// Run a CPU benchmark without opening a window.
    #[arg(long)]
    pub benchmark: Option<PathBuf>,
    /// Open a complex SVG gallery from a corpus directory.
    #[arg(long, conflicts_with = "benchmark")]
    pub gallery: Option<PathBuf>,
    /// Save a deterministic 36-second native gallery stress run and quit.
    #[arg(long, requires = "gallery")]
    pub gallery_report: Option<PathBuf>,
    #[arg(long, default_value_t = 24)]
    pub gallery_tiles: usize,
    /// Mount every tile instead of only visible rows plus one overscan row.
    #[arg(long)]
    pub gallery_eager: bool,
    /// Shared estimated image-memory pool in MiB (omitted keeps independent queues).
    #[arg(long, requires = "gallery")]
    pub scheduler_mib: Option<usize>,
    #[arg(long, default_value_t = 2)]
    pub scheduler_jobs: usize,
    #[arg(long, default_value_t = 1.)]
    zoom: f32,
    #[arg(long, default_value_t = 2.)]
    density: f32,
    #[arg(long, default_value_t = 5)]
    iterations: usize,
    /// Save a PNG of the final render (outside timed sections).
    #[arg(long)]
    preview: Option<PathBuf>,
    /// Maximum bytes for one RGBA output; filter intermediates are additional.
    #[arg(long, default_value_t = 16)]
    pub budget_mib: usize,
    /// Replay a 120ms rapid-zoom burst through the same queue used by the UI.
    #[arg(long)]
    sequence: bool,
}
fn rss() -> Option<u64> {
    let pid = sysinfo::get_current_pid().ok()?;
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        ProcessRefreshKind::new().with_memory(),
    );
    system.process(pid).map(|p| p.memory())
}
#[cfg(unix)]
fn peak_rss() -> Option<u64> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // getrusage initializes the whole struct on success; macOS reports bytes,
    // Linux reports KiB. This is the kernel's lifetime process high-water mark.
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } != 0 {
        return None;
    }
    let value = unsafe { usage.assume_init() }.ru_maxrss as u64;
    Some(if cfg!(target_os = "macos") {
        value
    } else {
        value * 1024
    })
}
#[cfg(not(unix))]
fn peak_rss() -> Option<u64> {
    None
}
fn stats(values: &[f64]) -> serde_json::Value {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    json!({"samples_ms":values, "median_ms":sorted[sorted.len()/2], "max_ms":sorted[sorted.len()-1]})
}
pub fn run(options: Options) -> Result<()> {
    ensure!(
        options.zoom.is_finite() && (0.25..=16.).contains(&options.zoom),
        "zoom must be 0.25..=16"
    );
    ensure!(
        options.density.is_finite() && (0.5..=4.).contains(&options.density),
        "density must be 0.5..=4"
    );
    ensure!(
        (1..=31).contains(&options.iterations),
        "iterations must be 1..=31"
    );
    ensure!(
        [16, 32, 256].contains(&options.budget_mib),
        "budget must be 16, 32, or 256 MiB (legacy comparison)"
    );
    let policy = RasterPolicy {
        max_output_bytes: options.budget_mib * MIB,
        ..Default::default()
    };
    let path = options
        .benchmark
        .clone()
        .context("benchmark path required")?;
    let start = Instant::now();
    let bytes = std::fs::read(&path)?;
    ensure!(bytes.len() <= 32 * 1024 * 1024, "SVG exceeds input limit");
    let io_ms = start.elapsed().as_secs_f64() * 1000.;
    let rss_start = rss();
    let start = Instant::now();
    let renderer = SvgRenderer::new(Arc::new(()));
    let renderer_init_ms = start.elapsed().as_secs_f64() * 1000.;
    let start = Instant::now();
    let parsed = Arc::new(renderer.parse_svg(&bytes)?);
    let parse_first_ms = start.elapsed().as_secs_f64() * 1000.;
    let rss_parsed = rss();
    let mut parse_samples = Vec::new();
    for _ in 0..options.iterations {
        let start = Instant::now();
        let other = renderer.parse_svg(&bytes)?;
        parse_samples.push(start.elapsed().as_secs_f64() * 1000.);
        std::hint::black_box(&other);
    }
    let intrinsic = parsed.size();
    let logical = display_size(&parsed);
    if options.sequence {
        return sequence(&renderer, parsed, logical, &options, path, policy);
    }
    let resolution = policy.resolution(logical, options.zoom, options.density)?;
    let pixels = resolution.output;
    // Model the live viewer: fixed base + previous zoom + replacement being built.
    let base = raster(&renderer, &parsed, raster_size(logical, 1., 1.))?;
    let rss_base = rss();
    let mut previous = raster(&renderer, &parsed, pixels)?;
    let raster_first_ms = previous.ms;
    let rss_one_zoom = rss();
    let mut raster_samples = Vec::new();
    let mut rss_two_zoom = None;
    for _ in 0..options.iterations {
        let next = raster(&renderer, &parsed, pixels)?;
        raster_samples.push(next.ms);
        let resident = rss();
        rss_two_zoom = rss_two_zoom.into_iter().chain(resident).max();
        previous = next;
    }
    let peak = peak_rss();
    let data = previous.image.as_bytes(0).context("missing image bytes")?;
    let visible_pixels = data.as_chunks::<4>().0.iter().filter(|p| p[3] > 0).count();
    let base_bytes = base.image.as_bytes(0).unwrap().len();
    let output_bytes = data.len();
    if let Some(path) = options.preview {
        let mut rgba = data.to_vec();
        for pixel in rgba.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
        }
        image::save_buffer(
            path,
            &rgba,
            i32::from(pixels.width) as u32,
            i32::from(pixels.height) as u32,
            image::ColorType::Rgba8,
        )?;
    }
    drop(previous);
    drop(base);
    drop(parsed);
    let rss_after_drop = rss();
    println!(
        "{}",
        json!({
            "file":path, "os":std::env::consts::OS, "arch":std::env::consts::ARCH,
            "release":!cfg!(debug_assertions), "input_bytes":bytes.len(), "read_ms":io_ms,
            "renderer_init_ms":renderer_init_ms, "parse_first_ms":parse_first_ms,
            "parse":stats(&parse_samples), "raster_first_ms":raster_first_ms, "raster":stats(&raster_samples),
            "zoom":options.zoom, "density":options.density, "budget_mib":options.budget_mib, "resolution_limited":resolution.limited(),
            "intrinsic_width":intrinsic.width, "intrinsic_height":intrinsic.height,
            "logical_width":logical.width, "logical_height":logical.height,
            "pixel_width":i32::from(pixels.width), "pixel_height":i32::from(pixels.height),
            "visible_pixels":visible_pixels, "output_bytes":output_bytes, "base_bytes":base_bytes,
            "replacement_image_bytes":base_bytes+2*output_bytes,
            "rss_start_bytes":rss_start, "rss_parsed_bytes":rss_parsed, "rss_base_bytes":rss_base,
            "rss_one_zoom_bytes":rss_one_zoom, "rss_two_zoom_bytes":rss_two_zoom,
            "process_peak_rss_bytes":peak, "rss_after_drop_bytes":rss_after_drop
        })
    );
    Ok(())
}

fn sequence(
    renderer: &SvgRenderer,
    parsed: Arc<ParsedSvg>,
    logical: Size<f32>,
    options: &Options,
    path: PathBuf,
    policy: RasterPolicy,
) -> Result<()> {
    use std::{sync::mpsc, thread};
    let base = raster(renderer, &parsed, raster_size(logical, 1., 1.))?;
    let base_bytes = base.image.as_bytes(0).unwrap().len();
    let mut current = base.image.clone();
    let mut queue = RasterQueue::new(policy);
    queue.set_current(0, current.size(0));
    let burst = [
        (0, 1.),
        (20, 2.),
        (40, 4.),
        (60, 8.),
        (80, 16.),
        (100, 4.),
        (120, 16.),
    ];
    let start = Instant::now();
    let mut event = 0;
    let mut jobs = Vec::new();
    let mut first_preview_ms = None;
    let mut final_resolution = None;
    let mut peak_images = base_bytes;
    let (tx, rx) = mpsc::channel();
    loop {
        let now = start.elapsed();
        while event < burst.len() && now >= Duration::from_millis(burst[event].0) {
            let resolution = policy.resolution(logical, burst[event].1, options.density)?;
            queue.request(0, resolution.output, Duration::from_millis(burst[event].0));
            final_resolution = Some(resolution);
            event += 1;
        }
        if let Ok((job, result, started_ms)) = rx.try_recv() {
            let result: Result<Raster> = result;
            let elapsed_ms = start.elapsed().as_secs_f64() * 1000.;
            let ms = result.as_ref().ok().map(|r| r.ms);
            let accepted = queue.complete(job, result.is_ok());
            if let Ok(ref raster) = result {
                peak_images = peak_images.max(
                    base_bytes
                        + current.as_bytes(0).unwrap().len()
                        + raster.image.as_bytes(0).unwrap().len(),
                );
            }
            jobs.push(json!({"stage":format!("{:?}",job.stage), "width":i32::from(job.pixels.width),"height":i32::from(job.pixels.height),"started_ms":started_ms,"completed_ms":elapsed_ms,"raster_ms":ms,"accepted":accepted}));
            if accepted {
                let raster = result?;
                current = raster.image;
                if first_preview_ms.is_none() {
                    first_preview_ms = Some(elapsed_ms);
                }
            }
        }
        if event == burst.len() && queue.ready() && !queue.busy() {
            break;
        }
        if let Some(job) = queue.next(now) {
            let renderer = renderer.clone();
            let parsed = parsed.clone();
            let tx = tx.clone();
            let started_ms = now.as_secs_f64() * 1000.;
            thread::spawn(move || {
                let result = raster(&renderer, &parsed, job.pixels);
                let _ = tx.send((job, result, started_ms));
            });
        }
        ensure!(
            start.elapsed() < Duration::from_secs(60),
            "sequence exceeded 60s"
        );
        thread::sleep(Duration::from_millis(1));
    }
    let final_ms = start.elapsed().as_secs_f64() * 1000.;
    let resolution = final_resolution.unwrap();
    println!(
        "{}",
        json!({"mode":"sequence", "file":path,"release":!cfg!(debug_assertions),"budget_mib":options.budget_mib,
        "density":options.density,"events":burst,"jobs":jobs,"first_preview_ready_ms":first_preview_ms,
        "final_detail_after_last_request_ms":final_ms-120., "total_ms":final_ms,
        "final_width":i32::from(current.size(0).width),"final_height":i32::from(current.size(0).height),
        "final_output_bytes":current.as_bytes(0).unwrap().len(),"resolution_limited":resolution.limited(),
        "requested_width":i32::from(resolution.requested.width),"requested_height":i32::from(resolution.requested.height),
        "peak_known_image_bytes":peak_images,"process_peak_rss_bytes":peak_rss(),"rss_end_bytes":rss()})
    );
    Ok(())
}
