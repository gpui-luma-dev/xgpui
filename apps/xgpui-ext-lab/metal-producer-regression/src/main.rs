//! Issue #110: exercise the real renderer without freezing a window or test runner.
use anyhow::{Context, Result, ensure};
use clap::Parser;
use std::{
    process::{Command, ExitCode},
    thread,
    time::{Duration, Instant},
};

#[derive(Parser, Debug)]
#[command(about = "macOS Metal custom-producer deadlock regression (issue #110)")]
struct Options {
    /// Independent visible texture producers in the dense scene.
    #[arg(long, default_value_t = 512, value_parser = clap::value_parser!(u32).range(2..=4096))]
    producers: u32,
    #[arg(long, default_value_t = 3, value_parser = clap::value_parser!(u32).range(1..))]
    frames: u32,
    #[arg(long, default_value_t = 3, value_parser = clap::value_parser!(u32).range(1..))]
    cycles: u32,
    /// Wall-clock limit per child, including renderer startup and GPU readback.
    #[arg(long, default_value_t = 15, value_parser = clap::value_parser!(u64).range(1..))]
    timeout_seconds: u64,
    #[arg(long, hide = true)]
    child: bool,
    #[arg(long, hide = true)]
    control: bool,
}

fn supervise(options: &Options, control: bool) -> Result<()> {
    let mut command = Command::new(std::env::current_exe()?);
    command.args([
        "--child",
        "--producers",
        &options.producers.to_string(),
        "--frames",
        &options.frames.to_string(),
        "--cycles",
        &options.cycles.to_string(),
    ]);
    if control {
        command.arg("--control");
    }
    let label = if control {
        "single-producer control"
    } else {
        "dense producer regression"
    };
    eprintln!("Starting {label}");
    // Inherited output preserves the last preparation marker even if Metal blocks.
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
                "{label} timed out after {}s ({status}); inspect the last preparation/frame marker",
                options.timeout_seconds
            );
        }
        thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(target_os = "macos")]
mod native {
    use super::*;
    use gpui::*;
    use gpui_apple::metal_renderer::MetalHeadlessRenderer;
    use std::{any::Any, sync::Arc};
    use xgpui_gamut::WideColor;

    #[derive(Debug)]
    struct Producer {
        index: u32,
        color: WideColor,
    }
    impl CustomContent for Producer {
        fn prepare(&self, backend: &dyn Any, extent: Size<DevicePixels>) -> Result<Box<dyn Any>> {
            eprintln!("prepare {} begin", self.index);
            let result = self.color.prepare(backend, extent);
            eprintln!(
                "prepare {} end: {}",
                self.index,
                if result.is_ok() { "ok" } else { "error" }
            );
            result
        }
    }

    fn scene(count: u32, green: bool) -> Result<Scene> {
        let mut scene = Scene::default();
        for index in 0..count {
            let bounds = Bounds::new(
                point(ScaledPixels(index as f32), ScaledPixels(0.)),
                size(ScaledPixels(1.), ScaledPixels(8.)),
            );
            scene.insert_primitive(CustomDraw {
                order: 0,
                bounds,
                content_mask: ContentMask { bounds },
                corner_radii: Default::default(),
                opacity: 1.,
                coverage: None,
                content: Arc::new(Producer {
                    index,
                    color: WideColor::linear_p3(if green {
                        [0., 1., 0., 1.]
                    } else {
                        [1., 0., 0., 1.]
                    })?,
                }),
            });
        }
        scene.finish();
        Ok(scene)
    }

    pub fn run(options: &Options) -> Result<()> {
        let mut renderer = MetalHeadlessRenderer::new();
        renderer.set_color_mode(WindowColorMode::WideGamutSdr)?;
        let count = if options.control {
            1
        } else {
            options.producers
        };
        for cycle in 0..options.cycles {
            // Replace dense -> sparse -> dense, like leaving and returning to a page.
            for (stage, count) in [("dense", count), ("sparse", 1), ("return", count)] {
                let green = stage == "sparse";
                let scene = scene(count, green)?;
                let extent = size(DevicePixels(count as i32), DevicePixels(8));
                for frame in 0..options.frames {
                    eprintln!("cycle={cycle} stage={stage} frame={frame} producers={count} begin");
                    // Reuse the same scene across frames; each producer still records fresh work.
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
                    eprintln!("cycle={cycle} stage={stage} frame={frame} readback verified");
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
    supervise(&options, true)?;
    supervise(&options, false)?;
    println!("PASS: control, dense scenes, replay, scene replacement, and readback");
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
