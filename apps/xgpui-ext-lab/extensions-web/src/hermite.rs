//! Browser UI for paint-lab's shared Hermite editing model.
use gpui::{prelude::*, *};
use std::collections::HashMap;
use xgpui_shader::painted;

#[path = "../../../web-support/mesh_tangent.rs"]
mod editor;
#[allow(dead_code, unreachable_patterns)]
#[path = "../../paint-lab/src/hermite_editor.rs"]
mod model;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Control {
    HermiteX,
    HermiteY,
    HermiteUAngle,
    HermiteULength,
    HermiteVAngle,
    HermiteVLength,
}
impl Control {
    fn range(self) -> (f32, f32, f32) {
        match self {
            Self::HermiteUAngle | Self::HermiteVAngle => (0., 360., 1.),
            Self::HermiteULength | Self::HermiteVLength => (0., 2., 0.01),
            _ => (0., 1., 0.01),
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::HermiteX => "Point X",
            Self::HermiteY => "Point Y",
            Self::HermiteUAngle => "U angle (°)",
            Self::HermiteULength => "U length",
            Self::HermiteVAngle => "V angle (°)",
            Self::HermiteVLength => "V length",
        }
    }
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

pub struct HermiteDemo {
    mesh: model::HermiteEditor,
    light: bool,
    dragging: Option<Control>,
    tracks: HashMap<Control, Bounds<Pixels>>,
}
fn button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
    div()
        .id(id)
        .px_3()
        .py_2()
        .rounded_md()
        .bg(rgb(0x344158))
        .text_color(white())
        .cursor_pointer()
        .child(label.into())
}
impl HermiteDemo {
    pub fn new() -> Self {
        Self {
            mesh: model::HermiteEditor::new(0),
            light: false,
            dragging: None,
            tracks: HashMap::new(),
        }
    }
    fn scrub(&mut self, control: Control, x: Pixels) {
        if let Some(track) = self.tracks.get(&control) {
            let (min, max, _) = control.range();
            let fraction = ((x - track.origin.x) / track.size.width).clamp(0., 1.);
            self.mesh.set(control, min + (max - min) * fraction);
        }
    }
    fn slider(&self, control: Control, cx: &mut Context<Self>) -> impl IntoElement {
        let value = self.mesh.value(control);
        let (min, max, step) = control.range();
        let fraction = (value - min) / (max - min);
        let entity = cx.entity().downgrade();
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .w(px(100.))
                    .text_sm()
                    .child(format!("{}: {value:.2}", control.label())),
            )
            .child(
                button(("minus", control as usize), "−").on_click(cx.listener(
                    move |s, _, _, cx| {
                        s.mesh.set(control, s.mesh.value(control) - step);
                        cx.notify();
                    },
                )),
            )
            .child(
                div()
                    .id(("track", control as usize))
                    .relative()
                    .w(px(150.))
                    .h(px(28.))
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |s, e: &MouseDownEvent, _, cx| {
                            s.dragging = Some(control);
                            s.scrub(control, e.position.x);
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    )
                    .child(
                        canvas(
                            move |bounds, _, cx| {
                                let _ = entity.update(cx, |s, _| {
                                    s.tracks.insert(control, bounds);
                                });
                            },
                            |_, _, _, _| (),
                        )
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full(),
                    )
                    .child(
                        div()
                            .absolute()
                            .top(px(12.))
                            .w_full()
                            .h(px(4.))
                            .bg(rgb(0x526079)),
                    )
                    .child(
                        div()
                            .absolute()
                            .top(px(7.))
                            .left(px(140. * fraction))
                            .w(px(10.))
                            .h(px(14.))
                            .rounded_md()
                            .bg(rgb(0x82bfff)),
                    ),
            )
            .child(
                button(("plus", control as usize), "+").on_click(cx.listener(
                    move |s, _, _, cx| {
                        s.mesh.set(control, s.mesh.value(control) + step);
                        cx.notify();
                    },
                )),
            )
    }
}
impl Render for HermiteDemo {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.mesh.refresh();
        let mut points = div().flex().flex_wrap().gap_2();
        for i in 0..self.mesh.points.len() {
            points = points.child(
                button(
                    ("point", i),
                    format!(
                        "Point {} · {},{}",
                        i + 1,
                        i / self.mesh.side as usize + 1,
                        i % self.mesh.side as usize + 1
                    ),
                )
                .bg(rgb(if i == self.mesh.selected {
                    0x2766b0
                } else {
                    0x344158
                }))
                .on_click(cx.listener(move |s, _, _, cx| {
                    s.mesh.selected = i;
                    cx.notify();
                })),
            );
        }
        let mut controls = div().flex().flex_wrap().gap_4();
        for pair in [
            [Control::HermiteX, Control::HermiteY],
            [Control::HermiteUAngle, Control::HermiteULength],
            [Control::HermiteVAngle, Control::HermiteVLength],
        ] {
            controls = controls.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(self.slider(pair[0], cx))
                    .child(self.slider(pair[1], cx)),
            );
        }
        let direct = match &self.mesh.brush {
            Ok(brush)=>div().child(painted().w(px(320.)).h(px(210.)).bg_brush(brush.clone())),
            Err(reason)=>div().w(px(320.)).h(px(210.)).p_4().bg(rgb(0x45454f)).text_color(white()).child(format!("Direct brush unavailable: {reason}. Adjust tangents or reset; the legacy preview still shows your current edit.")),
        };
        let selected = self.mesh.selected;
        let geometry = self.mesh.points.clone();
        let preview = div()
            .relative()
            .w(px(320.))
            .h(px(210.))
            .bg(rgb(0x45454f))
            .child(direct)
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, _, window, _| {
                        for (i, p) in geometry.iter().enumerate() {
                            let center = bounds.origin
                                + point(
                                    bounds.size.width * p.position[0],
                                    bounds.size.height * p.position[1],
                                );
                            if i == selected {
                                for (tangent, color) in
                                    [(p.u_tangent, rgb(0xffbf47)), (p.v_tangent, rgb(0x6ee7ff))]
                                {
                                    let end = center
                                        + point(
                                            bounds.size.width * tangent[0] * 0.25,
                                            bounds.size.height * tangent[1] * 0.25,
                                        );
                                    let mut path = PathBuilder::stroke(px(2.));
                                    path.move_to(center);
                                    path.line_to(end);
                                    if let Ok(path) = path.build() {
                                        window.paint_path(path, color);
                                    }
                                }
                            }
                            window.paint_quad(fill(
                                Bounds::new(center - point(px(4.), px(4.)), size(px(8.), px(8.))),
                                if i == selected {
                                    rgb(0xffffff)
                                } else {
                                    rgb(0x101827)
                                },
                            ));
                        }
                    },
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            );
        let names = [
            "Regular",
            "Bend U tangents",
            "Bend V tangents",
            "Inset patch",
            "Move center point",
        ];
        let mut root=div().id("hermite-editor").size_full().overflow_y_scroll().p_4().pb(px(150.)).flex().flex_col().gap_4()
            .bg(rgb(if self.light {0xf2f4fa}else{0x111827})).text_color(rgb(if self.light {0x18243b}else{0xe2e8f0})).font_family("IBM Plex Sans")
            .on_mouse_move(cx.listener(|s,e:&MouseMoveEvent,_,cx| {if e.pressed_button==Some(MouseButton::Left) {if let Some(c)=s.dragging {s.scrub(c,e.position.x);cx.notify();}}else{s.dragging=None;}}))
            .on_mouse_up(MouseButton::Left,cx.listener(|s,_,_,_| {s.dragging=None;}))
            .child(div().text_2xl().child("Hermite mesh · live editor"))
            .child("The paint-lab editing model, running in WebAssembly. Select a point, then drag the sliders or use −/+.")
            .child(div().flex().flex_wrap().gap_2()
                .child(button("grid",format!("Grid: {}×{} · switch",self.mesh.side,self.mesh.side)).on_click(cx.listener(|s,_,_,cx| {s.mesh.set_side(if s.mesh.side==2 {3}else{2});cx.notify();})))
                .child(button("preset",format!("{} · next preset",names[self.mesh.preset as usize])).on_click(cx.listener(|s,_,_,cx| {s.mesh.load_preset((s.mesh.preset+1)%s.mesh.preset_count());cx.notify();})))
                .child(button("reset-point","Reset selected point").on_click(cx.listener(|s,_,_,cx| {s.mesh.reset_selected();cx.notify();})))
                .child(button("reset-all","Reset all points").on_click(cx.listener(|s,_,_,cx| {s.mesh.load_preset(s.mesh.preset);cx.notify();})))
                .child(button("background","Swap light / dark").on_click(cx.listener(|s,_,_,cx| {s.light = !s.light;cx.notify();}))))
            .child(points).child(controls)
            .child("Angles are clockwise from right; lengths use normalized mesh coordinates. White marker = selected point; gold/cyan lines = U/V tangents at ¼ length. Both previews use the same edited points.")
            .child(div().flex().flex_wrap().gap_4()
                .child(div().flex().flex_col().gap_2().child("Legacy mesh · current edit").child(painted().w(px(320.)).h(px(210.)).bg_paint(self.mesh.legacy.clone())))
                .child(div().flex().flex_col().gap_2().child("Direct brush · point markers").child(preview)));
        if let Ok(brush) = &self.mesh.brush {
            root = root
                .child(
                    painted()
                        .w(px(650.))
                        .h(px(64.))
                        .border_8()
                        .border_brush(brush.clone())
                        .child("Same edited field on a border"),
                )
                .child(
                    painted()
                        .text_size(px(32.))
                        .text_brush(brush.clone())
                        .child("Hermite across glyphs"),
                );
        }
        root
    }
}
