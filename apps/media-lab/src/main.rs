mod fixtures;
mod headless;
mod model;
mod paths;
mod scroll_perf;
mod ui;
mod ui_perf;
mod upload_bench;
mod video_lab;

use anyhow::Result;
use clap::Parser;
use gpui::*;
use std::path::PathBuf;

#[derive(Parser)]
#[command(about = "Visual atlas/mipmap comparisons and bounded media upload diagnostics")]
struct Options {
    /// Add a PNG/JPEG/WebP source to the generated offline samples.
    #[arg(long)]
    image: Option<PathBuf>,
    /// Start with a vivid color adjustment on the media panel.
    #[arg(long)]
    vivid: bool,
    /// Open contour trim and dash examples (or capture them headlessly).
    #[arg(long, conflicts_with = "video")]
    paths: bool,
    /// Open synthetic native video presentation (macOS Metal).
    #[arg(long)]
    video: bool,
    /// Measure bounded upload CPU work versus cached-source rendering in release.
    #[arg(long, requires = "headless", conflicts_with_all = ["video", "paths", "image", "vivid"])]
    upload_bench: bool,
    #[arg(long, requires = "upload_bench")]
    bench_report: Option<PathBuf>,
    /// Record a visible release-window baseline, upload burst and cached cadence.
    #[arg(long, conflicts_with_all = ["headless", "paths", "video", "image"])]
    ui_perf_report: Option<PathBuf>,
    /// Run a sustained large-image scrolling workload; test at fixed 60 Hz.
    #[arg(long, conflicts_with_all = ["headless", "paths", "video", "image", "ui_perf_report", "vivid"])]
    scroll_perf_report: Option<PathBuf>,
    /// Render a repeatable atlas-versus-media contact sheet without a window.
    #[arg(long)]
    headless: bool,
    /// Use wgpu for the headless capture (requires feature wgpu).
    #[arg(long, requires = "headless")]
    wgpu: bool,
    #[arg(long, requires = "headless")]
    png: Option<PathBuf>,
}
fn main() -> Result<()> {
    let options = Options::parse();
    if let Some(path) = options.scroll_perf_report {
        return scroll_perf::run(path);
    }
    if options.upload_bench {
        return upload_bench::run(
            options
                .bench_report
                .as_deref()
                .unwrap_or(std::path::Path::new("/tmp/media-upload-bench.json")),
            options
                .png
                .as_deref()
                .unwrap_or(std::path::Path::new("/tmp/media-upload-bench.png")),
            options.wgpu,
        );
    }
    if options.headless && options.video {
        return video_lab::capture(
            options
                .png
                .as_deref()
                .unwrap_or(std::path::Path::new("/tmp/video-lab.png")),
            options.wgpu,
        );
    }
    if options.headless && options.paths {
        return paths::capture(
            options
                .png
                .as_deref()
                .unwrap_or(std::path::Path::new("/tmp/path-effects.png")),
            options.wgpu,
        );
    }
    let fixtures = fixtures::Fixtures::new(options.image)?;
    if options.headless {
        return headless::run(
            &fixtures,
            options
                .png
                .as_deref()
                .unwrap_or(std::path::Path::new("/tmp/media-lab.png")),
            options.wgpu,
            options.vivid,
        );
    }
    let mut lab = model::Lab::new(fixtures)?;
    if let Some(path) = options.ui_perf_report {
        lab.ui_perf = Some(ui_perf::Run::new(path, &lab)?);
        lab.sweep = true;
    }
    lab.path_mode = options.paths;
    lab.video_mode = options.video;
    if options.vivid {
        lab.filters = [2, 2, 2, 0, 0];
    }
    gpui_platform::application().run(move |cx: &mut App| {
        cx.open_window(
            WindowOptions {
                window_min_size: Some(size(px(1200.), px(800.))),
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1400.), px(980.)),
                    cx,
                ))),
                ..Default::default()
            },
            |window, cx| {
                window.set_window_title(if cfg!(debug_assertions) {
                    "XGPUI Media Lab — DEBUG"
                } else {
                    "XGPUI Media Lab — RELEASE"
                });
                cx.new(|_| lab)
            },
        )
        .expect("open media lab");
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.activate(true);
    });
    Ok(())
}
