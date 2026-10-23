//! macOS-only private composition experiment; no new GPUI API.
use anyhow::{Result, ensure};
use clap::Parser;
use gpui::{prelude::*, *};
use std::{sync::Arc, time::Instant};
use xgpui_shader::{SolidColor, painted};
mod checks;
mod content;

#[derive(Parser)]
#[command(about = "Private Metal wide-surface / sRGB-control compositing lab (macOS)")]
struct Options {
    #[arg(long)]
    headless: bool,
    /// Run the unchanged native sRGB renderer and sRGB paint producers.
    #[arg(long)]
    sdr: bool,
    /// Explicit clipped sRGB preview, not a wide-gamut export.
    #[arg(long, requires = "headless")]
    png: Option<std::path::PathBuf>,
    #[arg(long, default_value_t = 60)]
    frames: usize,
    /// Capture the actual control UI as an explicit clipped sRGB preview.
    #[arg(long, conflicts_with = "headless")]
    ui_png: Option<std::path::PathBuf>,
}
struct Lab {
    wide: bool,
    overlay: bool,
    opacity: f32,
    dark: bool,
    reverse: bool,
}
impl Lab {
    fn source(&self, color: [f64; 4]) -> Arc<dyn CustomContent> {
        if self.wide {
            Arc::new(content::WideColor(color))
        } else {
            Arc::new(SolidColor::new(color.map(|v| v as f32)).unwrap())
        }
    }
    fn button(
        &self,
        id: &'static str,
        label: String,
        action: fn(&mut Lab),
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .id(id)
            .px_3()
            .py_2()
            .rounded_md()
            .bg(rgb(0x374151))
            .hover(|s| s.bg(rgb(0x4b5563)))
            .on_click(cx.listener(move |this, _, _, cx| {
                action(this);
                cx.notify();
            }))
            .child(label)
    }
}
impl Render for Lab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mode = if self.wide {
            "Linear P3 surfaces · Float16 output"
        } else {
            "sRGB baseline"
        };
        let mut canvas = div()
            .relative()
            .w(px(960.))
            .h(px(640.))
            .overflow_hidden()
            .bg(rgb(if self.dark { 0x101010 } else { 0xf0f0f0 }));
        let red = painted()
            .absolute()
            .left(px(70.))
            .top(px(70.))
            .w(px(560.))
            .h(px(400.))
            .rounded_2xl()
            .opacity(self.opacity)
            .bg_paint(self.source([1., 0., 0., 1.]));
        let green = painted()
            .absolute()
            .left(px(390.))
            .top(px(260.))
            .w(px(470.))
            .h(px(300.))
            .rounded_2xl()
            .opacity(self.opacity)
            .bg_paint(self.source([0., 1., 0., 1.]));
        let mid = div()
            .absolute()
            .left(px(210.))
            .top(px(190.))
            .w(px(520.))
            .h(px(120.))
            .p_4()
            .bg(rgba(0xffffff80))
            .text_color(black())
            .child("sRGB overlay between surfaces");
        if self.reverse {
            canvas = canvas.child(green);
        } else {
            canvas = canvas.child(red);
        }
        if self.overlay {
            canvas = canvas.child(mid);
        }
        let second = painted()
            .absolute()
            .left(px(if self.reverse { 70. } else { 390. }))
            .top(px(if self.reverse { 70. } else { 260. }))
            .w(px(if self.reverse { 560. } else { 470. }))
            .h(px(if self.reverse { 400. } else { 300. }))
            .rounded_2xl()
            .opacity(self.opacity)
            .bg_paint(self.source(if self.reverse {
                [1., 0., 0., 1.]
            } else {
                [0., 1., 0., 1.]
            }));
        canvas = canvas.child(second);
        if self.overlay {
            canvas = canvas.child(
                div()
                    .absolute()
                    .left(px(510.))
                    .top(px(360.))
                    .px_4()
                    .py_3()
                    .rounded_lg()
                    .bg(rgba(0x172554d0))
                    .text_color(white())
                    .child("Native sRGB tooltip above both surfaces"),
            );
        }
        div().size_full().flex().flex_col().gap_4().p_6().bg(rgb(0x202025)).text_color(white())
            .child(div().text_xl().child("Gamut Lab — private Metal prototype"))
            .child(div().child(mode))
            .child(div().flex().gap_3()
                .child(self.button("overlay",format!("Overlays: {}",self.overlay),|s|s.overlay= !s.overlay,cx))
                .child(self.button("opacity",format!("Surface opacity: {:.2}",self.opacity),|s|s.opacity=if s.opacity>0.9{0.5}else{1.},cx))
                .child(self.button("background",format!("Dark backdrop: {}",self.dark),|s|s.dark= !s.dark,cx))
                .child(self.button("order","Reverse surfaces".into(),|s|s.reverse= !s.reverse,cx)))
            .child(canvas)
            .child(div().text_sm().child("Controls remain sRGB. P3 red/green content; overlays use linear source-over at surface boundaries. HDR is off."))
    }
}
fn main() -> Result<()> {
    let options = Options::parse();
    ensure!(
        (1..=10000).contains(&options.frames),
        "frames must be 1..=10000"
    );
    ensure!(
        cfg!(debug_assertions),
        "gamut-lab requires a debug build; the private host experiment is absent in release"
    );
    // Re-exec before threads/GPUI initialize rather than changing process env unsafely.
    let active = std::env::var_os("XGPUI_GAMUT_LAB").is_some_and(|v| v == "1");
    if active == options.sdr {
        let mut command = std::process::Command::new(std::env::current_exe()?);
        command.args(std::env::args_os().skip(1));
        if options.sdr {
            command.env_remove("XGPUI_GAMUT_LAB");
        } else {
            command.env("XGPUI_GAMUT_LAB", "1");
        }
        let status = command.status()?;
        ensure!(status.success(), "gamut-lab child failed: {status}");
        return Ok(());
    }
    if options.headless {
        let start = Instant::now();
        checks::run(options.frames, options.png.as_deref(), !options.sdr)?;
        println!(
            "{} frames in {:?} (CPU encode + final readback; not GPU timing)",
            options.frames,
            start.elapsed()
        );
        return Ok(());
    }
    gpui_platform::application().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1100.), px(920.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                window.set_window_title("XGPUI Gamut Lab — private prototype");
                if let Some(path) = options.ui_png.clone() {
                    window.on_next_frame(move |window, _| {
                        window.on_next_frame(move |window, _| {
                            match window
                                .render_to_image()
                                .and_then(|image| image.save(&path).map_err(Into::into))
                            {
                                Ok(()) => println!("Control UI sRGB preview: {}", path.display()),
                                Err(error) => eprintln!("UI capture failed: {error:#}"),
                            }
                        });
                    });
                }
                cx.new(|_| Lab {
                    wide: !options.sdr,
                    overlay: true,
                    opacity: 1.,
                    dark: true,
                    reverse: false,
                })
            },
        )
        .unwrap();
        cx.activate(true);
    });
    Ok(())
}
