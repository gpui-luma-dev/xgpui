use anyhow::{Result, ensure};
use clap::{Parser, ValueEnum};
use gpui::{prelude::*, *};
use std::{collections::HashMap, sync::Arc, time::Instant};
use xgpui_shader::{Gradient, GradientKind, GradientStop, MeshGradient, SolidColor};
use xgpui_shader::{Paint, WindowPaintExt, painted};
#[cfg(feature = "portable-shaders")]
mod brushes;
mod checks;
mod editor;
#[cfg(feature = "portable-shaders")]
mod hermite_editor;
#[cfg(feature = "portable-shaders")]
mod portable;
use editor::Settings;

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
    /// Use the portable wgpu renderer for headless checks (requires feature wgpu).
    #[arg(long, requires = "headless")]
    wgpu: bool,
    /// Render the experimental user WGSL paint (requires feature portable-shaders).
    #[arg(long, requires = "headless")]
    portable_shader: bool,
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Control {
    Offset,
    Red,
    Green,
    Blue,
    Alpha,
    Angle,
    CenterX,
    CenterY,
    RadiusX,
    RadiusY,
    MeshX,
    MeshY,
    MeshUAngle,
    MeshULength,
    MeshVAngle,
    MeshVLength,
    #[cfg(feature = "portable-shaders")]
    HermiteX,
    #[cfg(feature = "portable-shaders")]
    HermiteY,
    #[cfg(feature = "portable-shaders")]
    HermiteUAngle,
    #[cfg(feature = "portable-shaders")]
    HermiteULength,
    #[cfg(feature = "portable-shaders")]
    HermiteVAngle,
    #[cfg(feature = "portable-shaders")]
    HermiteVLength,
}
impl Control {
    fn range(self) -> (f32, f32, f32) {
        match self {
            #[cfg(feature = "portable-shaders")]
            Self::HermiteUAngle | Self::HermiteVAngle => (0., 360., 1.),
            #[cfg(feature = "portable-shaders")]
            Self::HermiteULength | Self::HermiteVLength => (0., 2., 0.01),
            Self::Angle => (0., 360., 1.),
            Self::MeshUAngle | Self::MeshVAngle => (0., 360., 5.),
            Self::MeshULength | Self::MeshVLength => (0., 2., 0.05),
            Self::RadiusX | Self::RadiusY => (0.01, 2., 0.01),
            Self::Red | Self::Green | Self::Blue => (0., 1., 1. / 255.),
            _ => (0., 1., 0.01),
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Offset => "Stop position",
            Self::Red => "Red",
            Self::Green => "Green",
            Self::Blue => "Blue",
            Self::Alpha => "Alpha",
            Self::Angle => "Angle",
            Self::CenterX => "Center X",
            Self::CenterY => "Center Y",
            Self::RadiusX => "Radius X",
            Self::RadiusY => "Radius Y",
            Self::MeshX => "Point X",
            Self::MeshY => "Point Y",
            Self::MeshUAngle => "U angle",
            Self::MeshULength => "U length",
            Self::MeshVAngle => "V angle",
            Self::MeshVLength => "V length",
            #[cfg(feature = "portable-shaders")]
            Self::HermiteX => "Point X",
            #[cfg(feature = "portable-shaders")]
            Self::HermiteY => "Point Y",
            #[cfg(feature = "portable-shaders")]
            Self::HermiteUAngle => "U angle",
            #[cfg(feature = "portable-shaders")]
            Self::HermiteULength => "U length",
            #[cfg(feature = "portable-shaders")]
            Self::HermiteVAngle => "V angle",
            #[cfg(feature = "portable-shaders")]
            Self::HermiteVLength => "V length",
        }
    }
}
struct Lab {
    mesh: Arc<MeshGradient>,
    analytic: Arc<Gradient>,
    mesh_points: Vec<xgpui_shader::ControlPoint>,
    mesh_selected: usize,
    mesh_tangents: Vec<[editor::MeshTangent; 2]>,
    editors: Vec<Settings>,
    slider_bounds: HashMap<Control, Bounds<Pixels>>,
    dragging: Option<Control>,
    effect: Effect,
    native: bool,
    faded: bool,
    clipped: bool,
    light_background: bool,
    #[cfg(feature = "portable-shaders")]
    backdrop_changed: bool,
    #[cfg(feature = "portable-shaders")]
    backdrop_moved: bool,
    #[cfg(feature = "portable-shaders")]
    backdrop_disabled: bool,
    #[cfg(feature = "portable-shaders")]
    layer_filter_step: u8,
    #[cfg(feature = "portable-shaders")]
    freeform_step: u8,
    #[cfg(feature = "portable-shaders")]
    hermite: hermite_editor::HermiteEditor,
    playing: bool,
    time: f32,
    updated: Instant,
}
fn button(id: impl Into<ElementId>, text: impl Into<SharedString>) -> Stateful<Div> {
    div()
        .id(id)
        .px_3()
        .py_2()
        .rounded_md()
        .bg(rgb(0x343a45))
        .text_color(white())
        .cursor_pointer()
        .child(text.into())
}
impl Lab {
    fn settings(&self) -> &Settings {
        &self.editors[self.effect as usize]
    }
    fn settings_mut(&mut self) -> &mut Settings {
        &mut self.editors[self.effect as usize]
    }
    fn displayed_mesh(&self) -> Vec<xgpui_shader::ControlPoint> {
        let mut points = self.mesh_points.clone();
        points[4].position = [
            (points[4].position[0] + 0.22 * self.time.sin()).clamp(0., 1.),
            (points[4].position[1] + 0.20 * (self.time * 0.7).sin()).clamp(0., 1.),
        ];
        points
    }
    fn freeze(&mut self) {
        if self.time == 0. {
            self.playing = false;
            self.updated = Instant::now();
            return;
        }
        if self.effect == Effect::Mesh {
            self.mesh_points = self.displayed_mesh();
        } else {
            let effect = self.effect;
            let time = self.time;
            self.settings_mut().freeze(effect, time);
        }
        self.time = 0.;
        self.playing = false;
        self.updated = Instant::now();
    }
    fn color(&self) -> [f32; 4] {
        if self.effect == Effect::Mesh {
            let c = self.mesh_points[self.mesh_selected].color;
            [c[0], c[1], c[2], 1.]
        } else {
            self.settings().stops[self.settings().selected].color
        }
    }
    fn value(&self, control: Control) -> f32 {
        let s = self.settings();
        match control {
            #[cfg(feature = "portable-shaders")]
            Control::HermiteX
            | Control::HermiteY
            | Control::HermiteUAngle
            | Control::HermiteULength
            | Control::HermiteVAngle
            | Control::HermiteVLength => self.hermite.value(control),
            Control::Offset => s.stops[s.selected].offset,
            Control::Red => self.color()[0],
            Control::Green => self.color()[1],
            Control::Blue => self.color()[2],
            Control::Alpha => self.color()[3],
            Control::Angle => {
                if !self.playing {
                    s.angle
                } else {
                    match s.kind(self.effect, self.time) {
                        Some(
                            GradientKind::Linear { angle }
                            | GradientKind::Conic { angle, .. }
                            | GradientKind::Diamond { angle, .. },
                        ) => angle,
                        _ => s.angle,
                    }
                }
            }
            Control::CenterX => s.center[0],
            Control::CenterY => s.center[1],
            Control::RadiusX => match s.kind(self.effect, self.time) {
                Some(GradientKind::Radial { radius, .. }) => radius[0],
                _ => s.radius[0],
            },
            Control::RadiusY => match s.kind(self.effect, self.time) {
                Some(GradientKind::Radial { radius, .. }) => radius[1],
                _ => s.radius[1],
            },
            Control::MeshX => self.displayed_mesh()[self.mesh_selected].position[0],
            Control::MeshY => self.displayed_mesh()[self.mesh_selected].position[1],
            Control::MeshUAngle => self.mesh_tangents[self.mesh_selected][0].angle,
            Control::MeshULength => self.mesh_tangents[self.mesh_selected][0].length,
            Control::MeshVAngle => self.mesh_tangents[self.mesh_selected][1].angle,
            Control::MeshVLength => self.mesh_tangents[self.mesh_selected][1].length,
        }
    }
    fn set_value(&mut self, control: Control, value: f32) {
        self.freeze();
        self.native = false;
        let (min, max, _) = control.range();
        let value = value.clamp(min, max);
        match control {
            #[cfg(feature = "portable-shaders")]
            Control::HermiteX
            | Control::HermiteY
            | Control::HermiteUAngle
            | Control::HermiteULength
            | Control::HermiteVAngle
            | Control::HermiteVLength => {
                self.hermite.set(control, value);
                self.publish_hermite_edit();
            }
            Control::Offset => self.settings_mut().move_stop(value),
            Control::Red | Control::Green | Control::Blue | Control::Alpha => {
                let channel = match control {
                    Control::Red => 0,
                    Control::Green => 1,
                    Control::Blue => 2,
                    _ => 3,
                };
                if self.effect == Effect::Mesh {
                    if channel < 3 {
                        self.mesh_points[self.mesh_selected].color[channel] = value;
                    }
                } else {
                    let selected = self.settings().selected;
                    self.settings_mut().stops[selected].color[channel] = value;
                }
            }
            Control::Angle => self.settings_mut().angle = value,
            Control::CenterX => self.settings_mut().center[0] = value,
            Control::CenterY => self.settings_mut().center[1] = value,
            Control::RadiusX => self.settings_mut().radius[0] = value,
            Control::RadiusY => self.settings_mut().radius[1] = value,
            Control::MeshX => self.mesh_points[self.mesh_selected].position[0] = value,
            Control::MeshY => self.mesh_points[self.mesh_selected].position[1] = value,
            Control::MeshUAngle
            | Control::MeshULength
            | Control::MeshVAngle
            | Control::MeshVLength => {
                let axis = usize::from(matches!(
                    control,
                    Control::MeshVAngle | Control::MeshVLength
                ));
                let tangent = &mut self.mesh_tangents[self.mesh_selected][axis];
                if matches!(control, Control::MeshUAngle | Control::MeshVAngle) {
                    tangent.angle = value;
                } else {
                    tangent.length = value;
                }
                let point = &mut self.mesh_points[self.mesh_selected];
                if axis == 0 {
                    point.u_tangent = tangent.vector();
                } else {
                    point.v_tangent = tangent.vector();
                }
            }
        }
    }
    #[cfg(feature = "portable-shaders")]
    fn publish_hermite_edit(&mut self) {
        if self.hermite.side == 3 {
            self.mesh_points = self.hermite.legacy_points();
            self.mesh_tangents = self.hermite.tangents.clone();
            self.mesh_selected = self.hermite.selected;
        }
    }
    fn scrub(&mut self, control: Control, x: Pixels) {
        if let Some(bounds) = self.slider_bounds.get(&control)
            && bounds.size.width > px(0.)
        {
            let (min, max, _) = control.range();
            self.set_value(
                control,
                min + ((x - bounds.origin.x) / bounds.size.width).clamp(0., 1.) * (max - min),
            );
        }
    }
    fn slider(&self, control: Control, cx: &mut Context<Self>) -> impl IntoElement {
        let (min, max, step) = control.range();
        let value = self.value(control);
        let fraction = ((value - min) / (max - min)).clamp(0., 1.);
        let display = match control {
            #[cfg(feature = "portable-shaders")]
            Control::HermiteUAngle | Control::HermiteVAngle => format!("{value:.1}°"),
            Control::Angle | Control::MeshUAngle | Control::MeshVAngle => format!("{value:.1}°"),
            Control::Red | Control::Green | Control::Blue => format!("{:.0}", value * 255.),
            Control::Alpha | Control::Offset => format!("{:.0}%", value * 100.),
            _ => format!("{value:.2}"),
        };
        let entity = cx.entity().downgrade();
        let measure = canvas(
            move |mut bounds, _, cx| {
                bounds.origin.x += px(6.);
                bounds.size.width -= px(12.);
                let _ = entity.update(cx, |this, _| {
                    this.slider_bounds.insert(control, bounds);
                });
            },
            |_, _, _, _| (),
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();
        div()
            .flex()
            .items_center()
            .gap_1()
            .w_full()
            .h(px(28.))
            .child(
                div()
                    .w(px(80.))
                    .flex_shrink_0()
                    .text_xs()
                    .child(control.label()),
            )
            .child(
                button(("minus", control as usize), "−")
                    .w(px(20.))
                    .px_0()
                    .py_0()
                    .text_sm()
                    .text_center()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let value = this.value(control) - step;
                        this.set_value(control, value);
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .id(("slider", control as usize))
                    .relative()
                    .w(px(128.))
                    .flex_shrink_0()
                    .h(px(24.))
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            this.dragging = Some(control);
                            this.scrub(control, event.position.x);
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px(6.))
                            .top(px(10.))
                            .w(px(116.))
                            .h(px(4.))
                            .rounded_full()
                            .bg(rgb(0x4a5465)),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px(6.))
                            .top(px(10.))
                            .w(px(116. * fraction))
                            .h(px(4.))
                            .rounded_full()
                            .bg(rgb(0x82bfff)),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px(116. * fraction))
                            .top(px(6.))
                            .size(px(12.))
                            .rounded_full()
                            .bg(white()),
                    )
                    .child(measure),
            )
            .child(
                button(("plus", control as usize), "+")
                    .w(px(20.))
                    .px_0()
                    .py_0()
                    .text_sm()
                    .text_center()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let value = this.value(control) + step;
                        this.set_value(control, value);
                        cx.notify();
                    })),
            )
            .child(div().w(px(44.)).text_right().text_sm().child(display))
    }

    fn editor(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mesh = self.effect == Effect::Mesh;
        let mut panel = div()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .bg(rgb(0x202633))
            .text_color(white())
            .rounded_lg();
        panel = panel.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(if mesh {
                    "Mesh control points"
                } else {
                    "Gradient stops"
                })
                .child(
                    button("reset", "Reset")
                        .text_sm()
                        .px_2()
                        .py_1()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.freeze();
                            let effect = this.effect;
                            this.editors[effect as usize] = Settings::new(effect);
                            this.native = false;
                            if effect == Effect::Mesh {
                                this.mesh_points = grid(0.).unwrap().points().to_vec();
                                this.mesh_selected = 0;
                                this.mesh_tangents = editor::mesh_tangents(&this.mesh_points);
                            }
                            cx.notify();
                        })),
                ),
        );
        let mut chips = div().flex().flex_wrap().gap_2();
        let colors: Vec<_> = if mesh {
            self.mesh_points
                .iter()
                .map(|p| [p.color[0], p.color[1], p.color[2], 1.])
                .collect()
        } else {
            self.settings().stops.iter().map(|s| s.color).collect()
        };
        let selected = if mesh {
            self.mesh_selected
        } else {
            self.settings().selected
        };
        for (i, color) in colors.iter().enumerate() {
            let label = if mesh {
                format!("{}", i + 1)
            } else {
                format!("{:.0}%", self.settings().stops[i].offset * 100.)
            };
            chips = chips.child(
                div()
                    .id(("color", i))
                    .w(px(44.))
                    .h(px(40.))
                    .border_2()
                    .border_color(if i == selected {
                        white()
                    } else {
                        hsla(0., 0., 0.3, 1.)
                    })
                    .rounded_md()
                    .cursor_pointer()
                    .bg(Rgba {
                        r: color[0],
                        g: color[1],
                        b: color[2],
                        a: 1.,
                    })
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_xs()
                    .text_color(black())
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.freeze();
                        if this.effect == Effect::Mesh {
                            this.mesh_selected = i;
                        } else {
                            this.settings_mut().selected = i;
                        }
                        cx.notify();
                    })),
            );
        }
        panel = panel.child(chips);
        if !mesh {
            let n = self.settings().stops.len();
            panel = panel.child(
                div()
                    .flex()
                    .gap_2()
                    .text_sm()
                    .child(
                        button("add", "Add")
                            .px_2()
                            .py_1()
                            .opacity(if n < 32 { 1. } else { 0.4 })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.freeze();
                                this.settings_mut().add();
                                this.native = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        button("duplicate", "Duplicate")
                            .px_2()
                            .py_1()
                            .opacity(if n < 32 { 1. } else { 0.4 })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.freeze();
                                this.settings_mut().duplicate();
                                this.native = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        button("remove", "Remove")
                            .px_2()
                            .py_1()
                            .opacity(if n > 2 { 1. } else { 0.4 })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.freeze();
                                this.settings_mut().remove();
                                this.native = false;
                                cx.notify();
                            })),
                    ),
            );
            panel = panel
                .child(format!("Stop {} of {} · 2–32 stops", selected + 1, n))
                .child(self.slider(Control::Offset, cx));
        } else {
            panel = panel.child(format!(
                "Point {} · row {}, column {}",
                selected + 1,
                selected / 3 + 1,
                selected % 3 + 1
            ));
        }
        let c = self.color();
        panel = panel.child(div().text_sm().child(format!(
            "Color #{:02X}{:02X}{:02X} · alpha {:.0}%",
            (c[0] * 255.).round() as u8,
            (c[1] * 255.).round() as u8,
            (c[2] * 255.).round() as u8,
            c[3] * 100.
        )));
        for control in [Control::Red, Control::Green, Control::Blue] {
            panel = panel.child(self.slider(control, cx));
        }
        if !mesh {
            panel = panel.child(self.slider(Control::Alpha, cx));
        }
        panel = panel.child(div().mt_2().child("Geometry"));
        let controls: &[Control] = match self.effect {
            Effect::Mesh => &[
                Control::MeshX,
                Control::MeshY,
                Control::MeshUAngle,
                Control::MeshULength,
                Control::MeshVAngle,
                Control::MeshVLength,
            ],
            Effect::Linear => &[Control::Angle],
            Effect::Radial => &[
                Control::CenterX,
                Control::CenterY,
                Control::RadiusX,
                Control::RadiusY,
            ],
            Effect::Conic => &[Control::Angle, Control::CenterX, Control::CenterY],
            Effect::Diamond => &[
                Control::Angle,
                Control::CenterX,
                Control::CenterY,
                Control::RadiusX,
                Control::RadiusY,
            ],
        };
        for control in controls {
            panel = panel.child(self.slider(*control, cx));
        }
        if mesh {
            panel = panel.child(
                button("reset-tangents", "Reset selected tangents")
                    .text_sm()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.freeze();
                        this.native = false;
                        let defaults = grid(0.).unwrap();
                        let point = &mut this.mesh_points[this.mesh_selected];
                        point.u_tangent = defaults.points()[this.mesh_selected].u_tangent;
                        point.v_tangent = defaults.points()[this.mesh_selected].v_tangent;
                        this.mesh_tangents[this.mesh_selected] = [
                            editor::MeshTangent::from_vector(point.u_tangent),
                            editor::MeshTangent::from_vector(point.v_tangent),
                        ];
                        cx.notify();
                    })),
            );
        }
        panel.child(div().text_xs().text_color(rgb(0xaab4c5)).child(if mesh {"Select point 1–9 to edit its U/V tangents. Angles: clockwise from right (0°); down is 90°. Length: 0–2 of the mesh size; default 0.5. Edits pause animation."}else{"Center: 0–1 of bounds. Radius: 0.01–2 × full size. Duplicate a stop for a hard edge. Edits pause animation."}))
    }
}
impl Render for Lab {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        if self.playing {
            self.time += now.duration_since(self.updated).as_secs_f32();
            window.request_animation_frame();
        }
        self.updated = now;
        #[cfg(feature = "portable-shaders")]
        {
            if self.hermite.side == 3 {
                let points = if self.effect == Effect::Mesh {
                    self.displayed_mesh()
                } else {
                    self.mesh_points.clone()
                };
                self.hermite.import_legacy(&points, &self.mesh_tangents);
                self.hermite.selected = self.mesh_selected;
            }
            self.hermite.refresh();
        }
        if self.analytic.stops() != self.settings().stops {
            self.analytic = Arc::new(
                self.analytic
                    .with_stops(self.settings().stops.clone())
                    .unwrap(),
            );
        }
        if let Some(kind) = self.settings().kind(self.effect, self.time)
            && self.analytic.kind() != kind
        {
            self.analytic = Arc::new(self.analytic.with_kind(kind).unwrap());
        }
        if self.effect == Effect::Mesh {
            let points = self.displayed_mesh();
            if self.mesh.grid().points() != points {
                self.mesh = Arc::new(
                    self.mesh
                        .with_grid(xgpui_shader::MeshGrid::new(3, 3, points).unwrap())
                        .unwrap(),
                );
            }
        }
        let paint = if self.native {
            Paint::Native(white().into())
        } else if self.effect == Effect::Mesh {
            self.mesh.clone().into()
        } else {
            self.analytic.clone().into()
        };
        let mut cards = div().flex().flex_wrap().gap_4();
        for (index, label) in ["Rounded background", "Dashed border", "Curved path fill"]
            .into_iter()
            .enumerate()
        {
            let p = paint.clone();
            let mut tile = painted().relative().w(px(320.)).h(px(240.));
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
        #[cfg(feature = "portable-shaders")]
        {
            cards = cards.child(brushes::gallery(self, cx));
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
                        this.freeze();
                        this.effect = effect;
                        this.native = false;
                        cx.notify();
                    })),
            );
        }
        div().id("paint-lab").size_full().flex().flex_col().gap_4().p_6().bg(rgb(if self.light_background {0xf4f5f7}else{0x151922})).text_color(rgb(if self.light_background {0x202633}else{0xffffff}))
            .on_mouse_move(cx.listener(|this,event:&MouseMoveEvent,_,cx|{if event.pressed_button!=Some(MouseButton::Left) {this.dragging=None;}else if let Some(control)=this.dragging {this.scrub(control,event.position.x);cx.notify();}}))
            .on_mouse_up(MouseButton::Left,cx.listener(|this,_,_,_|this.dragging=None))
            .on_mouse_up_out(MouseButton::Left,cx.listener(|this,_,_,_|this.dragging=None))
            .child(div().text_2xl().child("Paint lab · gradient editor"))
            .child(div().flex().flex_wrap().gap_4().items_center().child(choices)
                .child(button("play",if self.playing {"Pause"}else{"Animate"}).on_click(cx.listener(|this,_,_,cx|{if this.playing {this.freeze();}else{this.updated=Instant::now();this.playing=true;}cx.notify();})))
                .child(button("native",if self.native {"Native white"}else{"Gradient paint"}).on_click(cx.listener(|this,_,_,cx|{this.native = !this.native;cx.notify();})))
                .child(button("background",if self.light_background {"Background: light"}else{"Background: dark"}).on_click(cx.listener(|this,_,_,cx|{this.light_background = !this.light_background;cx.notify();})))
                .child(button("opacity",if self.faded {"Opacity 50%"}else{"Opacity 100%"}).on_click(cx.listener(|this,_,_,cx|{this.faded = !this.faded;cx.notify();})))
                .child(button("clip",if self.clipped {"Clip on"}else{"Clip off"}).on_click(cx.listener(|this,_,_,cx|{this.clipped = !this.clipped;cx.notify();}))))
            .child(div().flex().gap_5().flex_1().min_h_0()
                .child(div().id("editor-scroll").w(px(336.)).flex_shrink_0().overflow_y_scroll().child(self.editor(cx)))
                .child(div().id("preview-scroll").flex_1().min_w_0().overflow_y_scroll().flex().flex_col().gap_4()
                    .child(format!("{} · edits update all three previews",self.effect.label()))
                    .child(div().opacity(if self.faded {0.5}else{1.}).child(cards))
                    .child(div().text_sm().text_color(rgb(if self.light_background {0x596273}else{0xaab4c5})).child("Clipping reveals the same paint field. Each type keeps its own colors and geometry."))))
    }
}
fn main() -> Result<()> {
    let options = Options::parse();
    ensure!(
        (1..=10000).contains(&options.frames),
        "frames must be 1..=10000"
    );
    if options.headless {
        return checks::run(
            options.effect,
            options.frames,
            options.png.as_deref(),
            options.wgpu,
            options.portable_shader,
        );
    }
    gpui_platform::application().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1500.), px(940.)), cx);
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
                    mesh_points: grid(0.).unwrap().points().to_vec(),
                    mesh_selected: 0,
                    mesh_tangents: editor::mesh_tangents(grid(0.).unwrap().points()),
                    editors: [
                        Effect::Mesh,
                        Effect::Linear,
                        Effect::Radial,
                        Effect::Conic,
                        Effect::Diamond,
                    ]
                    .into_iter()
                    .map(Settings::new)
                    .collect(),
                    slider_bounds: HashMap::new(),
                    dragging: None,
                    effect: options.effect,
                    native: false,
                    faded: false,
                    clipped: false,
                    light_background: false,
                    #[cfg(feature = "portable-shaders")]
                    backdrop_changed: false,
                    #[cfg(feature = "portable-shaders")]
                    backdrop_moved: false,
                    #[cfg(feature = "portable-shaders")]
                    backdrop_disabled: false,
                    #[cfg(feature = "portable-shaders")]
                    layer_filter_step: 0,
                    #[cfg(feature = "portable-shaders")]
                    freeform_step: 2,
                    #[cfg(feature = "portable-shaders")]
                    hermite: hermite_editor::HermiteEditor::new(1),
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
