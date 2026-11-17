//! Node editor: anchors, handles and segment dragging for every node mode,
//! shape-preserving insertion and a curvature comb.
use super::{CurveLab, common::*};
use gpui::{prelude::*, *};
use xgpui_curve::geom;
use xgpui_curve::kurbo::{Affine, BezPath, Point as KPoint};
use xgpui_curve::node::{AutoSmooth, Node, NodeMode, NodePath, Side};

const W: f64 = 900.;
const H: f64 = 520.;

#[derive(Clone, Copy)]
enum Drag {
    Anchor(usize),
    Handle(usize, Side),
    Segment { index: usize, t: f64, last: KPoint },
}

pub struct NodesPanel {
    path: NodePath,
    selected: Option<usize>,
    comb: bool,
    drag: Option<Drag>,
    area: Area,
    view: Camera,
}

fn initial() -> NodePath {
    let anchors = [
        (110., 330.),
        (270., 180.),
        (450., 300.),
        (630., 170.),
        (790., 310.),
    ];
    NodePath::new(
        anchors
            .iter()
            .map(|&(x, y)| Node::auto(KPoint::new(x, y), AutoSmooth::Hobby))
            .collect(),
        false,
    )
    .expect("valid path")
}

const MODES: [(&str, NodeMode); 6] = [
    ("Corner", NodeMode::Corner),
    ("Smooth", NodeMode::Smooth),
    ("Symmetric", NodeMode::Symmetric),
    (
        "Catmull-Rom",
        NodeMode::Auto(AutoSmooth::CatmullRom { alpha: 0.5 }),
    ),
    ("Natural", NodeMode::Auto(AutoSmooth::Natural)),
    ("Hobby", NodeMode::Auto(AutoSmooth::Hobby)),
];

impl NodesPanel {
    pub fn new() -> Self {
        Self {
            path: initial(),
            selected: Some(2),
            comb: true,
            drag: None,
            area: area(),
            view: Camera::default(),
        }
    }

    /// Design space to window pixels, through the camera.
    fn transform(&self, bounds: Bounds<Pixels>) -> Affine {
        self.view.around(bounds) * fit(bounds, W, H)
    }

    /// Returns whether something was hit; empty space pans the view.
    fn down(&mut self, at: Point<Pixels>, clicks: usize) -> bool {
        let t = self.transform(self.area.get());
        let scale = t.as_coeffs()[0];
        let m = to_model(t, at);
        let near = |p: KPoint| (p - m).hypot() * scale <= 12.;
        for (i, n) in self.path.nodes().iter().enumerate() {
            if Some(i) == self.selected {
                for (side, h) in [(Side::In, n.handle_in), (Side::Out, n.handle_out)] {
                    if h.is_some_and(near) {
                        self.drag = Some(Drag::Handle(i, side));
                        return true;
                    }
                }
            }
        }
        if let Some(i) = self.path.nodes().iter().position(|n| near(n.anchor)) {
            self.selected = Some(i);
            self.drag = Some(Drag::Anchor(i));
            return true;
        }
        if let Some(hit) = geom::nearest_on_path(&self.path.to_bezpath(), m)
            && hit.distance * scale <= 10.
        {
            if clicks >= 2 {
                if let Ok(i) = self.path.insert_at(hit.segment, hit.t) {
                    self.selected = Some(i);
                }
                self.drag = None;
            } else {
                self.drag = Some(Drag::Segment {
                    index: hit.segment,
                    t: hit.t,
                    last: m,
                });
            }
            return true;
        }
        self.selected = None;
        self.drag = None;
        false
    }

    fn drag_to(&mut self, at: Point<Pixels>) {
        let m = to_model(self.transform(self.area.get()), at);
        match self.drag {
            Some(Drag::Anchor(i)) => {
                let _ = self.path.move_anchor(i, m);
            }
            Some(Drag::Handle(i, side)) => {
                let _ = self.path.move_handle(i, side, m);
            }
            Some(Drag::Segment { index, t, last }) => {
                let _ = self.path.drag_segment(index, t, m - last);
                self.drag = Some(Drag::Segment { index, t, last: m });
            }
            None => {}
        }
    }
}

pub fn render(lab: &mut CurveLab, _window: &mut Window, cx: &mut Context<CurveLab>) -> AnyElement {
    let panel = &lab.nodes;
    let path = panel.path.clone();
    let (selected, comb, view) = (panel.selected, panel.comb, panel.view);
    let current = selected.and_then(|i| path.nodes().get(i)).map(|n| n.mode);
    let mut controls = div().flex().flex_wrap().gap_2();
    for (label, mode) in MODES {
        let active = current == Some(mode);
        controls = controls.child(button(label, active).on_click(cx.listener(
            move |this, _, _, cx| {
                if let Some(i) = this.nodes.selected {
                    let _ = this.nodes.path.set_mode(i, mode);
                } else {
                    for i in 0..this.nodes.path.nodes().len() {
                        let _ = this.nodes.path.set_mode(i, mode);
                    }
                }
                cx.notify();
            },
        )));
    }
    controls = controls
        .child(
            button("Closed", path.is_closed()).on_click(cx.listener(|this, _, _, cx| {
                let closed = !this.nodes.path.is_closed();
                this.nodes.path.set_closed(closed);
                cx.notify();
            })),
        )
        .child(
            button("Curvature comb", comb).on_click(cx.listener(|this, _, _, cx| {
                this.nodes.comb = !this.nodes.comb;
                cx.notify();
            })),
        )
        .child(
            button("Delete node", false).on_click(cx.listener(|this, _, _, cx| {
                if let Some(i) = this.nodes.selected
                    && this.nodes.path.nodes().len() > 2
                {
                    let _ = this.nodes.path.delete_node(i);
                    this.nodes.selected = None;
                }
                cx.notify();
            })),
        )
        .child(
            button("Reset", false).on_click(cx.listener(|this, _, _, cx| {
                this.nodes.path = initial();
                this.nodes.selected = Some(2);
                cx.notify();
            })),
        )
        .child(view_button(cx, view, |lab: &mut CurveLab| {
            &mut lab.nodes.view
        }))
        .child(div().text_color(rgb(0x9fb0cc)).child(match selected {
            Some(i) => format!("Node {i} selected · mode buttons apply to it"),
            None => "No node selected · mode buttons apply to all".into(),
        }));
    let surface = interactive(
        "nodes-surface",
        cx,
        &panel.area,
        |lab: &mut CurveLab| &mut lab.nodes.view,
        |this: &mut CurveLab, at, clicks, _| this.nodes.down(at, clicks),
        |this: &mut CurveLab, at, _| this.nodes.drag_to(at),
        |this: &mut CurveLab, _| this.nodes.drag = None,
    )
    .rounded_lg()
    .overflow_hidden()
    .bg(rgb(0x101828))
    .child(
        canvas(record(&panel.area), move |bounds, _, window, _| {
            let t = view.around(bounds) * fit(bounds, W, H);
            if comb {
                let teeth = path.curvature_comb(24, 4000.);
                let mut spine = BezPath::new();
                let mut hairs = BezPath::new();
                for (i, (p, tip)) in teeth.iter().enumerate() {
                    hairs.extend(line(*p, *tip));
                    if i == 0 {
                        spine.move_to(*tip)
                    } else {
                        spine.line_to(*tip)
                    }
                }
                stroke(window, &hairs, t, 1., hsla(0.55, 0.7, 0.6, 0.35));
                stroke(window, &spine, t, 1., hsla(0.55, 0.8, 0.7, 0.8));
            }
            contrast(window, &path.to_bezpath(), t, 2.5);
            for (i, n) in path.nodes().iter().enumerate() {
                if Some(i) == selected {
                    for h in [n.handle_in, n.handle_out].into_iter().flatten() {
                        stroke(window, &line(n.anchor, h), t, 1.5, rgb(0x9ecbff));
                        handle(window, t * h, 4., rgb(0x9ecbff));
                    }
                }
                let color = match n.mode {
                    NodeMode::Corner => rgb(0xff9f43),
                    NodeMode::Smooth | NodeMode::Symmetric => rgb(0x9ecbff),
                    NodeMode::Auto(_) => rgb(0x7ee0a1),
                };
                handle(
                    window,
                    t * n.anchor,
                    if Some(i) == selected { 7. } else { 5. },
                    color,
                );
            }
        })
        .size_full(),
    );
    div()
        .size_full()
        .flex()
        .flex_col()
        .gap_2()
        .child(controls)
        .child(div().flex_1().min_h_0().child(surface))
        .into_any_element()
}
