//! Mesh lattice panel: a 3 × 4 Hermite mesh gradient with its exact curved
//! wireframe, node dragging and cursor-exact wire dragging.
use super::{CurveLab, common::*};
use gpui::{prelude::*, *};
use std::sync::Arc;
use xgpui_curve::kurbo::{ParamCurve, Point as KPoint, Vec2};
use xgpui_curve::mesh::{Axis, MeshLattice};

const ROWS: usize = 3;
const COLS: usize = 4;
const COLORS: [[f32; 3]; 12] = [
    [0.96, 0.32, 0.30],
    [1.0, 0.62, 0.20],
    [0.98, 0.85, 0.30],
    [0.55, 0.85, 0.40],
    [0.85, 0.25, 0.55],
    [0.95, 0.95, 0.95],
    [0.30, 0.75, 0.85],
    [0.25, 0.55, 0.95],
    [0.40, 0.20, 0.70],
    [0.20, 0.35, 0.85],
    [0.10, 0.65, 0.60],
    [0.15, 0.25, 0.45],
];

#[derive(Clone, Copy)]
enum Drag {
    Node(usize, usize),
    Edge {
        row: usize,
        col: usize,
        axis: Axis,
        t: f64,
        last: KPoint,
    },
}

pub struct MeshPanel {
    lattice: MeshLattice,
    curved: bool,
    tangents: bool,
    drag: Option<Drag>,
    area: Area,
    cache: Option<(
        Vec<xgpui_curve::mesh::MeshControl>,
        Arc<xgpui_shader::MeshGradient>,
    )>,
}

fn initial() -> MeshLattice {
    let mut points = Vec::new();
    for r in 0..ROWS {
        for c in 0..COLS {
            let (u, v) = (
                0.08 + 0.84 * c as f64 / (COLS - 1) as f64,
                0.08 + 0.84 * r as f64 / (ROWS - 1) as f64,
            );
            points.push(KPoint::new(
                u + 0.05 * (v * 6.).sin(),
                v + 0.06 * (u * 5.).cos(),
            ));
        }
    }
    MeshLattice::new(ROWS, COLS, points).expect("valid lattice")
}

impl MeshPanel {
    pub fn new() -> Self {
        Self {
            lattice: initial(),
            curved: true,
            tangents: false,
            drag: None,
            area: area(),
            cache: None,
        }
    }

    fn gradient(&mut self) -> Option<Arc<xgpui_shader::MeshGradient>> {
        let controls = self.lattice.controls();
        if let Some((old, paint)) = &self.cache
            && *old == controls
        {
            return Some(paint.clone());
        }
        let points = controls
            .iter()
            .zip(COLORS)
            .map(|(c, color)| xgpui_shader::ControlPoint {
                position: c.position,
                u_tangent: c.u_tangent,
                v_tangent: c.v_tangent,
                color,
            })
            .collect();
        let grid = xgpui_shader::MeshGrid::new(COLS as u32, ROWS as u32, points).ok()?;
        let paint = Arc::new(xgpui_shader::MeshGradient::new(grid, 32).ok()?);
        self.cache = Some((controls, paint.clone()));
        Some(paint)
    }

    fn down(&mut self, at: Point<Pixels>) {
        let bounds = self.area.get();
        let t = unit(bounds);
        let m = to_model(t, at);
        let scale = f64::from(f32::from(bounds.size.width.min(bounds.size.height))).max(1.);
        let mut best: Option<(f64, usize, usize)> = None;
        for r in 0..ROWS {
            for c in 0..COLS {
                let d = (t * self.lattice.point(r, c)).distance(kp(at));
                if d < 14. && best.is_none_or(|b| d < b.0) {
                    best = Some((d, r, c));
                }
            }
        }
        self.drag = if let Some((_, r, c)) = best {
            Some(Drag::Node(r, c))
        } else {
            self.lattice.hit_edge(m, 10. / scale).map(|hit| Drag::Edge {
                row: hit.row,
                col: hit.col,
                axis: hit.axis,
                t: hit.t,
                last: m,
            })
        };
    }

    fn drag_to(&mut self, at: Point<Pixels>) {
        let m = to_model(unit(self.area.get()), at);
        match self.drag {
            Some(Drag::Node(r, c)) => {
                let _ = self.lattice.set_point(
                    r,
                    c,
                    KPoint::new(m.x.clamp(-0.1, 1.1), m.y.clamp(-0.1, 1.1)),
                );
            }
            Some(Drag::Edge {
                row,
                col,
                axis,
                t,
                last,
            }) => {
                let _ = self.lattice.drag_edge(row, col, axis, t, m - last);
                self.drag = Some(Drag::Edge {
                    row,
                    col,
                    axis,
                    t,
                    last: m,
                });
            }
            None => {}
        }
    }
}

pub fn render(lab: &mut CurveLab, _window: &mut Window, cx: &mut Context<CurveLab>) -> AnyElement {
    let panel = &mut lab.mesh;
    let gradient = panel.gradient();
    let lattice = panel.lattice.clone();
    let (curved, tangents) = (panel.curved, panel.tangents);
    let explicit = (0..ROWS)
        .flat_map(|r| (0..COLS).map(move |c| (r, c)))
        .filter(|&(r, c)| {
            [Axis::U, Axis::V].iter().any(|&a| {
                matches!(
                    lattice.tangent(r, c, a),
                    xgpui_curve::mesh::Tangent::Explicit(_)
                )
            })
        })
        .count();
    let controls = div()
        .flex()
        .flex_wrap()
        .gap_2()
        .child(
            button(
                if curved {
                    "Wires: curved (exact edges)"
                } else {
                    "Wires: straight (before)"
                },
                curved,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.mesh.curved = !this.mesh.curved;
                cx.notify();
            })),
        )
        .child(
            button("Show tangents", tangents).on_click(cx.listener(|this, _, _, cx| {
                this.mesh.tangents = !this.mesh.tangents;
                cx.notify();
            })),
        )
        .child(
            button("Reset tangents", false).on_click(cx.listener(|this, _, _, cx| {
                this.mesh.lattice.reset_tangents();
                cx.notify();
            })),
        )
        .child(
            button("Reset mesh", false).on_click(cx.listener(|this, _, _, cx| {
                this.mesh.lattice = initial();
                cx.notify();
            })),
        )
        .child(div().text_color(rgb(0x9fb0cc)).child(format!(
            "{explicit} of {} nodes have explicit tangents",
            ROWS * COLS
        )));
    let surface = interactive(
        "mesh-surface",
        cx,
        |this: &mut CurveLab, at, _, _| this.mesh.down(at),
        |this: &mut CurveLab, at, _| this.mesh.drag_to(at),
        |this: &mut CurveLab, _| this.mesh.drag = None,
    )
    .rounded_lg()
    .overflow_hidden()
    .bg(rgb(0x05070c))
    .child(
        canvas(record(&panel.area), move |bounds, _, window, _| {
            if let Some(content) = gradient {
                window.paint_custom(
                    CustomGeometry::RoundedRect {
                        bounds,
                        corner_radii: Corners::default(),
                    },
                    content,
                );
            }
            let t = unit(bounds);
            let wire = if curved {
                lattice.wireframe()
            } else {
                let mut p = xgpui_curve::kurbo::BezPath::new();
                for r in 0..ROWS {
                    for c in 0..COLS {
                        if c + 1 < COLS {
                            p.extend(line(lattice.point(r, c), lattice.point(r, c + 1)));
                        }
                        if r + 1 < ROWS {
                            p.extend(line(lattice.point(r, c), lattice.point(r + 1, c)));
                        }
                    }
                }
                p
            };
            contrast(window, &wire, t, 1.5);
            for r in 0..ROWS {
                for c in 0..COLS {
                    let p = lattice.point(r, c);
                    if tangents {
                        for (axis, color) in [(Axis::U, rgb(0xff8a65)), (Axis::V, rgb(0x64d2ff))] {
                            let m: Vec2 = lattice.resolved_tangent(r, c, axis) / 3.;
                            let arm = line(t * (p - m), t * (p + m));
                            stroke(window, &arm, Affine::IDENTITY, 4.5, hsla(0., 0., 0., 0.6));
                            stroke(window, &arm, Affine::IDENTITY, 2.5, color);
                            handle(window, t * (p + m), 3., color);
                        }
                    }
                    handle(window, t * p, 5., rgb(0xffffff));
                }
            }
            // Mark the midpoint of each wire to invite dragging.
            if curved {
                for r in 0..ROWS {
                    for c in 0..COLS {
                        for axis in [Axis::U, Axis::V] {
                            if (axis == Axis::U && c + 1 < COLS)
                                || (axis == Axis::V && r + 1 < ROWS)
                            {
                                dot(
                                    window,
                                    t * lattice.edge(r, c, axis).eval(0.5),
                                    2.5,
                                    hsla(0., 0., 1., 0.7),
                                );
                            }
                        }
                    }
                }
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

use xgpui_curve::kurbo::Affine;
