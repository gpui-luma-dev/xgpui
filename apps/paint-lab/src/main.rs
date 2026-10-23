use anyhow::{Result, ensure};
use clap::{Parser, ValueEnum};
use gpui::{prelude::*, *};
use std::{sync::Arc, time::Instant};
use xgpui_shader::{Gradient, GradientKind, GradientStop, MeshGradient, SolidColor};
mod checks;

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum Effect {
    Mesh,
    Linear,
    Radial,
    Conic,
    Diamond,
}
impl Effect {
    fn label(self) -> &'static str {
        match self {
            Self::Mesh => "Mesh",
            Self::Linear => "Linear",
            Self::Radial => "Radial",
            Self::Conic => "Conic",
            Self::Diamond => "Diamond",
        }
    }
    fn kind(self, time: f32) -> Option<GradientKind> {
        match self {
            Self::Mesh => None,
            Self::Linear => Some(GradientKind::Linear {
                angle: 90. + time * 20.,
            }),
            Self::Radial => Some(GradientKind::Radial {
                center: [0.5; 2],
                radius: [0.5 + 0.12 * time.sin(); 2],
            }),
            Self::Conic => Some(GradientKind::Conic {
                center: [0.5; 2],
                angle: time * 20.,
            }),
            Self::Diamond => Some(GradientKind::Diamond {
                center: [0.5; 2],
                radius: [0.5; 2],
                angle: time * 20.,
            }),
        }
    }
}
fn gradient_stops() -> Vec<GradientStop> {
    vec![
        GradientStop::new(0., [1., 0.12, 0.18, 1.]),
        GradientStop::new(0.25, [1., 0.7, 0.12, 1.]),
        GradientStop::new(0.5, [0.25, 0.25, 1., 1.]),
        GradientStop::new(0.75, [0.05, 0.85, 0.75, 1.]),
        GradientStop::new(1., [1., 0.12, 0.18, 1.]),
    ]
}
#[derive(Parser)]
#[command(about = "Extension paint coverage and Metal integration lab")]
struct Options {
    #[arg(long)]
    headless: bool,
    #[arg(long, value_enum, default_value_t=Effect::Radial)]
    effect: Effect,
    #[arg(long, requires = "headless")]
    png: Option<std::path::PathBuf>,
    #[arg(long, default_value_t = 60)]
    frames: usize,
}
fn grid(t: f32) -> Result<xgpui_shader::MeshGrid> {
    let colors = vec![
        [1., 0.15, 0.12],
        [1., 0.65, 0.1],
        [0.3, 0.3, 1.],
        [0.95, 0.2, 0.6],
        [0.5, 0.8, 0.8],
        [0.1, 0.6, 1.],
        [0.45, 0.1, 0.8],
        [0.1, 0.9, 0.6],
        [0.1, 0.3, 0.8],
    ];
    let base = xgpui_shader::MeshGrid::regular(3, 3, colors)?;
    let mut points = base.points().to_vec();
    points[4].position = [0.5 + 0.22 * t.sin(), 0.5 + 0.20 * (t * 0.7).sin()];
    xgpui_shader::MeshGrid::new(3, 3, points)
}
fn shape(bounds: Bounds<Pixels>, stroke: bool) -> Path<Pixels> {
    let mut p = if stroke {
        PathBuilder::stroke(px(12.))
    } else {
        PathBuilder::fill()
    };
    let w = bounds.size.width;
    let h = bounds.size.height;
    let o = bounds.origin;
    p.move_to(o + point(w * 0.5, h * 0.0));
    p.line_to(o + point(w * 0.67, h * 0.33));
    p.curve_to(o + point(w * 1.0, h * 0.5), o + point(w * 1.0, h * 0.1));
    p.line_to(o + point(w * 0.72, h * 0.68));
    p.line_to(o + point(w * 0.8, h * 1.0));
    p.line_to(o + point(w * 0.5, h * 0.8));
    p.line_to(o + point(w * 0.2, h * 1.0));
    p.line_to(o + point(w * 0.28, h * 0.68));
    p.line_to(o + point(w * 0.0, h * 0.5));
    p.line_to(o + point(w * 0.33, h * 0.33));
    p.close();
    p.build().expect("bounded path")
}
struct Lab {
    mesh: Arc<MeshGradient>,
    analytic: Arc<Gradient>,
    effect: Effect,
    native: bool,
    faded: bool,
    clipped: bool,
    playing: bool,
    time: f32,
    updated: Instant,
}
impl Render for Lab {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        if self.playing {
            self.time += now.duration_since(self.updated).as_secs_f32();
            if self.effect == Effect::Mesh {
                self.mesh = Arc::new(self.mesh.with_grid(grid(self.time).unwrap()).unwrap());
            }
            window.request_animation_frame();
        }
        self.updated = now;
        if let Some(kind) = self.effect.kind(self.time)
            && self.analytic.kind() != kind
        {
            self.analytic = Arc::new(self.analytic.with_kind(kind).unwrap());
        }
        let paint = if self.native {
            Paint::Native(white().into())
        } else if self.effect == Effect::Mesh {
            self.mesh.clone().into()
        } else {
            self.analytic.clone().into()
        };
        let button = |id, text: &str| {
            div()
                .id(id)
                .px_3()
                .py_2()
                .rounded_md()
                .bg(rgb(0x343a45))
                .cursor_pointer()
                .child(text.to_owned())
        };
        let mut cards = div().flex().gap_4();
        for (index, label) in ["Rounded background", "Dashed border", "Curved path fill"]
            .into_iter()
            .enumerate()
        {
            let p = paint.clone();
            let mut tile = div().relative().w(px(320.)).h(px(240.));
            match index {
                0 => {
                    tile = tile
                        .rounded_3xl()
                        .bg_paint(p)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child("Normal GPUI children");
                }
                1 => {
                    tile = tile
                        .rounded_3xl()
                        .border_8()
                        .border_dashed()
                        .border_paint(p)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child("One continuous paint field");
                }
                _ => {
                    tile = tile.child(
                        canvas(
                            |_, _, _| (),
                            move |bounds, _, window, _| {
                                window.paint_path_with_paint(shape(bounds, false), p.clone())
                            },
                        )
                        .size_full(),
                    );
                }
            }
            let mut clip = div()
                .id(ElementId::Integer(index as u64))
                .w(px(if self.clipped { 230. } else { 340. }))
                .h(px(264.))
                .p_3()
                .overflow_hidden();
            clip = clip.child(tile);
            cards = cards.child(div().flex().flex_col().gap_3().child(label).child(clip));
        }
        let mut choices = div().flex().gap_3();
        for (i, effect) in [
            Effect::Mesh,
            Effect::Linear,
            Effect::Radial,
            Effect::Conic,
            Effect::Diamond,
        ]
        .into_iter()
        .enumerate()
        {
            choices = choices.child(
                button(ElementId::Integer(100 + i as u64), effect.label())
                    .bg(rgb(if self.effect == effect {
                        0x375a88
                    } else {
                        0x343a45
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.effect = effect;
                        this.native = false;
                        cx.notify();
                    })),
            );
        }
        div().size_full().flex().flex_col().gap_5().p_6().bg(rgb(0x151922)).text_color(rgb(0xffffff))
            .child(div().text_2xl().child("Extension paint lab"))
            .child(format!("{} paint · one shared producer · GPUI owns the shape coverage",self.effect.label()))
            .child(choices)
            .child(div().flex().gap_3()
                .child(button("play".into(), if self.playing {"Pause"} else {"Animate"}).on_click(cx.listener(|this,_,_,cx| { this.updated=Instant::now(); this.playing = !this.playing; cx.notify(); })))
                .child(button("native".into(), if self.native {"Native white coverage"} else {"Gradient paint"}).on_click(cx.listener(|this,_,_,cx| { this.native = !this.native; cx.notify(); })))
                .child(button("opacity".into(), if self.faded {"Opacity: 50%"} else {"Opacity: 100%"}).on_click(cx.listener(|this,_,_,cx| { this.faded = !this.faded; cx.notify(); })))
                .child(button("clip".into(), if self.clipped {"Clip: on"} else {"Clip: off"}).on_click(cx.listener(|this,_,_,cx| { this.clipped = !this.clipped; cx.notify(); }))))
            .child(div().id("examples").overflow_x_scroll().opacity(if self.faded {0.5} else {1.}).child(cards))
            .child(div().text_sm().text_color(rgb(0xaab4c5)).child("Clipping reveals the same field; it does not rescale it. All three shapes share the source texture."))
            .child(div().text_sm().text_color(rgb(0xaab4c5)).child("Metal first · backgrounds, borders and paths · shadows and text follow later"))
    }
}
fn main() -> Result<()> {
    let options = Options::parse();
    ensure!(
        (1..=10000).contains(&options.frames),
        "frames must be 1..=10000"
    );
    if options.headless {
        return checks::run(options.effect, options.frames, options.png.as_deref());
    }
    gpui_platform::application().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1200.), px(700.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                window.set_window_title("XGPUI Paint Lab");
                cx.new(|_| Lab {
                    mesh: Arc::new(MeshGradient::new(grid(0.).unwrap(), 18).unwrap()),
                    analytic: Arc::new(
                        Gradient::new(
                            options
                                .effect
                                .kind(0.)
                                .unwrap_or(GradientKind::Linear { angle: 90. }),
                            gradient_stops(),
                        )
                        .unwrap(),
                    ),
                    effect: options.effect,
                    native: false,
                    faded: false,
                    clipped: false,
                    playing: false,
                    time: 0.,
                    updated: Instant::now(),
                })
            },
        )
        .expect("open paint lab");
        cx.activate(true);
    });
    Ok(())
}
