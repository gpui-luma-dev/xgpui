//! An editable vector path: anchors with optional handles, per-node
//! continuity modes, and edits that keep each mode's constraint.

use crate::error::{CurveError, finite};
use crate::geom::{self, BezPath, CubicBez, ParamCurve, PathSeg, Point, Vec2};

/// How automatic nodes compute their handles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AutoSmooth {
    /// Catmull-Rom from the two neighboring anchors; `alpha` 0.5 is
    /// centripetal. G¹ (C¹ in its own parameterization). Local: a node depends
    /// only on its neighbors.
    CatmullRom {
        /// Parameterization exponent in `0..=1`.
        alpha: f64,
    },
    /// Natural cubic spline (C²) over each run of consecutive `Natural` nodes,
    /// with natural end conditions.
    Natural,
    /// Hobby's algorithm (unit tension, unit curl ends) over each run of
    /// consecutive `Hobby` nodes: visually even curvature.
    Hobby,
}

/// A node's continuity mode.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NodeMode {
    /// C⁰: handles move independently.
    Corner,
    /// G¹: handles stay collinear and opposite; their lengths are independent.
    Smooth,
    /// Handles stay collinear, opposite and equal in length.
    Symmetric,
    /// Handles are computed from the anchors.
    Auto(AutoSmooth),
}

/// Which handle of a node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// Toward the previous node.
    In,
    /// Toward the next node.
    Out,
}

/// One anchor. A handle of `None` coincides with the anchor; a segment with
/// no handles at either end is a straight line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Node {
    /// Anchor position.
    pub anchor: Point,
    /// Incoming handle.
    pub handle_in: Option<Point>,
    /// Outgoing handle.
    pub handle_out: Option<Point>,
    /// Continuity mode.
    pub mode: NodeMode,
}

impl Node {
    /// A corner node with no handles.
    pub fn corner(anchor: Point) -> Self {
        Self {
            anchor,
            handle_in: None,
            handle_out: None,
            mode: NodeMode::Corner,
        }
    }

    /// An automatic node.
    pub fn auto(anchor: Point, smooth: AutoSmooth) -> Self {
        Self {
            anchor,
            handle_in: None,
            handle_out: None,
            mode: NodeMode::Auto(smooth),
        }
    }

    fn handle(&self, side: Side) -> Option<Point> {
        match side {
            Side::In => self.handle_in,
            Side::Out => self.handle_out,
        }
    }

    fn set_handle(&mut self, side: Side, p: Option<Point>) {
        match side {
            Side::In => self.handle_in = p,
            Side::Out => self.handle_out = p,
        }
    }
}

/// An editable path of nodes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NodePath {
    nodes: Vec<Node>,
    closed: bool,
}

impl NodePath {
    /// A path from nodes; automatic handles are computed immediately.
    pub fn new(nodes: Vec<Node>, closed: bool) -> Result<Self, CurveError> {
        for n in &nodes {
            let mut v = vec![n.anchor.x, n.anchor.y];
            for h in [n.handle_in, n.handle_out].into_iter().flatten() {
                v.extend([h.x, h.y]);
            }
            finite(&v)?;
        }
        let mut path = Self { nodes, closed };
        path.update_auto();
        Ok(path)
    }

    /// The nodes.
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    /// Whether the path is closed.
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// Number of segments.
    pub fn segment_count(&self) -> usize {
        match self.nodes.len() {
            0 | 1 => 0,
            n if self.closed => n,
            n => n - 1,
        }
    }

    fn next(&self, i: usize) -> usize {
        (i + 1) % self.nodes.len()
    }

    /// Segment `i` (node `i` to node `i + 1`, wrapping when closed).
    pub fn segment(&self, i: usize) -> PathSeg {
        let (a, b) = (self.nodes[i], self.nodes[self.next(i)]);
        match (a.handle_out, b.handle_in) {
            (None, None) => PathSeg::Line(geom::Line::new(a.anchor, b.anchor)),
            (h1, h2) => PathSeg::Cubic(CubicBez::new(
                a.anchor,
                h1.unwrap_or(a.anchor),
                h2.unwrap_or(b.anchor),
                b.anchor,
            )),
        }
    }

    /// The path as a kurbo path.
    pub fn to_bezpath(&self) -> BezPath {
        let mut path = BezPath::new();
        let Some(first) = self.nodes.first() else {
            return path;
        };
        path.move_to(first.anchor);
        for i in 0..self.segment_count() {
            geom::push_seg(&mut path, self.segment(i));
        }
        if self.closed && self.nodes.len() > 1 {
            path.close_path();
        }
        path
    }

    /// Curvature comb: for each sample, the curve point and that point offset
    /// along the normal by κ · `scale`.
    pub fn curvature_comb(&self, samples_per_segment: usize, scale: f64) -> Vec<(Point, Point)> {
        let n = samples_per_segment.max(1);
        let mut out = Vec::new();
        for i in 0..self.segment_count() {
            let seg = self.segment(i);
            for k in 0..=n {
                let t = k as f64 / n as f64;
                let p = seg.eval(t);
                let tip = geom::normal(seg, t)
                    .map_or(p, |nrm| p + nrm * (geom::curvature(seg, t) * scale));
                out.push((p, tip));
            }
        }
        out
    }

    /// Move an anchor, carrying its handles with it.
    pub fn move_anchor(&mut self, i: usize, to: Point) -> Result<(), CurveError> {
        finite(&[to.x, to.y])?;
        let node = &mut self.nodes[i];
        let delta = to - node.anchor;
        node.anchor = to;
        node.handle_in = node.handle_in.map(|h| h + delta);
        node.handle_out = node.handle_out.map(|h| h + delta);
        self.update_auto();
        Ok(())
    }

    /// Move one handle; the other handle follows the node's constraint.
    /// Moving a handle of an automatic node turns it into a `Smooth` node.
    pub fn move_handle(&mut self, i: usize, side: Side, to: Point) -> Result<(), CurveError> {
        finite(&[to.x, to.y])?;
        let node = &mut self.nodes[i];
        if matches!(node.mode, NodeMode::Auto(_)) {
            node.mode = NodeMode::Smooth;
        }
        node.set_handle(side, Some(to));
        constrain(node, side);
        self.update_auto();
        Ok(())
    }

    /// Change a node's mode and apply its constraint (the outgoing handle
    /// leads when both exist).
    pub fn set_mode(&mut self, i: usize, mode: NodeMode) {
        let node = &mut self.nodes[i];
        node.mode = mode;
        let lead = if node.handle_out.is_some() {
            Side::Out
        } else {
            Side::In
        };
        constrain(node, lead);
        self.update_auto();
    }

    /// Drag the point at `t` on segment `i` by `delta`: the segment's inner
    /// controls move by the minimum-norm solve, so B(t) follows the cursor
    /// exactly. Each end node's constraint then adjusts its other handle, and
    /// automatic end nodes become `Smooth`.
    pub fn drag_segment(&mut self, i: usize, t: f64, delta: Vec2) -> Result<(), CurveError> {
        finite(&[t, delta.x, delta.y])?;
        let t = t.clamp(0., 1.);
        let w1 = 3. * t * (1. - t) * (1. - t);
        let w2 = 3. * t * t * (1. - t);
        let norm = w1 * w1 + w2 * w2;
        if norm <= 1e-12 {
            return Ok(());
        }
        let j = self.next(i);
        let c = geom::to_cubic(self.segment(i));
        let (c1, c2) = (c.p1 + delta * (w1 / norm), c.p2 + delta * (w2 / norm));
        for (k, side, p) in [(i, Side::Out, c1), (j, Side::In, c2)] {
            let node = &mut self.nodes[k];
            if matches!(node.mode, NodeMode::Auto(_)) {
                node.mode = NodeMode::Smooth;
            }
            node.set_handle(side, Some(p));
            constrain(node, side);
        }
        self.update_auto();
        Ok(())
    }

    /// Split segment `i` at `t` with a new `Smooth` node, leaving the drawn
    /// curve unchanged. Automatic nodes whose handles depend on the split
    /// segment are frozen as `Smooth` nodes with their current handles.
    /// Returns the new node's index.
    pub fn insert_at(&mut self, i: usize, t: f64) -> Result<usize, CurveError> {
        finite(&[t])?;
        let t = t.clamp(0., 1.);
        let j = self.next(i);
        for k in self.auto_dependents(i, j) {
            self.nodes[k].mode = NodeMode::Smooth;
        }
        let seg = self.segment(i);
        let new = if let PathSeg::Line(line) = seg {
            Node::corner(line.eval(t))
        } else {
            let c = geom::to_cubic(seg);
            let (left, right) = (c.subsegment(0. ..t), c.subsegment(t..1.));
            self.nodes[i].handle_out = Some(left.p1);
            self.nodes[j].handle_in = Some(right.p2);
            Node {
                anchor: left.p3,
                handle_in: Some(left.p2),
                handle_out: Some(right.p1),
                mode: NodeMode::Smooth,
            }
        };
        let at = i + 1;
        self.nodes.insert(at, new);
        Ok(at)
    }

    /// Remove a node.
    pub fn delete_node(&mut self, i: usize) {
        self.nodes.remove(i);
        self.update_auto();
    }

    /// Open or close the path.
    pub fn set_closed(&mut self, closed: bool) {
        self.closed = closed;
        self.update_auto();
    }

    /// Nodes (indices) whose automatic handles would change if a node were
    /// inserted between `i` and `j`.
    fn auto_dependents(&self, i: usize, j: usize) -> Vec<usize> {
        let mut out = Vec::new();
        for run in self.auto_runs() {
            if run.indices.contains(&i) || run.indices.contains(&j) {
                match run.kind {
                    AutoSmooth::CatmullRom { .. } => {
                        out.extend([i, j].into_iter().filter(|k| run.indices.contains(k)))
                    }
                    _ => out.extend(run.indices.iter().copied()),
                }
            }
        }
        out
    }

    fn auto_kind(&self, i: usize) -> Option<AutoSmooth> {
        match self.nodes[i].mode {
            NodeMode::Auto(kind) => Some(kind),
            _ => None,
        }
    }

    /// Maximal runs of consecutive automatic nodes of the same kind
    /// (Catmull-Rom nodes each form their own run; they are local).
    fn auto_runs(&self) -> Vec<Run> {
        let n = self.nodes.len();
        let same = |a: Option<AutoSmooth>, b: Option<AutoSmooth>| {
            matches!(
                (a, b),
                (Some(AutoSmooth::Natural), Some(AutoSmooth::Natural))
                    | (Some(AutoSmooth::Hobby), Some(AutoSmooth::Hobby))
            )
        };
        let mut runs = Vec::new();
        if n == 0 {
            return runs;
        }
        if self.closed && (0..n).all(|i| same(self.auto_kind(i), self.auto_kind(0))) {
            runs.push(Run {
                kind: self.auto_kind(0).unwrap(),
                indices: (0..n).collect(),
                cyclic: true,
            });
            return runs;
        }
        // Start after a boundary so wrapping runs stay whole on closed paths.
        let start = if self.closed {
            (0..n)
                .find(|&i| !same(self.auto_kind(i), self.auto_kind((i + n - 1) % n)))
                .unwrap_or(0)
        } else {
            0
        };
        let mut k = 0;
        while k < n {
            let i = (start + k) % n;
            let Some(kind) = self.auto_kind(i) else {
                k += 1;
                continue;
            };
            let mut indices = vec![i];
            k += 1;
            while k < n
                && same(Some(kind), self.auto_kind((start + k) % n))
                && !self.wraps_open(indices[indices.len() - 1])
            {
                indices.push((start + k) % n);
                k += 1;
            }
            runs.push(Run {
                kind,
                indices,
                cyclic: false,
            });
        }
        runs
    }

    fn wraps_open(&self, i: usize) -> bool {
        !self.closed && i + 1 == self.nodes.len()
    }

    /// Recompute every automatic node's handles.
    pub fn update_auto(&mut self) {
        let n = self.nodes.len();
        if n < 2 {
            return;
        }
        for run in self.auto_runs() {
            match run.kind {
                AutoSmooth::CatmullRom { alpha } => {
                    for &i in &run.indices {
                        self.catmull_rom_node(i, alpha.clamp(0., 1.));
                    }
                }
                AutoSmooth::Natural | AutoSmooth::Hobby => self.spline_run(&run),
            }
        }
    }

    fn neighbor(&self, i: usize, forward: bool) -> Option<usize> {
        let n = self.nodes.len();
        if forward {
            if i + 1 < n {
                Some(i + 1)
            } else if self.closed {
                Some(0)
            } else {
                None
            }
        } else if i > 0 {
            Some(i - 1)
        } else if self.closed {
            Some(n - 1)
        } else {
            None
        }
    }

    fn catmull_rom_node(&mut self, i: usize, alpha: f64) {
        let p = self.nodes[i].anchor;
        let prev = self.neighbor(i, false).map(|k| self.nodes[k].anchor);
        let next = self.neighbor(i, true).map(|k| self.nodes[k].anchor);
        let node = &mut self.nodes[i];
        match (prev, next) {
            (Some(a), Some(b)) => {
                let d1 = (p - a).hypot().powf(alpha).max(1e-12);
                let d2 = (b - p).hypot().powf(alpha).max(1e-12);
                // Barry–Goldman tangent in knot units, then per-segment scaling.
                let m = (p - a) / d1 - (b - a) / (d1 + d2) + (b - p) / d2;
                node.handle_in = Some(p - m * (d1 / 3.));
                node.handle_out = Some(p + m * (d2 / 3.));
            }
            _ => {
                node.handle_in = None;
                node.handle_out = None;
            }
        }
    }

    /// Natural or Hobby handles over one run. The run is extended by the
    /// neighboring fixed anchors (if any) so it meets them smoothly.
    fn spline_run(&mut self, run: &Run) {
        let mut ids = run.indices.clone();
        let mut lead = false;
        let mut trail = false;
        if !run.cyclic {
            if let Some(k) = self.neighbor(ids[0], false) {
                ids.insert(0, k);
                lead = true;
            }
            if let Some(k) = self.neighbor(*ids.last().unwrap(), true)
                && !(lead && k == ids[0])
            {
                ids.push(k);
                trail = true;
            }
        }
        let points: Vec<Point> = ids.iter().map(|&k| self.nodes[k].anchor).collect();
        if points.len() < 2 {
            let node = &mut self.nodes[ids[0]];
            node.handle_in = None;
            node.handle_out = None;
            return;
        }
        let controls = match run.kind {
            AutoSmooth::Hobby => hobby(&points, run.cyclic),
            _ => natural(&points, run.cyclic),
        };
        // controls[s] = (c1, c2) for the segment from points[s] to points[s + 1] (cyclic).
        let m = points.len();
        for (pos, &k) in ids.iter().enumerate() {
            let is_fixed = (lead && pos == 0) || (trail && pos == m - 1);
            if is_fixed {
                continue;
            }
            let segs = controls.len();
            let node = &mut self.nodes[k];
            node.handle_out = (pos < segs).then(|| controls[pos].0);
            node.handle_in = if pos > 0 {
                Some(controls[pos - 1].1)
            } else if run.cyclic {
                Some(controls[segs - 1].1)
            } else {
                None
            };
        }
    }
}

struct Run {
    kind: AutoSmooth,
    indices: Vec<usize>,
    cyclic: bool,
}

/// Re-impose a node's constraint after its `moved` handle changed.
fn constrain(node: &mut Node, moved: Side) {
    let other = match moved {
        Side::In => Side::Out,
        Side::Out => Side::In,
    };
    let Some(lead) = node.handle(moved) else {
        return;
    };
    let v = lead - node.anchor;
    if v.hypot() <= 1e-15 {
        return;
    }
    let dir = v.normalize();
    match node.mode {
        NodeMode::Corner | NodeMode::Auto(_) => {}
        NodeMode::Smooth => {
            if let Some(h) = node.handle(other) {
                let length = (h - node.anchor).hypot();
                node.set_handle(other, Some(node.anchor - dir * length));
            }
        }
        NodeMode::Symmetric => node.set_handle(other, Some(node.anchor - v)),
    }
}

/// C² cubic spline controls through `p` (uniform parameterization): open
/// with natural ends, or periodic when `cyclic`.
fn natural(p: &[Point], cyclic: bool) -> Vec<(Point, Point)> {
    let n = p.len();
    let mut d = vec![Vec2::ZERO; n];
    // Solve for derivatives D: D[i−1] + 4D[i] + D[i+1] = 3(P[i+1] − P[i−1]),
    // with 2D[0] + D[1] = 3(P[1] − P[0]) and D[n−2] + 2D[n−1] = 3(P[n−1] − P[n−2]) when open.
    // Diagonally dominant: Gauss–Seidel converges geometrically.
    for _ in 0..200 {
        let mut change: f64 = 0.;
        for i in 0..n {
            let next = if cyclic {
                d[(i + 1) % n]
            } else if i + 1 < n {
                d[i + 1]
            } else {
                Vec2::ZERO
            };
            let prev = if cyclic {
                d[(i + n - 1) % n]
            } else if i > 0 {
                d[i - 1]
            } else {
                Vec2::ZERO
            };
            let value = if cyclic {
                (3. * (p[(i + 1) % n] - p[(i + n - 1) % n]) - prev - next) / 4.
            } else if n == 2 {
                p[1] - p[0]
            } else if i == 0 {
                (3. * (p[1] - p[0]) - next) / 2.
            } else if i == n - 1 {
                (3. * (p[n - 1] - p[n - 2]) - prev) / 2.
            } else {
                (3. * (p[i + 1] - p[i - 1]) - prev - next) / 4.
            };
            change = change.max((value - d[i]).hypot());
            d[i] = value;
        }
        if change <= 1e-15 {
            break;
        }
    }
    let segs = if cyclic { n } else { n - 1 };
    (0..segs)
        .map(|i| (p[i] + d[i] / 3., p[(i + 1) % n] - d[(i + 1) % n] / 3.))
        .collect()
}

/// Hobby's algorithm with unit tension and unit curl ends: choose the angles
/// θ (start) and φ (end) of every segment so linearized curvature is
/// continuous at each interior point, then place controls with Hobby's
/// velocity function.
fn hobby(p: &[Point], cyclic: bool) -> Vec<(Point, Point)> {
    let n = p.len();
    let segs = if cyclic { n } else { n - 1 };
    let chord = |k: usize| p[(k + 1) % n] - p[k];
    let d: Vec<f64> = (0..segs).map(|k| chord(k).hypot().max(1e-12)).collect();
    if segs == 1 {
        let c = chord(0);
        return vec![(p[0] + c / 3., p[1] - c / 3.)];
    }
    // Turning angle at vertex k between chord k−1 and chord k.
    let psi = |k: usize| -> f64 {
        let (a, b) = (chord((k + segs - 1) % segs), chord(k % segs));
        let angle = b.atan2() - a.atan2();
        (angle + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI
    };
    let (theta, phi_end) = if cyclic {
        // Row k: −θ[k−1]/d[k−1] − (2/d[k−1] + 2/d[k])θ[k] − θ[k+1]/d[k] = 2ψ[k]/d[k−1] + ψ[k+1]/d[k].
        let mut a = vec![0.; n];
        let mut b = vec![0.; n];
        let mut c = vec![0.; n];
        let mut r = vec![0.; n];
        for k in 0..n {
            let (dp, dk) = (d[(k + n - 1) % n], d[k]);
            a[k] = -1. / dp;
            b[k] = -2. / dp - 2. / dk;
            c[k] = -1. / dk;
            r[k] = 2. * psi(k) / dp + psi((k + 1) % n) / dk;
        }
        (solve_cyclic_tridiagonal(&a, &b, &c, &r), 0.)
    } else {
        // Unknowns θ[0..segs] and φ at the last point (index segs).
        let m = segs + 1;
        let mut a = vec![0.; m];
        let mut b = vec![0.; m];
        let mut c = vec![0.; m];
        let mut r = vec![0.; m];
        // Curl 1 at the start: θ0 = φ1 = −ψ1 − θ1.
        b[0] = 1.;
        c[0] = 1.;
        r[0] = -psi(1);
        for k in 1..segs {
            let (dp, dk) = (d[k - 1], d[k]);
            a[k] = -1. / dp;
            b[k] = -2. / dp - 2. / dk;
            if k + 1 < segs {
                c[k] = -1. / dk;
                r[k] = 2. * psi(k) / dp + psi(k + 1) / dk;
            } else {
                c[k] = 1. / dk;
                r[k] = 2. * psi(k) / dp;
            }
        }
        // Curl 1 at the end: φn = θ[n−1].
        a[segs] = -1.;
        b[segs] = 1.;
        let x = solve_tridiagonal(&a, &b, &c, &r);
        (x[..segs].to_vec(), x[segs])
    };
    let phi = |k: usize| -> f64 {
        // φ at the end of segment k.
        let end = (k + 1) % n;
        if !cyclic && k + 1 == segs {
            phi_end
        } else {
            -psi(end) - theta[end % segs]
        }
    };
    (0..segs)
        .map(|k| {
            let (th, ph) = (theta[k], phi(k));
            let u = chord(k) / d[k];
            let rot = |v: Vec2, a: f64| {
                Vec2::new(v.x * a.cos() - v.y * a.sin(), v.x * a.sin() + v.y * a.cos())
            };
            (
                p[k] + rot(u, th) * (d[k] * velocity(th, ph)),
                p[(k + 1) % n] - rot(u, -ph) * (d[k] * velocity(ph, th)),
            )
        })
        .collect()
}

/// Hobby's velocity function (includes the 1/3 factor).
fn velocity(theta: f64, phi: f64) -> f64 {
    let (st, sp, ct, cp) = (theta.sin(), phi.sin(), theta.cos(), phi.cos());
    let s5 = 5f64.sqrt();
    let num = 2. + 2f64.sqrt() * (st - sp / 16.) * (sp - st / 16.) * (ct - cp);
    let den = 3. * (1. + 0.5 * (s5 - 1.) * ct + 0.5 * (3. - s5) * cp);
    (num / den).max(0.)
}

fn solve_tridiagonal(a: &[f64], b: &[f64], c: &[f64], r: &[f64]) -> Vec<f64> {
    let n = b.len();
    let mut cp = vec![0.; n];
    let mut rp = vec![0.; n];
    cp[0] = c[0] / b[0];
    rp[0] = r[0] / b[0];
    for i in 1..n {
        let m = b[i] - a[i] * cp[i - 1];
        cp[i] = c[i] / m;
        rp[i] = (r[i] - a[i] * rp[i - 1]) / m;
    }
    let mut x = vec![0.; n];
    x[n - 1] = rp[n - 1];
    for i in (0..n - 1).rev() {
        x[i] = rp[i] - cp[i] * x[i + 1];
    }
    x
}

/// Cyclic tridiagonal solve (a[0] couples to x[n−1], c[n−1] to x[0]) by Sherman–Morrison.
fn solve_cyclic_tridiagonal(a: &[f64], b: &[f64], c: &[f64], r: &[f64]) -> Vec<f64> {
    let n = b.len();
    if n == 2 {
        // [b0, a0 + c0; a1 + c1, b1].
        let (m00, m01, m10, m11) = (b[0], a[0] + c[0], a[1] + c[1], b[1]);
        let det = m00 * m11 - m01 * m10;
        return vec![
            (r[0] * m11 - m01 * r[1]) / det,
            (m00 * r[1] - m10 * r[0]) / det,
        ];
    }
    let (alpha, beta) = (c[n - 1], a[0]);
    let gamma = -b[0];
    let mut bb = b.to_vec();
    bb[0] = b[0] - gamma;
    bb[n - 1] = b[n - 1] - alpha * beta / gamma;
    let x = solve_tridiagonal(a, &bb, c, r);
    let mut u = vec![0.; n];
    u[0] = gamma;
    u[n - 1] = alpha;
    let z = solve_tridiagonal(a, &bb, c, &u);
    let fact = (x[0] + beta * x[n - 1] / gamma) / (1. + z[0] + beta * z[n - 1] / gamma);
    x.iter().zip(&z).map(|(xi, zi)| xi - fact * zi).collect()
}

#[cfg(test)]
#[path = "node_tests.rs"]
mod tests;
