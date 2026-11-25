//! Mesh lattice panel: a 3 × 4 Hermite mesh gradient with its exact curved
//! wireframe, node dragging and cursor-exact wire dragging.
use super::{CurveLab, common::*};
use gpui::{prelude::*, *};
use std::{cell::RefCell, rc::Rc, sync::Arc};
use xgpui_curve::geom::{Point as KPoint, Rect, Vec2};
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
    view: Camera,
    cache: Option<(
        Vec<xgpui_curve::mesh::MeshControl>,
        Arc<xgpui_shader::MeshGradient>,
    )>,
    cropped: CropCache,
}

/// Largest mesh coordinate `MeshGrid` accepts, with a little headroom.
const MESH_LIMIT: f64 = 15.5;

/// The window rectangle to paint a zoomed gradient into, and `grid` remapped
/// onto it.
///
/// Painting the whole zoomed rectangle overflows GPUI's custom-texture limits
/// (16384 device pixels per side, 256 MiB) once zoomed in. Paint only the
/// visible crop instead and move the mesh coordinates so the picture is
/// unchanged. Deep zoom pushes off-screen nodes past `MeshGrid`'s ±16 range,
/// so the crop grows around its center just enough to keep them inside.
/// `None` when nothing is visible.
fn crop(
    grid: &xgpui_shader::MeshGrid,
    zoomed: Rect,
    visible: Rect,
) -> Option<(Rect, Option<xgpui_shader::MeshGrid>)> {
    let inside = zoomed.intersect(visible);
    if inside.width() < 1. || inside.height() < 1. {
        return None;
    }
    if inside == zoomed {
        return Some((zoomed, None));
    }
    let (center, half) = (
        inside.center(),
        Vec2::new(inside.width(), inside.height()) / 2.,
    );
    let size = Vec2::new(zoomed.width(), zoomed.height());
    let window =
        |p: [f32; 2]| zoomed.origin() + Vec2::new(p[0] as f64 * size.x, p[1] as f64 * size.y);
    // Grow factor per axis: positions satisfy |x - center| / (2 h k) + 0.5 <= LIMIT,
    // tangents satisfy |t| * size / (2 h k) <= LIMIT.
    let mut grow = Vec2::new(1., 1.);
    for p in grid.points() {
        let d = window(p.position) - center;
        for (k, d, h, s, t) in [
            (
                &mut grow.x,
                d.x,
                half.x,
                size.x,
                [p.u_tangent[0], p.v_tangent[0]],
            ),
            (
                &mut grow.y,
                d.y,
                half.y,
                size.y,
                [p.u_tangent[1], p.v_tangent[1]],
            ),
        ] {
            *k = k.max(d.abs() / (2. * h * (MESH_LIMIT - 0.5)));
            for t in t {
                *k = k.max((t as f64).abs() * s / (2. * h * MESH_LIMIT));
            }
        }
    }
    let half = Vec2::new(half.x * grow.x, half.y * grow.y);
    let rect = Rect::from_points(center - half, center + half);
    let points = grid
        .points()
        .iter()
        .map(|p| {
            let q = window(p.position);
            let scale = |t: [f32; 2]| {
                [
                    (t[0] as f64 * size.x / rect.width()) as f32,
                    (t[1] as f64 * size.y / rect.height()) as f32,
                ]
            };
            xgpui_shader::ControlPoint {
                position: [
                    ((q.x - rect.x0) / rect.width()) as f32,
                    ((q.y - rect.y0) / rect.height()) as f32,
                ],
                u_tangent: scale(p.u_tangent),
                v_tangent: scale(p.v_tangent),
                color: p.color,
            }
        })
        .collect();
    let grid = xgpui_shader::MeshGrid::new(grid.width(), grid.height(), points).ok()?;
    Some((rect, Some(grid)))
}

type CroppedGradient = (xgpui_shader::MeshGrid, Arc<xgpui_shader::MeshGradient>);

/// The last cropped gradient, so repaints at the same view reuse its texture.
#[derive(Clone, Default)]
struct CropCache(Rc<RefCell<Option<CroppedGradient>>>);

impl CropCache {
    fn paint(
        &self,
        full: &Arc<xgpui_shader::MeshGradient>,
        zoomed: Rect,
        visible: Rect,
    ) -> Option<(Rect, Arc<xgpui_shader::MeshGradient>)> {
        let (rect, grid) = crop(full.grid(), zoomed, visible)?;
        let Some(grid) = grid else {
            return Some((rect, full.clone()));
        };
        let mut cache = self.0.borrow_mut();
        if let Some((old, paint)) = &*cache
            && *old == grid
        {
            return Some((rect, paint.clone()));
        }
        let paint = Arc::new(full.with_grid(grid.clone()).ok()?);
        *cache = Some((grid, paint.clone()));
        Some((rect, paint))
    }
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
            view: Camera::default(),
            cache: None,
            cropped: CropCache::default(),
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

    /// Lattice space (0..1 of the canvas) to window pixels, through the camera.
    fn transform(&self, bounds: Bounds<Pixels>) -> Affine {
        self.view.around(bounds) * unit(bounds)
    }

    fn down(&mut self, at: Point<Pixels>) -> bool {
        let bounds = self.area.get();
        let t = self.transform(bounds);
        let m = to_model(t, at);
        let scale = f64::from(f32::from(bounds.size.width.min(bounds.size.height))).max(1.)
            * self.view.zoom();
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
        self.drag.is_some()
    }

    fn drag_to(&mut self, at: Point<Pixels>) {
        let m = to_model(self.transform(self.area.get()), at);
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
    let (curved, tangents, view) = (panel.curved, panel.tangents, panel.view);
    let cropped = panel.cropped.clone();
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
        .child(view_button(cx, view, |lab: &mut CurveLab| {
            &mut lab.mesh.view
        }))
        .child(div().text_color(rgb(0x9fb0cc)).child(format!(
            "{explicit} of {} nodes have explicit tangents",
            ROWS * COLS
        )));
    let surface = interactive(
        "mesh-surface",
        cx,
        &panel.area,
        |lab: &mut CurveLab| &mut lab.mesh.view,
        |this: &mut CurveLab, at, _, _| this.mesh.down(at),
        |this: &mut CurveLab, at, _| this.mesh.drag_to(at),
        |this: &mut CurveLab, _| this.mesh.drag = None,
    )
    .rounded_lg()
    .overflow_hidden()
    .bg(rgb(0x05070c))
    .child(
        canvas(record(&panel.area), move |bounds, _, window, _| {
            let camera = view.around(bounds);
            if let Some(content) = gradient {
                // The gradient fills its rectangle, so zoom the rectangle itself,
                // but only paint the part that is on screen.
                let zoomed = Rect::from_points(
                    camera * kp(bounds.origin),
                    camera * kp(bounds.bottom_right()),
                );
                let visible = Rect::from_points(kp(bounds.origin), kp(bounds.bottom_right()));
                if let Some((rect, content)) = cropped.paint(&content, zoomed, visible) {
                    window.paint_custom(
                        CustomGeometry::RoundedRect {
                            bounds: Bounds::from_corners(
                                gp(rect.origin()),
                                gp(KPoint::new(rect.x1, rect.y1)),
                            ),
                            corner_radii: Corners::default(),
                        },
                        content,
                    );
                }
            }
            let t = camera * unit(bounds);
            let wire = if curved {
                lattice.wireframe()
            } else {
                let mut p = xgpui_curve::geom::BezPath::new();
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

use xgpui_curve::geom::Affine;

#[cfg(test)]
mod tests {
    use super::*;

    fn grid() -> xgpui_shader::MeshGrid {
        let points = initial()
            .controls()
            .iter()
            .zip(COLORS)
            .map(|(c, color)| xgpui_shader::ControlPoint {
                position: c.position,
                u_tangent: c.u_tangent,
                v_tangent: c.v_tangent,
                color,
            })
            .collect();
        xgpui_shader::MeshGrid::new(COLS as u32, ROWS as u32, points).unwrap()
    }

    #[::core::prelude::v1::test]
    fn deep_zoom_paints_a_bounded_crop_with_the_same_picture() {
        let grid = grid();
        let visible = Rect::new(40., 30., 1240., 830.);
        // Unzoomed: paint the rectangle as is.
        assert!(matches!(crop(&grid, visible, visible), Some((r, None)) if r == visible));
        // Off screen: paint nothing.
        assert!(crop(&grid, Rect::new(2000., 2000., 2100., 2100.), visible).is_none());
        for zoom in [2., 8., 32.] {
            for anchor in [(0., 0.), (0.5, 0.5), (1., 1.), (0.9, 0.1)] {
                let size = Vec2::new(visible.width() * zoom, visible.height() * zoom);
                let fixed = KPoint::new(
                    visible.x0 + anchor.0 * visible.width(),
                    visible.y0 + anchor.1 * visible.height(),
                );
                let origin = fixed - Vec2::new(anchor.0 * size.x, anchor.1 * size.y);
                let zoomed = Rect::from_origin_size(origin, (size.x, size.y));
                let (rect, cropped) = crop(&grid, zoomed, visible).unwrap();
                let cropped = cropped.expect("zoomed in past the canvas");
                // Covers the screen, and stays far below the 16384px texture limit
                // even on a 2x display.
                assert!(
                    rect.contains(visible.origin())
                        && rect.contains(KPoint::new(visible.x1 - 1e-6, visible.y1 - 1e-6))
                );
                assert!(
                    rect.width() * 2. < 8192. && rect.height() * 2. < 8192.,
                    "{zoom} {anchor:?} {rect:?}"
                );
                // Every node lands on the same window point.
                for (a, b) in grid.points().iter().zip(cropped.points()) {
                    let at = |r: Rect, p: [f32; 2]| {
                        KPoint::new(
                            r.x0 + p[0] as f64 * r.width(),
                            r.y0 + p[1] as f64 * r.height(),
                        )
                    };
                    assert!(at(zoomed, a.position).distance(at(rect, b.position)) < 0.05);
                }
            }
        }
    }
}
