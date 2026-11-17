//! Variable-width and tapered stroke outlines.
//!
//! Each side of the centerline is offset by ±w(s)·N(t), where w is half the
//! stroke width at arc length s. Offset curves are fitted with cubic Hermite
//! pieces that match the true offset's position and derivative at both ends
//! (d/dt[P + wN] = P′ + w′N − w·κ|P′|T), subdividing until the midpoint
//! error is within tolerance. Constant-width strokes can use [`kurbo_stroke`].

use crate::error::{CurveError, finite};
use crate::geom::{self, BezPath, Hermite, ParamCurve, PathSeg, Point, Vec2};
use crate::measure::{MeasuredPath, arclen};

/// Most subdivisions per offset piece.
const MAX_DEPTH: u32 = 12;

/// End cap of an open stroke.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Cap {
    /// Flat at the endpoint.
    #[default]
    Butt,
    /// A half circle.
    Round,
    /// Flat, extended by half the width.
    Square,
}

/// Join where segments meet at an angle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Join {
    /// Sharp corner, beveled when longer than `limit` × half the width.
    Miter(f64),
    /// Circular.
    Round,
    /// Straight cut.
    Bevel,
}

impl Default for Join {
    fn default() -> Self {
        Self::Miter(4.)
    }
}

/// Stroke width along the path, as piecewise-linear stops over the
/// normalized arc length of each subpath.
#[derive(Clone, Debug, PartialEq)]
pub struct StrokeProfile {
    stops: Vec<(f64, f64)>,
    /// Cap at the start of open subpaths.
    pub start_cap: Cap,
    /// Cap at the end of open subpaths.
    pub end_cap: Cap,
    /// Join between segments.
    pub join: Join,
}

impl StrokeProfile {
    /// Width stops `(fraction of length, width)`; fractions must be within
    /// `0..=1` and non-decreasing, widths finite and non-negative.
    pub fn new(stops: Vec<(f64, f64)>) -> Result<Self, CurveError> {
        if stops.is_empty() {
            return Err(CurveError::TooFewPoints {
                required: 1,
                actual: 0,
            });
        }
        for &(s, w) in &stops {
            finite(&[s, w])?;
            if !(0. ..=1.).contains(&s) || w < 0. {
                return Err(CurveError::OutOfRange("stroke stop"));
            }
        }
        if stops.windows(2).any(|p| p[1].0 < p[0].0) {
            return Err(CurveError::NotIncreasing);
        }
        Ok(Self {
            stops,
            start_cap: Cap::Butt,
            end_cap: Cap::Butt,
            join: Join::default(),
        })
    }

    /// One width everywhere.
    pub fn constant(width: f64) -> Result<Self, CurveError> {
        Self::new(vec![(0., width)])
    }

    /// Full `width` in the middle, tapering linearly to zero over the first
    /// `taper_in` and last `taper_out` fractions of the length.
    pub fn tapered(width: f64, taper_in: f64, taper_out: f64) -> Result<Self, CurveError> {
        let (a, b) = (
            taper_in.clamp(0., 1.),
            (1. - taper_out.clamp(0., 1.)).max(taper_in.clamp(0., 1.)),
        );
        Self::new(vec![(0., 0.), (a, width), (b, width), (1., 0.)])
    }

    /// Set both caps.
    pub fn caps(mut self, cap: Cap) -> Self {
        self.start_cap = cap;
        self.end_cap = cap;
        self
    }

    /// Set the join.
    pub fn with_join(mut self, join: Join) -> Self {
        self.join = join;
        self
    }

    /// Width at normalized position `f`, and its derivative with respect to `f`.
    pub fn width_at(&self, f: f64) -> (f64, f64) {
        let f = f.clamp(0., 1.);
        let s = &self.stops;
        if f <= s[0].0 {
            return (s[0].1, 0.);
        }
        if f >= s[s.len() - 1].0 {
            return (s[s.len() - 1].1, 0.);
        }
        let i = s.partition_point(|(x, _)| *x <= f).clamp(1, s.len() - 1);
        let ((x0, w0), (x1, w1)) = (s[i - 1], s[i]);
        if x1 <= x0 {
            return (w1, 0.);
        }
        let slope = (w1 - w0) / (x1 - x0);
        (w0 + slope * (f - x0), slope)
    }

    /// The filled outline of `path` (non-zero fill rule), within `tolerance`.
    pub fn outline(&self, path: &BezPath, tolerance: f64) -> Result<BezPath, CurveError> {
        let tolerance = tolerance.max(1e-9);
        let measured = MeasuredPath::new(path, (tolerance * 0.1).clamp(1e-12, 16.))?;
        let mut out = BezPath::new();
        let mut contour = 0;
        for sub in subpaths(path) {
            let segs: Vec<PathSeg> = sub
                .segments()
                .filter(|s| (s.end() - s.start()).hypot() > 0. || !matches!(s, PathSeg::Line(_)))
                .collect();
            if segs.is_empty() {
                continue;
            }
            let closed = matches!(sub.elements().last(), Some(geom::PathEl::ClosePath));
            let total = measured.contour_length(contour).unwrap_or(0.).max(1e-300);
            contour += 1;
            let mut starts = Vec::with_capacity(segs.len());
            let mut s = 0.;
            for seg in &segs {
                starts.push(s);
                s += arclen(*seg, 0., 1., tolerance * 0.1);
            }
            let left = self.side(&segs, &starts, total, 1., tolerance, closed);
            let right = self.side(&segs, &starts, total, -1., tolerance, closed);
            if closed {
                append_closed(&mut out, &left);
                append_closed(&mut out, &reverse(&right));
            } else {
                let half_start = 0.5 * self.width_at(0.).0;
                let half_end = 0.5 * self.width_at(1.).0;
                let first = segs[0];
                let last = *segs.last().unwrap();
                out.move_to(left[0].start());
                for seg in &left {
                    geom::push_seg(&mut out, *seg);
                }
                let right_rev = reverse(&right);
                cap(
                    &mut out,
                    last.end(),
                    geom::tangent(last, 1.).unwrap_or(Vec2::new(1., 0.)),
                    half_end,
                    self.end_cap,
                    right_rev[0].start(),
                );
                for seg in &right_rev {
                    geom::push_seg(&mut out, *seg);
                }
                cap(
                    &mut out,
                    first.start(),
                    -geom::tangent(first, 0.).unwrap_or(Vec2::new(1., 0.)),
                    half_start,
                    self.start_cap,
                    left[0].start(),
                );
                out.close_path();
            }
        }
        Ok(out)
    }

    /// One side's offset segments, with joins, in path order.
    fn side(
        &self,
        segs: &[PathSeg],
        starts: &[f64],
        total: f64,
        sign: f64,
        tolerance: f64,
        closed: bool,
    ) -> Vec<PathSeg> {
        let mut out: Vec<PathSeg> = Vec::new();
        for (k, seg) in segs.iter().enumerate() {
            let piece = offset_segment(
                *seg,
                |t| {
                    let s = starts[k] + arclen(*seg, 0., t, tolerance * 0.1);
                    let (w, dw) = self.width_at(s / total);
                    (0.5 * w * sign, 0.5 * dw / total * sign)
                },
                tolerance,
                0,
            );
            if let Some(prev) = out.last().copied() {
                self.join_to(
                    &mut out,
                    prev.end(),
                    piece[0].start(),
                    segs[k].start(),
                    sign,
                );
            }
            out.extend(piece);
        }
        if closed && segs.len() > 1 {
            let (end, start) = (out.last().unwrap().end(), out[0].start());
            self.join_to(&mut out, end, start, segs[0].start(), sign);
        }
        out
    }

    /// Connect two offset endpoints around the centerline vertex `at`.
    fn join_to(&self, out: &mut Vec<PathSeg>, from: Point, to: Point, at: Point, sign: f64) {
        if (to - from).hypot() <= 1e-12 {
            return;
        }
        let (a, b) = (from - at, to - at);
        // Outer side: the offset turns away from the centerline (positive area).
        let outer = a.cross(b) * sign < 0.;
        if !outer {
            // Inner side: go through the vertex so the overlap fills cleanly.
            out.push(PathSeg::Line(geom::Line::new(from, at)));
            out.push(PathSeg::Line(geom::Line::new(at, to)));
            return;
        }
        match self.join {
            Join::Bevel => out.push(PathSeg::Line(geom::Line::new(from, to))),
            Join::Round => {
                let r = a.hypot().max(b.hypot());
                let (a0, a1) = (a.atan2(), b.atan2());
                let sweep = (a1 - a0 + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU)
                    - std::f64::consts::PI;
                let mut prev = from;
                kurbo::Arc::new(at, (r, r), a0, sweep, 0.).to_cubic_beziers(
                    1e-3 * r.max(1e-9),
                    |c1, c2, p| {
                        out.push(PathSeg::Cubic(geom::CubicBez::new(prev, c1, c2, p)));
                        prev = p;
                    },
                );
                if (prev - to).hypot() > 1e-12 {
                    out.push(PathSeg::Line(geom::Line::new(prev, to)));
                }
            }
            Join::Miter(limit) => {
                // Intersect the offset tangent lines (perpendicular to a and b).
                let (da, db) = (Vec2::new(-a.y, a.x), Vec2::new(-b.y, b.x));
                let denom = da.cross(db);
                let half = a.hypot().max(b.hypot());
                if denom.abs() > 1e-12 {
                    let u = (to - from).cross(db) / denom;
                    let tip = from + da * u;
                    if (tip - at).hypot() <= limit * half {
                        out.push(PathSeg::Line(geom::Line::new(from, tip)));
                        out.push(PathSeg::Line(geom::Line::new(tip, to)));
                        return;
                    }
                }
                out.push(PathSeg::Line(geom::Line::new(from, to)));
            }
        }
    }
}

/// Offset one segment by `w(t)` along its normal: cubic Hermite pieces with
/// exact end positions and derivatives, subdivided until the midpoint error
/// is within `tolerance`.
fn offset_segment(
    seg: PathSeg,
    w: impl Fn(f64) -> (f64, f64) + Copy,
    tolerance: f64,
    depth: u32,
) -> Vec<PathSeg> {
    offset_range(seg, w, 0., 1., tolerance, depth)
}

fn offset_point(seg: PathSeg, w: impl Fn(f64) -> (f64, f64), t: f64) -> (Point, Vec2) {
    let p = seg.eval(t);
    let d1 = geom::deriv1(seg, t);
    let speed = d1.hypot();
    let n = geom::normal(seg, t).unwrap_or(Vec2::new(0., 1.));
    let tan = Vec2::new(n.y, -n.x);
    let (half, dhalf_ds) = w(t);
    let kappa = geom::curvature(seg, t);
    // d/dt (P + w N) with dN/dt = −κ|P′|T and dw/dt = dw/ds · |P′|.
    let derivative = d1 + n * (dhalf_ds * speed) - tan * (half * kappa * speed);
    (p + n * half, derivative)
}

fn offset_range(
    seg: PathSeg,
    w: impl Fn(f64) -> (f64, f64) + Copy,
    t0: f64,
    t1: f64,
    tolerance: f64,
    depth: u32,
) -> Vec<PathSeg> {
    let h = t1 - t0;
    let (p0, d0) = offset_point(seg, w, t0);
    let (p1, d1) = offset_point(seg, w, t1);
    let cubic = Hermite {
        p0,
        m0: d0 * h,
        p1,
        m1: d1 * h,
    }
    .to_cubic();
    let error = [0.25, 0.5, 0.75]
        .iter()
        .map(|&f| (cubic.eval(f) - offset_point(seg, w, t0 + f * h).0).hypot())
        .fold(0., f64::max);
    if error <= tolerance || depth >= MAX_DEPTH {
        return vec![PathSeg::Cubic(cubic)];
    }
    let mid = 0.5 * (t0 + t1);
    let mut out = offset_range(seg, w, t0, mid, tolerance, depth + 1);
    let mut right = offset_range(seg, w, mid, t1, tolerance, depth + 1);
    // Share the exact midpoint.
    if let (Some(PathSeg::Cubic(l)), Some(PathSeg::Cubic(r))) = (out.last_mut(), right.first_mut())
    {
        r.p0 = l.p3;
    }
    out.append(&mut right);
    out
}

fn cap(out: &mut BezPath, at: Point, dir: Vec2, half: f64, cap: Cap, to: Point) {
    match cap {
        Cap::Butt => out.line_to(to),
        Cap::Square => {
            let from = out
                .elements()
                .last()
                .and_then(|e| e.end_point())
                .unwrap_or(at);
            out.line_to(from + dir * half);
            out.line_to(to + dir * half);
            out.line_to(to);
        }
        Cap::Round => {
            let from = out
                .elements()
                .last()
                .and_then(|e| e.end_point())
                .unwrap_or(at);
            let a0 = (from - at).atan2();
            let sweep = {
                let s = (to - at).atan2() - a0;
                // Go around the far side (through `dir`).
                let mid = Vec2::from_angle(a0 + 0.5 * s).dot(dir);
                if mid >= 0. {
                    s
                } else if s > 0. {
                    s - std::f64::consts::TAU
                } else {
                    s + std::f64::consts::TAU
                }
            };
            if half > 0. {
                kurbo::Arc::new(at, (half, half), a0, sweep, 0.)
                    .to_cubic_beziers(1e-3 * half, |c1, c2, p| out.curve_to(c1, c2, p));
            }
            out.line_to(to);
        }
    }
}

fn subpaths(path: &BezPath) -> Vec<BezPath> {
    let mut out: Vec<BezPath> = Vec::new();
    for el in path.elements() {
        if matches!(el, geom::PathEl::MoveTo(_)) || out.is_empty() {
            out.push(BezPath::new());
        }
        out.last_mut().unwrap().push(*el);
    }
    out
}

fn reverse(segs: &[PathSeg]) -> Vec<PathSeg> {
    segs.iter().rev().map(|s| s.reverse()).collect()
}

fn append_closed(out: &mut BezPath, segs: &[PathSeg]) {
    if let Some(first) = segs.first() {
        out.move_to(first.start());
        for s in segs {
            geom::push_seg(out, *s);
        }
        out.close_path();
    }
}

/// Constant-width stroking through kurbo (handles joins, caps and dashes).
pub fn kurbo_stroke(path: &BezPath, style: &kurbo::Stroke, tolerance: f64) -> BezPath {
    kurbo::stroke(
        path.iter(),
        style,
        &kurbo::StrokeOpts::default(),
        tolerance.max(1e-9),
    )
}

#[cfg(test)]
#[path = "stroke_tests.rs"]
mod tests;
