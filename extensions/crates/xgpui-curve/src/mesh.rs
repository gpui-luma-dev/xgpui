//! Hermite mesh lattices: an R × C grid of points with per-node, per-axis
//! tangents, whose edges are the cubic boundary curves of the mesh gradient
//! patches. Each cell is the zero-twist bicubic Hermite (Ferguson) patch that
//! xgpui-shader's mesh shaders render, so edges, interior points and
//! iso-curves all lie on the drawn gradient.
//!
//! Automatic tangents and their fold clamp reproduce the xgpui-luma gradient
//! builder (`paint/mesh.rs`) and the tangents it hands to
//! `xgpui_shader::MeshGrid`, so wireframes drawn from a lattice lie exactly on
//! the rendered patch edges.

use crate::error::{CurveError, finite};
use crate::geom::{self, BezPath, CubicBez, Hermite, ParamCurve, PathSeg, Point, Vec2};

/// Default fold clamp: a tangent may be at most this fraction of the distance
/// to the nearer neighbor along its axis (the lab's `GEOMETRY_TANGENT_LIMIT`).
pub const DEFAULT_TANGENT_LIMIT: f64 = 0.7;

/// A node tangent along one axis.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[non_exhaustive]
pub enum Tangent {
    /// Central difference of the neighbors (one-sided at the border).
    #[default]
    Auto,
    /// A user-set derivative.
    Explicit(Vec2),
}

/// Lattice direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Axis {
    /// Along a row: from column `c` to `c + 1`.
    U,
    /// Along a column: from row `r` to `r + 1`.
    V,
}

/// The four boundary curves of one cell, oriented like the lab's `MeshCell`:
/// top and bottom run in +u, left and right in +v.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CellBoundary {
    /// Row `r`, columns `c → c + 1`.
    pub top: CubicBez,
    /// Row `r + 1`, columns `c → c + 1`.
    pub bottom: CubicBez,
    /// Column `c`, rows `r → r + 1`.
    pub left: CubicBez,
    /// Column `c + 1`, rows `r → r + 1`.
    pub right: CubicBez,
}

/// Position and resolved tangents of one node, in `f32` for GPU meshes
/// (field-for-field the geometry of `xgpui_shader::ControlPoint`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshControl {
    /// Node position.
    pub position: [f32; 2],
    /// Resolved u tangent.
    pub u_tangent: [f32; 2],
    /// Resolved v tangent.
    pub v_tangent: [f32; 2],
}

/// The nearest wire to a point.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct EdgeHit {
    /// Start row of the edge.
    pub row: usize,
    /// Start column of the edge.
    pub col: usize,
    /// Edge direction.
    pub axis: Axis,
    /// Parameter on the edge.
    pub t: f64,
    /// Distance to the wire.
    pub distance: f64,
}

/// An R × C Hermite lattice.
#[derive(Clone, Debug, PartialEq)]
pub struct MeshLattice {
    rows: usize,
    cols: usize,
    points: Vec<Point>,
    u: Vec<Tangent>,
    v: Vec<Tangent>,
    tangent_limit: f64,
}

impl MeshLattice {
    /// A lattice from row-major `points`, with automatic tangents. Needs at
    /// least 2 × 2 nodes.
    pub fn new(rows: usize, cols: usize, points: Vec<Point>) -> Result<Self, CurveError> {
        if rows < 2 || cols < 2 || points.len() != rows * cols {
            return Err(CurveError::Dimensions);
        }
        for p in &points {
            finite(&[p.x, p.y])?;
        }
        let n = points.len();
        Ok(Self {
            rows,
            cols,
            points,
            u: vec![Tangent::Auto; n],
            v: vec![Tangent::Auto; n],
            tangent_limit: DEFAULT_TANGENT_LIMIT,
        })
    }

    /// Row count.
    pub fn rows(&self) -> usize {
        self.rows
    }

    /// Column count.
    pub fn cols(&self) -> usize {
        self.cols
    }

    fn index(&self, row: usize, col: usize) -> usize {
        assert!(
            row < self.rows && col < self.cols,
            "mesh node ({row}, {col}) out of range"
        );
        row * self.cols + col
    }

    fn checked(&self, row: usize, col: usize) -> Result<usize, CurveError> {
        if row < self.rows && col < self.cols {
            Ok(row * self.cols + col)
        } else {
            Err(CurveError::OutOfRange("mesh node"))
        }
    }

    /// Node position.
    pub fn point(&self, row: usize, col: usize) -> Point {
        self.points[self.index(row, col)]
    }

    /// Move a node.
    pub fn set_point(&mut self, row: usize, col: usize, p: Point) -> Result<(), CurveError> {
        let i = self.checked(row, col)?;
        finite(&[p.x, p.y])?;
        self.points[i] = p;
        Ok(())
    }

    /// The stored (unresolved) tangent.
    pub fn tangent(&self, row: usize, col: usize, axis: Axis) -> Tangent {
        let i = self.index(row, col);
        match axis {
            Axis::U => self.u[i],
            Axis::V => self.v[i],
        }
    }

    /// Store a tangent.
    pub fn set_tangent(
        &mut self,
        row: usize,
        col: usize,
        axis: Axis,
        tangent: Tangent,
    ) -> Result<(), CurveError> {
        let i = self.checked(row, col)?;
        if let Tangent::Explicit(m) = tangent {
            finite(&[m.x, m.y])?;
        }
        match axis {
            Axis::U => self.u[i] = tangent,
            Axis::V => self.v[i] = tangent,
        }
        Ok(())
    }

    /// Reset every tangent to [`Tangent::Auto`].
    pub fn reset_tangents(&mut self) {
        self.u.fill(Tangent::Auto);
        self.v.fill(Tangent::Auto);
    }

    /// Fold clamp factor.
    pub fn tangent_limit(&self) -> f64 {
        self.tangent_limit
    }

    /// Set the fold clamp factor (positive and finite).
    pub fn set_tangent_limit(&mut self, limit: f64) -> Result<(), CurveError> {
        if !limit.is_finite() || limit <= 0. {
            return Err(CurveError::OutOfRange("tangent limit"));
        }
        self.tangent_limit = limit;
        Ok(())
    }

    fn neighbors(
        &self,
        row: usize,
        col: usize,
        axis: Axis,
    ) -> (Option<Point>, Point, Option<Point>) {
        let p = self.point(row, col);
        match axis {
            Axis::U => (
                (col > 0).then(|| self.point(row, col - 1)),
                p,
                (col + 1 < self.cols).then(|| self.point(row, col + 1)),
            ),
            Axis::V => (
                (row > 0).then(|| self.point(row - 1, col)),
                p,
                (row + 1 < self.rows).then(|| self.point(row + 1, col)),
            ),
        }
    }

    /// The longest tangent the fold clamp allows at a node.
    fn max_tangent(&self, row: usize, col: usize, axis: Axis) -> f64 {
        let (prev, p, next) = self.neighbors(row, col, axis);
        let nearer = match (prev, next) {
            (None, Some(n)) => p.distance(n),
            (Some(b), None) => p.distance(b),
            (Some(b), Some(n)) => p.distance(b).min(p.distance(n)),
            (None, None) => 0.,
        };
        nearer * self.tangent_limit
    }

    /// The tangent actually used: automatic or explicit, then clamped to
    /// [`Self::tangent_limit`] × the distance to the nearer neighbor along the
    /// axis, which keeps patches from folding.
    pub fn resolved_tangent(&self, row: usize, col: usize, axis: Axis) -> Vec2 {
        let (prev, p, next) = self.neighbors(row, col, axis);
        let raw = match self.tangent(row, col, axis) {
            Tangent::Explicit(m) => m,
            Tangent::Auto => match (prev, next) {
                (None, Some(n)) => n - p,
                (Some(b), None) => p - b,
                (Some(b), Some(n)) => (n - b) * 0.5,
                (None, None) => Vec2::ZERO,
            },
        };
        clamp_length(raw, self.max_tangent(row, col, axis))
    }

    /// The edge from node (row, col) one step along `axis`.
    ///
    /// Panics if either end node is outside the lattice (`col + 1 == cols`
    /// for [`Axis::U`], `row + 1 == rows` for [`Axis::V`]).
    pub fn edge(&self, row: usize, col: usize, axis: Axis) -> CubicBez {
        let (r1, c1) = match axis {
            Axis::U => (row, col + 1),
            Axis::V => (row + 1, col),
        };
        Hermite {
            p0: self.point(row, col),
            m0: self.resolved_tangent(row, col, axis),
            p1: self.point(r1, c1),
            m1: self.resolved_tangent(r1, c1, axis),
        }
        .to_cubic()
    }

    /// The boundary curves of cell (row, col), `row < rows − 1`, `col < cols − 1`.
    pub fn cell_boundary(&self, row: usize, col: usize) -> CellBoundary {
        CellBoundary {
            top: self.edge(row, col, Axis::U),
            bottom: self.edge(row + 1, col, Axis::U),
            left: self.edge(row, col, Axis::V),
            right: self.edge(row, col + 1, Axis::V),
        }
    }

    /// Row `row` as one continuous cubic path.
    pub fn row_path(&self, row: usize) -> BezPath {
        let mut path = BezPath::new();
        path.move_to(self.point(row, 0));
        for col in 0..self.cols - 1 {
            geom::push_seg(&mut path, PathSeg::Cubic(self.edge(row, col, Axis::U)));
        }
        path
    }

    /// Column `col` as one continuous cubic path.
    pub fn col_path(&self, col: usize) -> BezPath {
        let mut path = BezPath::new();
        path.move_to(self.point(0, col));
        for row in 0..self.rows - 1 {
            geom::push_seg(&mut path, PathSeg::Cubic(self.edge(row, col, Axis::V)));
        }
        path
    }

    /// Every row and column path.
    pub fn wireframe(&self) -> BezPath {
        let mut path = BezPath::new();
        for row in 0..self.rows {
            path.extend(self.row_path(row));
        }
        for col in 0..self.cols {
            path.extend(self.col_path(col));
        }
        path
    }

    /// The position in cell (row, col) at local (u, v): the zero-twist
    /// bicubic Hermite patch the mesh shaders render, from the four corner
    /// positions and their resolved u and v tangents.
    pub fn patch_point(&self, row: usize, col: usize, u: f64, v: f64) -> Point {
        self.iso_curve(row, col, Axis::U, v).eval(u)
    }

    /// The interior iso-curve of cell (row, col) at constant `s` on the other
    /// axis: `Axis::U` runs along u at v = s, `Axis::V` along v at u = s. A
    /// zero-twist Hermite patch's iso-curve is exactly a cubic: it runs between
    /// the two side edges at `s`, with the end tangents blended by the Hermite
    /// position weights. `s = 0` and `s = 1` give the cell's edges.
    pub fn iso_curve(&self, row: usize, col: usize, axis: Axis, s: f64) -> CubicBez {
        let b = self.cell_boundary(row, col);
        // Corners in (u, v) order, and the tangents along the curve's axis.
        let (r1, c1) = (row + 1, col + 1);
        let (side0, side1, ends) = match axis {
            Axis::U => (
                b.left,
                b.right,
                [(row, col), (r1, col), (row, c1), (r1, c1)],
            ),
            Axis::V => (
                b.top,
                b.bottom,
                [(row, col), (row, c1), (r1, col), (r1, c1)],
            ),
        };
        let m = |(r, c): (usize, usize)| self.resolved_tangent(r, c, axis);
        let (h0, h1) = (1. - s * s * (3. - 2. * s), s * s * (3. - 2. * s));
        Hermite {
            p0: side0.eval(s),
            m0: m(ends[0]) * h0 + m(ends[1]) * h1,
            p1: side1.eval(s),
            m1: m(ends[2]) * h0 + m(ends[3]) * h1,
        }
        .to_cubic()
    }

    /// Row-major positions and resolved tangents for building a GPU mesh.
    pub fn controls(&self) -> Vec<MeshControl> {
        let f = |v: Vec2| [v.x as f32, v.y as f32];
        (0..self.rows)
            .flat_map(|r| (0..self.cols).map(move |c| (r, c)))
            .map(|(r, c)| {
                let p = self.point(r, c);
                MeshControl {
                    position: [p.x as f32, p.y as f32],
                    u_tangent: f(self.resolved_tangent(r, c, Axis::U)),
                    v_tangent: f(self.resolved_tangent(r, c, Axis::V)),
                }
            })
            .collect()
    }

    /// Drag the point at `t` on an edge by `delta`, moving the two inner
    /// controls by the minimum-norm solve so B(t) follows the cursor exactly:
    /// Δc₁ = w₁/(w₁² + w₂²)·Δ, Δc₂ = w₂/(w₁² + w₂²)·Δ with w₁ = 3t(1 − t)²,
    /// w₂ = 3t²(1 − t). The result is stored as explicit tangents at both
    /// nodes; a node's tangent is shared by its two edges on that axis, so the
    /// neighboring edge bends too. When the fold clamp shortens one tangent,
    /// the other control takes the rest of the move; only when both are at
    /// the clamp does the drag stop short. Endpoints (t ≈ 0 or 1) move no
    /// controls.
    pub fn drag_edge(
        &mut self,
        row: usize,
        col: usize,
        axis: Axis,
        t: f64,
        delta: Vec2,
    ) -> Result<(), CurveError> {
        // The start node is checked first, so `+ 1` cannot overflow.
        self.checked(row, col)?;
        let (r1, c1) = match axis {
            Axis::U => (row, col + 1),
            Axis::V => (row + 1, col),
        };
        self.checked(r1, c1)?;
        finite(&[t, delta.x, delta.y])?;
        let t = t.clamp(0., 1.);
        let w1 = 3. * t * (1. - t) * (1. - t);
        let w2 = 3. * t * t * (1. - t);
        let norm = w1 * w1 + w2 * w2;
        if norm <= 1e-12 {
            return Ok(());
        }
        // c₁ = p₀ + m₀/3 and c₂ = p₁ − m₁/3, and B(t) moves by w₁Δc₁ + w₂Δc₂.
        let (base0, base1) = (
            self.resolved_tangent(row, col, axis),
            self.resolved_tangent(r1, c1, axis),
        );
        let (max0, max1) = (
            self.max_tangent(row, col, axis),
            self.max_tangent(r1, c1, axis),
        );
        let want0 = base0 + delta * (3. * w1 / norm);
        let want1 = base1 - delta * (3. * w2 / norm);
        let mut m0 = clamp_length(want0, max0);
        let mut m1 = clamp_length(want1, max1);
        // If the clamp shortened one tangent, give the displacement it could
        // not take to the other control (if both are clamped, stop short).
        let missing = delta - (m0 - base0) * (w1 / 3.) - (base1 - m1) * (w2 / 3.);
        let (clamped0, clamped1) = (m0 != want0, m1 != want1);
        if clamped0 && !clamped1 && w2 > 1e-12 {
            m1 = clamp_length(m1 - missing * (3. / w2), max1);
        } else if clamped1 && !clamped0 && w1 > 1e-12 {
            m0 = clamp_length(m0 + missing * (3. / w1), max0);
        }
        self.set_tangent(row, col, axis, Tangent::Explicit(m0))?;
        self.set_tangent(r1, c1, axis, Tangent::Explicit(m1))
    }

    /// The nearest edge to `p` within `max_distance`.
    pub fn hit_edge(&self, p: Point, max_distance: f64) -> Option<EdgeHit> {
        let mut best: Option<EdgeHit> = None;
        let mut consider = |row, col, axis, edge: CubicBez| {
            let hit = geom::nearest(PathSeg::Cubic(edge), p);
            if hit.distance <= max_distance && best.is_none_or(|b| hit.distance < b.distance) {
                best = Some(EdgeHit {
                    row,
                    col,
                    axis,
                    t: hit.t,
                    distance: hit.distance,
                });
            }
        };
        for row in 0..self.rows {
            for col in 0..self.cols {
                if col + 1 < self.cols {
                    consider(row, col, Axis::U, self.edge(row, col, Axis::U));
                }
                if row + 1 < self.rows {
                    consider(row, col, Axis::V, self.edge(row, col, Axis::V));
                }
            }
        }
        best
    }
}

fn clamp_length(v: Vec2, max: f64) -> Vec2 {
    let length = v.hypot();
    if length <= f64::EPSILON || length <= max {
        v
    } else {
        v * (max / length)
    }
}

/// The lab's color-channel tangent: zero at a local extremum or flat side,
/// otherwise the mean of the adjacent differences clamped to the smaller one.
pub fn monotone_channel_tangent(prev_delta: f64, next_delta: f64) -> f64 {
    if prev_delta.abs() <= f64::EPSILON
        || next_delta.abs() <= f64::EPSILON
        || prev_delta.signum() != next_delta.signum()
    {
        0.
    } else {
        let limit = prev_delta.abs().min(next_delta.abs());
        ((prev_delta + next_delta) * 0.5).clamp(-limit, limit)
    }
}

#[cfg(test)]
#[path = "mesh_tests.rs"]
mod tests;
