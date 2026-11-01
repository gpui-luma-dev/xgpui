//! Issue #110: exercise the real renderer without freezing a window or test runner.
use anyhow::{Context, Result, ensure};
use clap::Parser;
use std::{
    process::{Command, ExitCode},
    thread,
    time::{Duration, Instant},
};

#[derive(Parser, Debug)]
#[command(about = "macOS portable WideColor regression (issue #110)")]
struct Options {
    /// Visible WideColor draws; --distinct forces separate producer outputs.
    #[arg(long, default_value_t = 512, value_parser = clap::value_parser!(u32).range(2..=4096))]
    producers: u32,
    #[arg(long, default_value_t = 3, value_parser = clap::value_parser!(u32).range(1..))]
    frames: u32,
    #[arg(long, default_value_t = 3, value_parser = clap::value_parser!(u32).range(1..))]
    cycles: u32,
    /// Wall-clock limit per child, including renderer startup and GPU readback.
    #[arg(long, default_value_t = 15, value_parser = clap::value_parser!(u64).range(1..))]
    timeout_seconds: u64,
    /// Give each source a different natural extent to prevent solid deduplication.
    #[arg(long)]
    distinct: bool,
    #[arg(long, hide = true)]
    child: bool,
    #[arg(long, hide = true)]
    control: bool,
}

fn supervise(options: &Options, control: bool, distinct: bool) -> Result<()> {
    let mut command = Command::new(std::env::current_exe()?);
    command.args([
        "--child",
        "--producers",
        &options.producers.to_string(),
        "--frames",
        &options.frames.to_string(),
        "--cycles",
        &options.cycles.to_string(),
        "--timeout-seconds",
        &options.timeout_seconds.to_string(),
    ]);
    if control {
        command.arg("--control");
    }
    if distinct {
        command.arg("--distinct");
    }
    let label = if control {
        "single-producer control"
    } else if distinct {
        "dense distinct-output regression"
    } else {
        "original dense WideColor regression"
    };
    eprintln!("Starting {label}");
    // Inherited output preserves the last submission marker even if Metal blocks.
    let mut child = command.spawn().context("launch regression child")?;
    let deadline = Instant::now() + Duration::from_secs(options.timeout_seconds);
    loop {
        if let Some(status) = child.try_wait()? {
            ensure!(status.success(), "{label} failed: {status}");
            return Ok(());
        }
        if Instant::now() >= deadline {
            child.kill().context("kill timed-out regression child")?;
            let status = child.wait()?;
            anyhow::bail!(
                "{label} timed out after {}s ({status}); inspect the last submission/frame marker",
                options.timeout_seconds
            );
        }
        thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(target_os = "macos")]
mod native {
    use super::*;
    use gpui::content::*;
    use gpui::{
        Bounds, ContentMask, CustomDraw, DevicePixels, PlatformHeadlessRenderer, ScaledPixels,
        Scene, WindowColorMode, point, size,
    };
    use gpui_apple::metal_renderer::MetalHeadlessRenderer;
    use std::sync::{Arc, mpsc};
    use xgpui_gamut::WideColor;

    #[derive(Debug)]
    struct Producer {
        index: u32,
        color: WideColor,
    }
    impl ContentSource for Producer {
        fn describe(
            &self,
            cx: &mut ContentBuilder,
            request: &ContentRequest,
        ) -> std::result::Result<ImageRef, ContentError> {
            // Distinct natural extents retain exactly the original red/green
            // pixels while forcing count separate logical/native outputs.
            let request = if self.index == 0 {
                request.clone()
            } else {
                ContentRequest::new(
                    request.logical_size(),
                    ImageExtent::new(self.index + 1, request.extent().height())?,
                    request.scale_factor(),
                )?
            };
            self.color.describe(cx, &request)
        }
    }

    fn scene(
        count: u32,
        green: bool,
        distinct: bool,
        slots: &[PresentationSlotId],
    ) -> Result<Scene> {
        let mut scene = Scene::default();
        for index in 0..count {
            let bounds = Bounds::new(
                point(ScaledPixels(index as f32), ScaledPixels(0.)),
                size(ScaledPixels(1.), ScaledPixels(8.)),
            );
            let producer = Producer {
                index: if distinct { index } else { 0 },
                color: WideColor::linear_p3(if green {
                    [0., 1., 0., 1.]
                } else {
                    [1., 0., 0., 1.]
                })?,
            };
            let draw = ContentDraw::new(Content::new(producer)).presentation(ContentPresentation {
                slot: Some(slots[index as usize]),
                on_error: ContentErrorPresentation::FailFrame,
                ..Default::default()
            });
            scene.insert_primitive(CustomDraw {
                order: 0,
                bounds,
                content_mask: ContentMask { bounds },
                corner_radii: Default::default(),
                opacity: 1.,
                coverage: None,
                content: draw.into_scene_content(),
            });
        }
        scene.finish();
        Ok(scene)
    }

    pub fn run(options: &Options) -> Result<()> {
        let mut renderer = MetalHeadlessRenderer::new();
        renderer.set_color_mode(WindowColorMode::WideGamutSdr)?;
        let (done, ready) = mpsc::channel();
        renderer.set_content_redraw(Arc::new(move || {
            let _ = done.send(());
        }));
        let count = if options.control {
            1
        } else {
            options.producers
        };
        let slots: Vec<_> = (0..count).map(|_| PresentationSlotId::new()).collect();
        for cycle in 0..options.cycles {
            // Replace dense -> sparse -> dense, like leaving and returning to a page.
            for (stage, count) in [("dense", count), ("sparse", 1), ("return", count)] {
                let green = stage == "sparse";
                let scene = scene(count, green, options.distinct, &slots)?;
                let extent = size(DevicePixels(count as i32), DevicePixels(8));
                eprintln!(
                    "cycle={cycle} stage={stage} producers={count} distinct={} submit",
                    options.distinct
                );
                renderer.render_scene(&scene, extent)?;
                if renderer.content_presentation_stats().pending {
                    ready
                        .recv_timeout(Duration::from_secs(options.timeout_seconds))
                        .context("await requested producer generation")?;
                }
                for frame in 0..options.frames {
                    eprintln!("cycle={cycle} stage={stage} frame={frame} producers={count} begin");
                    // Replay the same immutable scene; successful output must be cached.
                    let image = renderer.render_scene_to_linear_p3(&scene, extent)?;
                    let expected = if green {
                        [0, 0x3c00, 0, 0x3c00]
                    } else {
                        [0x3c00, 0, 0, 0x3c00]
                    };
                    ensure!(
                        image.size == extent && image.pixels.len() == count as usize * 8,
                        "incomplete readback"
                    );
                    ensure!(
                        image.pixels.iter().all(|pixel| *pixel == expected),
                        "unexpected linear-P3 pixels"
                    );
                    let stats = renderer.content_presentation_stats();
                    ensure!(
                        !stats.pending && stats.last_error.is_none(),
                        "requested generation was not ready: {stats:?}"
                    );
                    ensure!(
                        stats.execution.readbacks == 0,
                        "producer path performed CPU readback"
                    );
                    let operations = if options.distinct { count as usize } else { 1 };
                    ensure!(
                        stats.execution.operations == operations,
                        "wrong operation count: {stats:?}"
                    );
                    let requests = stats.requests;
                    let image_again = renderer.render_scene_to_linear_p3(&scene, extent)?;
                    ensure!(image_again.pixels == image.pixels, "replay pixels changed");
                    ensure!(
                        renderer.content_presentation_stats().requests == requests,
                        "replay resubmitted producers"
                    );
                    eprintln!(
                        "cycle={cycle} stage={stage} frame={frame} readback verified; producer_operations={operations} producer_readbacks=0 cached_replay=true max_in_flight={}",
                        stats.execution.max_in_flight
                    );
                }
            }
        }
        Ok(())
    }
}

fn run() -> Result<()> {
    let options = Options::parse();
    ensure!(
        cfg!(target_os = "macos"),
        "this regression requires native macOS Metal"
    );
    if options.child {
        #[cfg(target_os = "macos")]
        return native::run(&options);
    }
    supervise(&options, true, options.distinct)?;
    supervise(&options, false, options.distinct)?;
    if !options.distinct {
        supervise(&options, false, true)?;
    }
    println!(
        "PASS: control, dense WideColor scenes, cached replay, replacement, exact FP16 pixels, and zero producer readback"
    );
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("FAIL: {error:#}");
            ExitCode::FAILURE
        }
    }
}
