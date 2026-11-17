//! Live controls for the shared-point direct/legacy Hermite comparison.
use super::{Control, editor::MeshTangent};
use std::sync::Arc;
use xgpui_curve::kurbo::{BezPath, Point as KPoint, Vec2};
use xgpui_curve::mesh::{Axis, MeshLattice, Tangent};
use xgpui_shader::{Brush, BrushMeshPoint, ControlPoint, MeshGradient, MeshGrid};

/// A wire being dragged: the edge from node (row, col) along `axis`, grabbed at `t`.
#[derive(Clone, Copy, Debug)]
pub(super) struct WireDrag {
    row: usize,
    col: usize,
    axis: Axis,
    t: f64,
    last: KPoint,
}

/// The editor never clamps tangents (the direct brush reports invalid edits),
/// so the lattice's fold clamp is set far beyond any editable length.
const NO_CLAMP: f64 = 1e6;

pub(super) struct HermiteEditor {
    pub points: Vec<BrushMeshPoint>,
    pub side: u32,
    pub tangents: Vec<[MeshTangent; 2]>,
    pub selected: usize,
    pub preset: u8,
    pub edited: bool,
    pub dirty: bool,
    pub legacy: Arc<MeshGradient>,
    pub brush: Result<Brush, String>,
    pub wire: Option<WireDrag>,
}
impl HermiteEditor {
    pub fn new(preset: u8) -> Self {
        Self::with_side(preset, 2)
    }
    fn with_side(preset: u8, side: u32) -> Self {
        let points = preset_points(preset, side);
        let tangents = points
            .iter()
            .map(|p| {
                [
                    MeshTangent::from_vector(p.u_tangent),
                    MeshTangent::from_vector(p.v_tangent),
                ]
            })
            .collect();
        let legacy =
            Arc::new(MeshGradient::new(grid(&points, side), 64).expect("bounded editor mesh"));
        let brush = Brush::hermite_mesh(side, side, points.clone()).map_err(|e| e.to_string());
        Self {
            points,
            tangents,
            side,
            selected: 0,
            preset,
            edited: false,
            dirty: false,
            legacy,
            brush,
            wire: None,
        }
    }
    pub fn preset_count(&self) -> u8 {
        if self.side == 3 { 5 } else { 4 }
    }
    pub fn set_side(&mut self, side: u32) {
        let selected = self.selected.min((side * side - 1) as usize);
        *self = Self::with_side(self.preset.min(3), side);
        self.selected = selected;
    }
    pub fn import_legacy(&mut self, points: &[ControlPoint], tangents: &[[MeshTangent; 2]]) {
        let incoming: Vec<_> = points
            .iter()
            .map(|p| {
                BrushMeshPoint::new(
                    p.position,
                    p.u_tangent,
                    p.v_tangent,
                    [p.color[0], p.color[1], p.color[2], 1.],
                )
            })
            .collect();
        if self.points != incoming {
            self.points = incoming;
            self.edited = true;
            self.dirty = true;
        }
        self.tangents = tangents.to_vec();
    }
    pub fn legacy_points(&self) -> Vec<ControlPoint> {
        self.points
            .iter()
            .map(|p| ControlPoint {
                position: p.position,
                u_tangent: p.u_tangent,
                v_tangent: p.v_tangent,
                color: [p.color[0], p.color[1], p.color[2]],
            })
            .collect()
    }
    pub fn load_preset(&mut self, preset: u8) {
        let selected = self.selected;
        *self = Self::with_side(preset, self.side);
        self.selected = selected;
    }
    pub fn reset_selected(&mut self) {
        let point = preset_points(self.preset, self.side)[self.selected];
        self.points[self.selected] = point;
        self.tangents[self.selected] = [
            MeshTangent::from_vector(point.u_tangent),
            MeshTangent::from_vector(point.v_tangent),
        ];
        self.edited = true;
        self.dirty = true;
    }
    pub fn value(&self, control: Control) -> f32 {
        let p = self.points[self.selected];
        let t = self.tangents[self.selected];
        match control {
            Control::HermiteX => p.position[0],
            Control::HermiteY => p.position[1],
            Control::HermiteUAngle => t[0].angle,
            Control::HermiteULength => t[0].length,
            Control::HermiteVAngle => t[1].angle,
            Control::HermiteVLength => t[1].length,
            _ => unreachable!("Hermite control"),
        }
    }
    pub fn set(&mut self, control: Control, value: f32) {
        let (min, max, _) = control.range();
        let value = value.clamp(min, max);
        if value == self.value(control) {
            return;
        }
        let p = &mut self.points[self.selected];
        match control {
            Control::HermiteX => p.position[0] = value,
            Control::HermiteY => p.position[1] = value,
            _ => {
                let axis = usize::from(matches!(
                    control,
                    Control::HermiteVAngle | Control::HermiteVLength
                ));
                let t = &mut self.tangents[self.selected][axis];
                if matches!(control, Control::HermiteUAngle | Control::HermiteVAngle) {
                    t.angle = value;
                } else {
                    t.length = value;
                }
                if axis == 0 {
                    p.u_tangent = t.vector();
                } else {
                    p.v_tangent = t.vector();
                }
            }
        }
        self.edited = true;
        self.dirty = true;
    }
    /// The points as an xgpui-curve lattice with their explicit tangents. Its
    /// edges are the exact patch boundaries both previews render.
    pub fn lattice(&self) -> MeshLattice {
        let side = self.side as usize;
        let to = |v: [f32; 2]| Vec2::new(v[0] as f64, v[1] as f64);
        let positions = self
            .points
            .iter()
            .map(|p| KPoint::new(p.position[0] as f64, p.position[1] as f64))
            .collect();
        let mut lattice = MeshLattice::new(side, side, positions).expect("finite editor mesh");
        lattice.set_tangent_limit(NO_CLAMP).expect("positive limit");
        for (i, p) in self.points.iter().enumerate() {
            let (row, col) = (i / side, i % side);
            for (axis, t) in [(Axis::U, p.u_tangent), (Axis::V, p.v_tangent)] {
                lattice
                    .set_tangent(row, col, axis, Tangent::Explicit(to(t)))
                    .expect("finite tangent");
            }
        }
        lattice
    }
    /// Every patch edge, in normalized 0..1 coordinates.
    pub fn wireframe(&self) -> BezPath {
        self.lattice().wireframe()
    }
    /// Start dragging the wire nearest `at` (normalized), if one is within
    /// `tolerance`. Returns whether a wire was grabbed.
    pub fn begin_wire(&mut self, at: KPoint, tolerance: f64) -> bool {
        self.wire = self.lattice().hit_edge(at, tolerance).map(|hit| WireDrag {
            row: hit.row,
            col: hit.col,
            axis: hit.axis,
            t: hit.t,
            last: at,
        });
        self.wire.is_some()
    }
    /// Move the grabbed point of the wire to `at`: both end tangents change so
    /// the point follows the pointer exactly.
    pub fn drag_wire(&mut self, at: KPoint) {
        let Some(drag) = self.wire else {
            return;
        };
        let mut lattice = self.lattice();
        if lattice
            .drag_edge(drag.row, drag.col, drag.axis, drag.t, at - drag.last)
            .is_err()
        {
            return;
        }
        let side = self.side as usize;
        let (r1, c1) = match drag.axis {
            Axis::U => (drag.row, drag.col + 1),
            Axis::V => (drag.row + 1, drag.col),
        };
        for (row, col) in [(drag.row, drag.col), (r1, c1)] {
            let i = row * side + col;
            let m = lattice.resolved_tangent(row, col, drag.axis);
            let v = [m.x as f32, m.y as f32];
            let slot = usize::from(drag.axis == Axis::V);
            if slot == 0 {
                self.points[i].u_tangent = v;
            } else {
                self.points[i].v_tangent = v;
            }
            self.tangents[i][slot] = MeshTangent::from_vector(v);
        }
        self.wire = Some(WireDrag { last: at, ..drag });
        self.edited = true;
        self.dirty = true;
    }
    pub fn end_wire(&mut self) {
        self.wire = None;
    }
    /// Coalesce input events into one material rebuild per rendered frame.
    pub fn refresh(&mut self) {
        if !self.dirty {
            return;
        }
        self.legacy = Arc::new(
            self.legacy
                .with_grid(grid(&self.points, self.side))
                .expect("bounded editor mesh"),
        );
        self.brush = Brush::hermite_mesh(self.side, self.side, self.points.clone())
            .map_err(|e| e.to_string());
        self.dirty = false;
    }
}
fn grid(points: &[BrushMeshPoint], side: u32) -> MeshGrid {
    MeshGrid::new(
        side,
        side,
        points
            .iter()
            .map(|p| ControlPoint {
                position: p.position,
                u_tangent: p.u_tangent,
                v_tangent: p.v_tangent,
                color: [p.color[0], p.color[1], p.color[2]],
            })
            .collect(),
    )
    .expect("finite editor mesh")
}
fn preset_points(kind: u8, side: u32) -> Vec<BrushMeshPoint> {
    let colors = [
        [1., 0.05, 0.1, 1.],
        [0.05, 0.2, 1., 1.],
        [0., 0.9, 0.5, 1.],
        [1., 0.65, 0., 1.],
    ];
    let mut points: Vec<_> = if side == 3 {
        super::grid(0.)
            .unwrap()
            .points()
            .iter()
            .map(|p| {
                BrushMeshPoint::new(
                    p.position,
                    p.u_tangent,
                    p.v_tangent,
                    [p.color[0], p.color[1], p.color[2], 1.],
                )
            })
            .collect()
    } else {
        (0..4)
            .map(|i| {
                BrushMeshPoint::new(
                    [(i % 2) as f32, (i / 2) as f32],
                    [1., 0.],
                    [0., 1.],
                    colors[i],
                )
            })
            .collect()
    };
    let step = 1. / (side - 1) as f32;
    match kind {
        1 => {
            points[0].u_tangent[1] = 0.2 * step;
            points[(side - 1) as usize].u_tangent[1] = -0.2 * step;
        }
        2 => {
            points[0].v_tangent[0] = 0.2 * step;
            points[(side * (side - 1)) as usize].v_tangent[0] = -0.2 * step;
        }
        3 => {
            for p in &mut points {
                p.position = p.position.map(|v| 0.1 + v * 0.8);
                p.u_tangent = [step * 0.8, 0.];
                p.v_tangent = [0., step * 0.8];
            }
        }
        4 if side == 3 => points[4].position = [0.53, 0.47],
        _ => {}
    }
    points
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wires_are_the_patch_edges_and_dragging_one_follows_the_pointer() {
        let mut editor = HermiteEditor::new(1);
        editor.set_side(3);
        editor.load_preset(4);
        // Edge (row 0, col 0) along U is the Hermite curve of its two nodes.
        let lattice = editor.lattice();
        let edge = lattice.edge(0, 0, Axis::U);
        let (a, b) = (editor.points[0], editor.points[1]);
        let h = |t: f64| {
            let (p0, p1) = (a.position.map(|v| v as f64), b.position.map(|v| v as f64));
            let (m0, m1) = (a.u_tangent.map(|v| v as f64), b.u_tangent.map(|v| v as f64));
            let (t2, t3) = (t * t, t * t * t);
            [0, 1].map(|k| {
                (2. * t3 - 3. * t2 + 1.) * p0[k]
                    + (t3 - 2. * t2 + t) * m0[k]
                    + (-2. * t3 + 3. * t2) * p1[k]
                    + (t3 - t2) * m1[k]
            })
        };
        use xgpui_curve::kurbo::ParamCurve;
        for t in [0.25, 0.5, 0.75] {
            let e = edge.eval(t);
            let r = h(t);
            assert!((e.x - r[0]).abs() < 1e-6 && (e.y - r[1]).abs() < 1e-6);
        }
        assert_eq!(editor.wireframe().segments().count(), 3 * 2 + 3 * 2);
        // Grab the middle of the top-left U wire and pull it down.
        let start = edge.eval(0.5);
        assert!(editor.begin_wire(start, 0.02));
        let target = start + Vec2::new(0., 0.04);
        editor.drag_wire(target);
        editor.end_wire();
        let moved = editor.lattice().edge(0, 0, Axis::U).eval(0.5);
        assert!((moved - target).hypot() < 1e-5, "{moved:?} vs {target:?}");
        // Both end tangents changed, and the polar controls follow them.
        assert_ne!(editor.points[0].u_tangent, a.u_tangent);
        assert_ne!(editor.points[1].u_tangent, b.u_tangent);
        let polar = editor.tangents[0][0].vector();
        assert!((polar[0] - editor.points[0].u_tangent[0]).abs() < 1e-5);
        assert!(editor.edited && editor.dirty);
        assert!(!editor.begin_wire(KPoint::new(5., 5.), 0.02));
    }
    #[test]
    fn rejected_edits_keep_legacy_current_and_recover_without_a_stale_direct_brush() {
        let mut editor = HermiteEditor::new(0);
        editor.set(Control::HermiteUAngle, 180.);
        editor.refresh();
        assert!(editor.brush.is_err());
        assert_eq!(editor.legacy.grid().points()[0].u_tangent, [-1., 0.]);
        editor.set(Control::HermiteUAngle, 0.);
        editor.refresh();
        assert!(editor.brush.is_ok());
        assert_eq!(editor.legacy.grid().points()[0].u_tangent, [1., 0.]);
    }
    #[test]
    fn zero_length_preserves_direction_and_selected_reset_preserves_other_edits() {
        let mut editor = HermiteEditor::new(0);
        editor.set(Control::HermiteULength, 0.);
        editor.set(Control::HermiteUAngle, 10.);
        editor.selected = 1;
        editor.set(Control::HermiteVLength, 0.9);
        editor.reset_selected();
        editor.selected = 0;
        assert_eq!(editor.value(Control::HermiteUAngle), 10.);
        assert_eq!(editor.value(Control::HermiteULength), 0.);
        editor.set(Control::HermiteULength, 1.);
        editor.refresh();
        assert!(editor.brush.is_ok());
        assert!(editor.points[0].u_tangent[1] > 0.17);
        assert_eq!(editor.points[1], preset_points(0, 2)[1]);
        editor.load_preset(3);
        assert!(!editor.edited);
        assert_eq!(editor.points, preset_points(3, 2));
    }
    #[test]
    fn three_by_three_presets_and_legacy_roundtrip_keep_shared_points() {
        let mut editor = HermiteEditor::new(0);
        editor.set_side(3);
        for preset in 0..editor.preset_count() {
            editor.load_preset(preset);
            assert!(
                editor.brush.is_ok(),
                "preset {preset}: {:?}",
                editor.brush.as_ref().err()
            );
            assert_eq!(editor.points.len(), 9);
        }
        editor.selected = 4;
        editor.set(Control::HermiteX, 0.525);
        editor.refresh();
        assert!(editor.brush.is_ok());
        let points = editor.legacy_points();
        assert_eq!(points[4].position, editor.points[4].position);
        let mut imported = HermiteEditor::new(0);
        imported.set_side(3);
        imported.import_legacy(&points, &editor.tangents);
        imported.refresh();
        assert_eq!(imported.points, editor.points);
        assert_eq!(imported.legacy.grid().points(), points.as_slice());
        assert!(imported.brush.is_ok());
        editor.set_side(2);
        assert!(editor.selected < 4);
        assert_eq!(editor.points.len(), 4);
    }
}
