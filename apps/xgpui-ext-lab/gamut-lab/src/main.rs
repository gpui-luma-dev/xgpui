//! Wide SDR content surfaces and native sRGB controls.
use anyhow::{Result, ensure};
use clap::Parser;
use gpui::{prelude::*, *};
use std::{cell::RefCell, rc::Rc, sync::Arc};
use xgpui_shader::{SolidColor, painted};
mod checks;
use xgpui_gamut::{
    WideColor,
    color::{ColorInterpolation, GamutMapping, LinearRgb, LinearRgba, RgbSpace},
    instanced::{WideGradient, WidePainter},
};

#[derive(Parser)]
#[command(about = "Wide-surface / sRGB-control compositing lab (Metal / Direct3D 11)")]
struct Options {
    #[arg(long)]
    headless: bool,
    /// Draw wide surfaces through the shared main-pass shader instead of textures.
    #[arg(long)]
    instanced: bool,
    /// Use two-stop OkLab gradients on the instanced path.
    #[arg(long, requires = "instanced")]
    gradient: bool,
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
    painter: Option<WidePainter>,
    gradient: bool,
}
impl Lab {
    fn source(&self, color: [f64; 4]) -> Arc<dyn CustomContent> {
        if self.wide {
            Arc::new(WideColor::linear_p3(color.map(|v| v as f32)).unwrap())
        } else {
            Arc::new(SolidColor::new(color.map(|v| v as f32)).unwrap())
        }
    }
    fn surface(&self, x: f32, y: f32, w: f32, h: f32, color: [f64; 4]) -> AnyElement {
        if self.wide {
            if let Some(painter) = self.painter.clone() {
                let gradient = if self.gradient {
                    let first = LinearRgba::new(
                        LinearRgb::new(RgbSpace::DisplayP3, [color[0], color[1], color[2]])
                            .unwrap(),
                        color[3],
                    )
                    .unwrap();
                    let second = LinearRgba::new(
                        LinearRgb::new(RgbSpace::DisplayP3, [color[2], color[0], color[1]])
                            .unwrap(),
                        color[3],
                    )
                    .unwrap();
                    Some(
                        WideGradient::linear(
                            [0., 0.],
                            [1., 0.],
                            [first, second],
                            ColorInterpolation::Oklab,
                            GamutMapping::ReduceChroma,
                        )
                        .unwrap(),
                    )
                } else {
                    None
                };
                let solid = WideColor::linear_p3(color.map(|v| v as f32)).unwrap();
                return div()
                    .absolute()
                    .left(px(x))
                    .top(px(y))
                    .w(px(w))
                    .h(px(h))
                    .opacity(self.opacity)
                    .child(
                        canvas(
                            |_, _, _| (),
                            move |bounds, _, window, _| {
                                let result = if let Some(gradient) = &gradient {
                                    painter.paint_gradient(window, bounds, px(16.), gradient)
                                } else {
                                    painter.paint_color(window, bounds, px(16.), &solid)
                                };
                                result.expect("instanced wide fill");
                            },
                        )
                        .size_full(),
                    )
                    .into_any_element();
            }
        }
        painted()
            .absolute()
            .left(px(x))
            .top(px(y))
            .w(px(w))
            .h(px(h))
            .rounded_2xl()
            .opacity(self.opacity)
            .bg_paint(self.source(color))
            .into_any_element()
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mode = if self.wide {
            if self.painter.is_some() {
                if self.gradient {
                    "Instanced OkLab gradients · Float16 output"
                } else {
                    "Instanced P3 fills · Float16 output"
                }
            } else {
                "Offscreen P3 surfaces · Float16 output"
            }
        } else {
            "sRGB baseline"
        };
        let mut canvas = div()
            .relative()
            .w(px(960.))
            .h(px(640.))
            .overflow_hidden()
            .bg(rgb(if self.dark { 0x101010 } else { 0xf0f0f0 }));
        let red = self.surface(70., 70., 560., 400., [1., 0., 0., 1.]);
        let green = self.surface(390., 260., 470., 300., [0., 1., 0., 1.]);
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
        let second = if self.reverse {
            self.surface(70., 70., 560., 400., [1., 0., 0., 1.])
        } else {
            self.surface(390., 260., 470., 300., [0., 1., 0., 1.])
        };
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
        let status = window.color_status();
        let presentation = div()
            .id("presentation")
            .px_3()
            .py_2()
            .rounded_md()
            .bg(rgb(0x374151))
            .hover(|s| s.bg(rgb(0x4b5563)))
            .on_click(cx.listener(|this, _, window, cx| {
                let mode = if this.wide {
                    WindowColorMode::Default
                } else {
                    WindowColorMode::WideGamutSdr
                };
                match window.set_color_mode(mode) {
                    Ok(_) => {
                        this.wide = !this.wide;
                        cx.notify();
                    }
                    Err(error) => eprintln!("Presentation request failed: {error:#}"),
                }
            }))
            .child(if self.wide {
                "Switch to sRGB"
            } else {
                "Switch to P3"
            });
        let controls = div()
            .flex()
            .gap_3()
            .child(presentation)
            .child(self.button(
                "overlay",
                format!("Overlays: {}", self.overlay),
                |s| s.overlay = !s.overlay,
                cx,
            ))
            .child(self.button(
                "opacity",
                format!("Opacity: {:.2}", self.opacity),
                |s| s.opacity = if s.opacity > 0.9 { 0.5 } else { 1. },
                cx,
            ))
            .child(self.button(
                "background",
                format!("Dark: {}", self.dark),
                |s| s.dark = !s.dark,
                cx,
            ))
            .child(self.button(
                "order",
                "Reverse surfaces".into(),
                |s| s.reverse = !s.reverse,
                cx,
            ));
        div().size_full().flex().flex_col().gap_4().p_6()
            .bg(rgb(0x202025)).text_color(white())
            .child(div().text_xl().child("Gamut Lab — wide SDR composition"))
            .child(div().child(mode))
            .child(div().text_sm().child(format!("Requested: {:?} · Effective: {:?} · Display wide gamut: {:?}", status.requested(), status.effective(), status.display_wide_gamut())))
            .child(controls)
            .child(canvas)
            .child(div().text_sm().child("Controls remain sRGB. Overlays use linear composition at surface boundaries. HDR is off."))
    }
}
fn main() -> Result<()> {
    let options = Options::parse();
    ensure!(
        (1..=10000).contains(&options.frames),
        "frames must be 1..=10000"
    );
    let painter = if options.instanced {
        Some(WidePainter::new()?)
    } else {
        None
    };
    let completion = Rc::new(RefCell::new(None));
    let capture_result = completion.clone();
    let headless = options.headless;
    gpui_platform::application().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1100.), px(920.)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    show: !options.headless,
                    focus: !options.headless,
                    ..Default::default()
                },
                |window, cx| {
                    let mode = if options.sdr {
                        WindowColorMode::Default
                    } else {
                        WindowColorMode::WideGamutSdr
                    };
                    window
                        .set_color_mode(mode)
                        .expect("color presentation negotiation");
                    window.set_window_title("XGPUI Gamut Lab — wide SDR");
                    cx.new(|_| Lab {
                        wide: !options.sdr,
                        overlay: true,
                        opacity: 1.,
                        dark: true,
                        reverse: false,
                        painter: painter.clone(),
                        gradient: options.gradient,
                    })
                },
            )
            .unwrap();
        if options.headless || options.ui_png.is_some() {
            let path = options.png.or(options.ui_png);
            let frames = if headless { options.frames } else { 1 };
            cx.defer(move |cx| {
                cx.update_window(window.into(), |_, window, cx| {
                    checks::capture(window, cx, frames, path, headless, capture_result);
                })
                .expect("capture window");
            });
        }
        if !headless {
            cx.activate(true);
        }
    });
    if headless {
        completion.borrow_mut().take().unwrap_or_else(|| {
            Err(anyhow::anyhow!(
                "application exited before headless capture completed"
            ))
        })?;
    }
    Ok(())
}
