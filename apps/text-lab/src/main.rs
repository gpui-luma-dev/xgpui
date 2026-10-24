//! Native and Parley comparison through the same GPUI application/rendering path.
#[cfg(test)]
mod editing_tests;
mod paragraph;
mod samples;
mod timings;
mod workload;
// Reuse GPUI's example editor; do not maintain a second editing implementation.
#[allow(dead_code)]
#[path = "../../../gpui/crates/gpui/examples/input.rs"]
mod input;
use anyhow::{Result, ensure};
use clap::{Parser, ValueEnum};
use gpui::{prelude::*, *};
use std::{borrow::Cow, process::Command, sync::Arc, time::Instant};
use xgpui_parley::ParleyTextSystem;

#[derive(Clone, Copy, Debug, PartialEq, ValueEnum)]
enum Backend {
    Both,
    Native,
    Parley,
}
#[derive(Clone, Copy, Debug, PartialEq, ValueEnum)]
enum Workload {
    Compare,
    Scroll,
    Paragraph,
}
#[derive(Parser)]
struct Options {
    #[arg(long, value_enum, default_value_t = Backend::Both)]
    backend: Backend,
    #[arg(long, value_enum, default_value_t = Workload::Compare)]
    workload: Workload,
    #[arg(long, default_value_t = 5000)]
    lines: usize,
    /// Start a deterministic scroll/size/weight run; zero leaves manual controls.
    #[arg(long, default_value_t = 0)]
    scroll_frames: usize,
    /// Quit when the scripted run finishes. Both backends run sequentially.
    #[arg(long)]
    exit_after_scroll: bool,
    /// Run CPU diagnostics without opening windows; both backends run sequentially.
    #[arg(long)]
    timings_only: bool,
    #[arg(long, default_value_t = 31)]
    iterations: usize,
    /// Raster diagnostics only; live windows use their actual display scale.
    #[arg(long, default_value_t = 1.0)]
    raster_scale: f32,
}

struct Lab {
    name: String,
    samples: Vec<samples::Sample>,
    timings: Vec<timings::Timing>,
    setup_ms: f64,
    input: Entity<input::TextInput>,
    scroll: Option<Entity<workload::ScrollLab>>,
    paragraph: Option<Entity<paragraph::ParagraphLab>>,
}
impl Render for Lab {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        if let Some(paragraph) = &self.paragraph {
            return div()
                .id("paragraph-lab-root")
                .size_full()
                .child(paragraph.clone());
        }
        if let Some(scroll) = &self.scroll {
            return div()
                .id("scroll-lab-root")
                .size_full()
                .child(scroll.clone());
        }
        let mut root = div()
            .id("text-lab")
            .size_full()
            .overflow_y_scroll()
            .bg(rgb(0x111827))
            .text_color(rgb(0xeff4fc))
            .font_family("IBM Plex Sans")
            .text_size(px(13.0))
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_size(px(24.0))
                    .child(format!("{} text", self.name)),
            )
            .child(format!(
                "Setup {:.2} ms | CPU timings in us, 28px",
                self.setup_ms
            ))
            .child("Cold (first per sample) / warm median. Shared state across rows.")
            .child("Warm layout = GPUI cache hit; raster = direct mask rebuild.")
            .child("Raster diagnostic scale is printed in the terminal. No GPU timing.")
            .child(div().flex_none().flex().flex_col().gap_1()
                .child("Edit: click/drag, Shift+arrows, select all, copy/paste; try IME composition.")
                .child(self.input.clone()));
        for (sample, t) in self.samples.iter().zip(&self.timings) {
            root = root.child(
                div()
                    .p_3()
                    .rounded_md()
                    .bg(rgb(0x1f2937))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(sample.label.clone())
                    .child(
                        div()
                            .text_size(px(28.0))
                            .whitespace_nowrap()
                            .child(StyledText::new(sample.text.clone()).with_runs(sample.runs())),
                    )
                    .child(format!(
                        "Layout {:.1} / {:.2} us | Raster {:.1} / {:.1} us",
                        t.layout_first, t.layout_warm, t.raster_first, t.raster_warm
                    ))
                    .child(format!(
                        "Width {:.2}px | {} resolved face(s) | {} unique glyphs",
                        t.width, t.faces, t.glyphs
                    )),
            );
        }
        root
    }
}

fn main() -> Result<()> {
    let mut options = Options::parse();
    if options.workload == Workload::Paragraph {
        ensure!(
            options.backend != Backend::Native,
            "native backends do not implement the optional paragraph hook; use --backend parley"
        );
        options.backend = Backend::Parley; // Both defaults to the available paragraph provider.
    }
    ensure!(
        (1..=1000).contains(&options.iterations),
        "iterations must be 1..=1000"
    );
    ensure!(
        options.raster_scale.is_finite()
            && options.raster_scale > 0.0
            && options.raster_scale <= 4.0,
        "raster scale must be in (0,4]"
    );
    ensure!(
        (100..=100_000).contains(&options.lines),
        "lines must be 100..=100000"
    );
    ensure!(
        options.scroll_frames == 0 || (4..=100_000).contains(&options.scroll_frames),
        "scroll frames must be zero or 4..=100000"
    );
    ensure!(
        !options.exit_after_scroll
            || (options.workload == Workload::Scroll
                && options.scroll_frames > 0
                && !options.timings_only),
        "exit-after-scroll needs --workload scroll --scroll-frames N and a window"
    );
    if options.backend == Backend::Both {
        return launch_pair(&options);
    }
    let start = Instant::now();
    let platform = gpui_platform::current_platform(options.timings_only);
    let backend: Arc<dyn PlatformTextSystem> = match options.backend {
        Backend::Native => platform.text_system(),
        Backend::Parley => {
            let mut fonts = parley::FontContext::new();
            for data in [samples::INTER, samples::PLEX] {
                fonts
                    .collection
                    .register_fonts(parley::fontique::Blob::new(Arc::new(data.to_vec())), None);
            }
            if options.workload == Workload::Paragraph {
                for data in paragraph::FONTS {
                    fonts
                        .collection
                        .register_fonts(parley::fontique::Blob::new(Arc::new(data.to_vec())), None);
                }
            }
            Arc::new(ParleyTextSystem::new(fonts, "IBM Plex Sans")?)
        }
        Backend::Both => unreachable!(),
    };
    let app = Application::with_platform_and_text_system(platform, backend.clone());
    if options.backend == Backend::Native {
        app.text_system().add_fonts(vec![
            Cow::Borrowed(samples::INTER),
            Cow::Borrowed(samples::PLEX),
        ])?;
    }
    let setup_ms = start.elapsed().as_secs_f64() * 1000.0;
    let paragraph_timings = if options.workload == Workload::Paragraph {
        println!(
            "Parley paragraphs: {} / {} | release={} | setup={setup_ms:.2}ms",
            std::env::consts::OS,
            std::env::consts::ARCH,
            !cfg!(debug_assertions)
        );
        Some(paragraph::measure(app.text_system(), options.iterations)?)
    } else {
        None
    };
    let samples = samples::samples()?;
    let name = format!("{:?}", options.backend);
    let lines = if options.workload == Workload::Scroll {
        workload::corpus(options.lines)
    } else {
        Vec::new()
    };
    if options.workload == Workload::Scroll && options.timings_only {
        println!(
            "{name}: {} / {} | release={} | setup={setup_ms:.2}ms",
            std::env::consts::OS,
            std::env::consts::ARCH,
            !cfg!(debug_assertions)
        );
        print!(
            "{}",
            workload::measure(app.text_system(), &lines, &samples[2].font)?
        );
        return Ok(());
    }
    let timings = if options.workload == Workload::Compare {
        timings::measure(
            app.text_system(),
            &*backend,
            &samples,
            options.iterations,
            options.raster_scale,
        )?
    } else {
        Vec::new()
    };
    let mut report = format!(
        "\n{name}: {} / {} | release={} | setup={setup_ms:.2}ms\n28px @ {}x, {} warm iterations (median). First per row, shared backend state.\nLayout: GPUI cache hits. Raster: direct CPU rebuild, no atlas cache. Not frame/GPU timings.\n",
        std::env::consts::OS,
        std::env::consts::ARCH,
        !cfg!(debug_assertions),
        options.raster_scale,
        options.iterations
    );
    for (sample, t) in samples.iter().zip(&timings) {
        report.push_str(&format!("{:44} layout {:8.1}/{:6.2}us raster {:8.1}/{:8.1}us width {:7.2}px faces {} glyphs {}\n",
            sample.label, t.layout_first, t.layout_warm, t.raster_first, t.raster_warm, t.width, t.faces, t.glyphs));
    }
    if options.workload == Workload::Compare {
        print!("{report}");
    } else if options.workload == Workload::Scroll {
        println!(
            "{name} scroll: {} lines, release={}, setup={setup_ms:.2}ms; UI profiling, no corpus pre-shaping",
            lines.len(),
            !cfg!(debug_assertions)
        );
    }
    if options.timings_only {
        return Ok(());
    }
    app.run(move |cx| {
        input::bind_keys(cx);
        let display = cx.primary_display().expect("display").bounds();
        let width = (display.size.width / 2.0 - px(24.0)).min(px(640.0));
        let height = px(1200.0);
        let x = display.origin.x
            + px(16.0)
            + if options.backend == Backend::Parley {
                width + px(16.0)
            } else {
                px(0.0)
            };
        cx.open_window(
            WindowOptions {
                inactive_frame_interval: if options.workload == Workload::Scroll {
                    None
                } else {
                    WindowOptions::default().inactive_frame_interval
                },
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(x, display.origin.y + px(45.0)),
                    size(width, height),
                ))),
                ..Default::default()
            },
            |window, cx| {
                window.set_window_title(&format!("Text Lab - {name}"));
                let input = cx.new(|cx| input::TextInput::new("office café e\u{301} Ångström", cx));
                let paragraph = paragraph_timings
                    .map(|timings| cx.new(|cx| paragraph::ParagraphLab::new(timings, cx)));
                if let Some(paragraph) = &paragraph {
                    window.focus(&paragraph.focus_handle(cx), cx);
                }
                let scroll = if options.workload == Workload::Scroll {
                    Some(cx.new(|_| {
                        workload::ScrollLab::new(
                            name.clone(),
                            lines,
                            samples[2].font.clone(),
                            options.scroll_frames,
                            options.exit_after_scroll,
                        )
                    }))
                } else {
                    if paragraph.is_none() {
                        window.focus(&input.focus_handle(cx), cx);
                    }
                    None
                };
                cx.new(|_| Lab {
                    input,
                    scroll,
                    paragraph,
                    name,
                    samples,
                    timings,
                    setup_ms,
                })
            },
        )
        .expect("open text lab window");
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

fn launch_pair(options: &Options) -> Result<()> {
    let executable = std::env::current_exe()?;
    let command = |backend: &str| {
        let mut cmd = Command::new(&executable);
        cmd.args([
            "--backend",
            backend,
            "--iterations",
            &options.iterations.to_string(),
            "--raster-scale",
            &options.raster_scale.to_string(),
            "--workload",
            match options.workload {
                Workload::Compare => "compare",
                Workload::Scroll => "scroll",
                Workload::Paragraph => "paragraph",
            },
            "--lines",
            &options.lines.to_string(),
            "--scroll-frames",
            &options.scroll_frames.to_string(),
        ]);
        if options.exit_after_scroll {
            cmd.arg("--exit-after-scroll");
        }
        if options.timings_only {
            cmd.arg("--timings-only");
        }
        cmd
    };
    if options.timings_only || options.exit_after_scroll {
        for backend in ["native", "parley"] {
            ensure!(
                command(backend).status()?.success(),
                "{backend} diagnostics failed"
            );
        }
    } else {
        let mut native = command("native").spawn()?;
        let mut parley = match command("parley").spawn() {
            Ok(child) => child,
            Err(error) => {
                native.kill().ok();
                native.wait().ok();
                return Err(error.into());
            }
        };
        let first = native.wait()?;
        if !first.success() {
            parley.kill().ok();
        }
        let second = parley.wait()?;
        ensure!(first.success() && second.success(), "text lab child failed");
    }
    Ok(())
}
