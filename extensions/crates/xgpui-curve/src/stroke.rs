//! Variable-width and tapered stroke outlines.
//!
//! Each side of the centerline is offset by ±w(s)·N(t), where w is half the
//! stroke width at arc length s. Offset curves are fitted with cubic Hermite
//! pieces that match the true offset's position and derivative at both ends
//! (d/dt[P + wN] = P′ + w′N − w·κ|P′|T), subdividing until the midpoint
//! error is within tolerance. Constant-width strokes can use [`stroke_outline`].

use crate::error::{CurveError, finite};
use crate::geom::{self, Affine, BezPath, FillRule, Hermite, PathSeg, Point, Rect, Vec2};
use crate::measure::{MeasuredPath, PathEffect, arclen};

/// Most subdivisions per offset piece.
const MAX_DEPTH: u32 = 12;

/// End cap of an open stroke.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
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
#[non_exhaustive]
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
        for (contour, sub) in subpaths(path).into_iter().enumerate() {
            // `contour` counts every subpath, empty ones included, as
            // `MeasuredPath` does, so widths are read from the right contour.
            let segs: Vec<PathSeg> = sub
                .segments()
                .filter(|s| (s.end() - s.start()).hypot() > 0. || !matches!(s, PathSeg::Line(_)))
                .collect();
            if segs.is_empty() {
                continue;
            }
            let closed = matches!(sub.elements().last(), Some(geom::PathEl::ClosePath));
            let total = measured.contour_length(contour).unwrap_or(0.).max(1e-300);
            let mut starts = Vec::with_capacity(segs.len());
            let mut s = 0.;
            for seg in &segs {
                starts.push(s);
                s += arclen(*seg, 0., 1., tolerance * 0.1);
            }
            let width = |k: usize, t| {
                let s = starts[k] + arclen(segs[k], 0., t, tolerance * 0.1);
                let (w, dw) = self.width_at(s / total);
                (w, dw / total)
            };
            self.append_outline(
                &mut out,
                &segs,
                width,
                (self.width_at(0.).0, self.width_at(1.).0),
                tolerance,
                closed,
            );
        }
        Ok(out)
    }

    fn dashed_outline(
        &self,
        path: &BezPath,
        effect: &PathEffect,
        tolerance: f64,
    ) -> Result<BezPath, CurveError> {
        let tolerance = tolerance.max(1e-9);
        let accuracy = (tolerance * 0.1).clamp(1e-12, 16.);
        let measured = MeasuredPath::new(path, accuracy)?;
        let mut out = BezPath::new();
        let mut segs = Vec::new();
        let mut lengths = Vec::new();
        // Give the offset fitter a segment boundary at each profile stop so
        // a narrow feature cannot fall between its error samples.
        let cuts: Vec<_> = self.stops.iter().map(|&(fraction, _)| fraction).collect();
        measured.stroke_fragments(effect, &cuts, |source, total, closed| {
            segs.clear();
            lengths.clear();
            for &(seg, _) in source {
                segs.push(seg);
                lengths.push(arclen(seg, 0., 1., accuracy));
            }
            let width = |k: usize, t: f64| {
                let (seg, range) = &source[k];
                let length = lengths[k];
                let span = range.end - range.start;
                let scale = if length > 0. { span / length } else { 0. };
                let distance = if t == 1. {
                    range.end
                } else {
                    (range.start + scale * arclen(*seg, 0., t, accuracy)).min(range.end)
                };
                let (w, dw) = self.width_at(distance / total);
                (w, dw * scale / total)
            };
            let caps = (width(0, 0.).0, width(segs.len() - 1, 1.).0);
            self.append_outline(&mut out, &segs, width, caps, tolerance, closed);
        })?;
        Ok(out)
    }

    fn append_outline(
        &self,
        out: &mut BezPath,
        segs: &[PathSeg],
        width: impl Fn(usize, f64) -> (f64, f64),
        (start_width, end_width): (f64, f64),
        tolerance: f64,
        closed: bool,
    ) {
        let left = self.side(segs, &width, 1., tolerance, closed);
        let right = self.side(segs, &width, -1., tolerance, closed);
        if closed {
            append_closed(out, &left);
            append_closed(out, &reverse(&right));
        } else {
            let half_start = 0.5 * start_width;
            let half_end = 0.5 * end_width;
            let first = segs[0];
            let last = *segs.last().unwrap();
            // Caps run between the full-width end points: an end inside a
            // tight bend has its inner offset clamped short of them.
            let n_end = geom::normal(last, 1.).unwrap_or(Vec2::new(0., 1.)) * half_end;
            let n_start = geom::normal(first, 0.).unwrap_or(Vec2::new(0., 1.)) * half_start;
            out.move_to(left[0].start());
            for seg in &left {
                geom::push_seg(out, *seg);
            }
            let right_rev = reverse(&right);
            line_to(out, last.end() + n_end);
            cap(
                out,
                last.end(),
                geom::tangent(last, 1.).unwrap_or(Vec2::new(1., 0.)),
                half_end,
                self.end_cap,
                last.end() - n_end,
            );
            line_to(out, right_rev[0].start());
            for seg in &right_rev {
                geom::push_seg(out, *seg);
            }
            line_to(out, first.start() - n_start);
            cap(
                out,
                first.start(),
                -geom::tangent(first, 0.).unwrap_or(Vec2::new(1., 0.)),
                half_start,
                self.start_cap,
                first.start() + n_start,
            );
            line_to(out, left[0].start());
            out.close_path();
        }
    }

    /// One side's offset segments, with joins, in path order.
    fn side(
        &self,
        segs: &[PathSeg],
        width: impl Fn(usize, f64) -> (f64, f64),
        sign: f64,
        tolerance: f64,
        closed: bool,
    ) -> Vec<PathSeg> {
        let mut out: Vec<PathSeg> = Vec::new();
        // The unclamped (full-width) offset points at each segment's ends;
        // joins are built between these, since a tight bend clamps the inner
        // offset short of them.
        let full = |seg: PathSeg, t: f64, half: f64| {
            seg.eval(t) + geom::normal(seg, t).unwrap_or(Vec2::new(0., 1.)) * half
        };
        let mut first_full = None;
        let mut prev_full = None;
        for (k, seg) in segs.iter().enumerate() {
            let w = |t: f64| {
                let (w, dw) = width(k, t);
                (0.5 * w * sign, 0.5 * dw * sign)
            };
            let start_full = full(*seg, 0., w(0.).0);
            first_full.get_or_insert(start_full);
            let mut cusp = None;
            for (piece, next_cusp) in offset_segment(*seg, w, tolerance) {
                if let Some(prev) = out.last().copied() {
                    match (cusp, prev_full) {
                        (Some(t), _) => {
                            self.cusp_join(&mut out, prev.end(), piece[0].start(), *seg, t, w, sign)
                        }
                        (None, Some(from)) => {
                            push_line(&mut out, prev.end(), from);
                            self.join_to(&mut out, from, start_full, seg.start(), sign);
                            push_line(&mut out, start_full, piece[0].start());
                        }
                        (None, None) => {}
                    }
                }
                out.extend(piece);
                cusp = next_cusp;
            }
            prev_full = Some(full(*seg, 1., w(1.).0));
        }
        // A closed subpath joins at its seam, even with a single segment.
        if closed && let (Some(from), Some(to)) = (prev_full, first_full) {
            let (end, start) = (out.last().unwrap().end(), out[0].start());
            push_line(&mut out, end, from);
            self.join_to(&mut out, from, to, segs[0].start(), sign);
            push_line(&mut out, to, start);
        }
        out
    }

    /// Join across a cusp at `t`, where the curve turns back on itself. The
    /// inner offset has collapsed onto the centerline there (on opposite
    /// sides before and after the cusp), so the end is built from the full
    /// half width: out to the offset just before the cusp, around the far
    /// side of the tip (a round join) or straight across, then in to `to`.
    #[allow(clippy::too_many_arguments)]
    fn cusp_join(
        &self,
        out: &mut Vec<PathSeg>,
        from: Point,
        to: Point,
        seg: PathSeg,
        t: f64,
        w: impl Fn(f64) -> (f64, f64),
        sign: f64,
    ) {
        let at = seg.eval(t);
        let half = w(t).0.abs();
        let normal = |t: f64| geom::normal(seg, t.clamp(0., 1.)).unwrap_or(Vec2::new(0., 1.));
        let before = at + normal(t - 1e-6) * (half * sign);
        let after = at + normal(t + 1e-6) * (half * sign);
        let mut line = |a: Point, b: Point| {
            if (b - a).hypot() > 1e-12 {
                out.push(PathSeg::Line(geom::Line::new(a, b)));
            }
        };
        line(from, before);
        let mut end = before;
        if matches!(self.join, Join::Round) && half > 0. {
            // Through the incoming direction: clockwise from the left side,
            // counterclockwise from the right.
            let sweep = if sign > 0. {
                -std::f64::consts::PI
            } else {
                std::f64::consts::PI
            };
            let mut prev = before;
            let mut arcs = Vec::new();
            crate::geometry::arc_to_cubics(
                at,
                half,
                (before - at).atan2(),
                sweep,
                1e-3 * half,
                |c1, c2, p| {
                    arcs.push(PathSeg::Cubic(geom::CubicBez::new(prev, c1, c2, p)));
                    prev = p;
                },
            );
            out.extend(arcs);
            end = prev;
        }
        let mut line = |a: Point, b: Point| {
            if (b - a).hypot() > 1e-12 {
                out.push(PathSeg::Line(geom::Line::new(a, b)));
            }
        };
        line(end, after);
        line(after, to);
    }

    /// Connect two offset endpoints around the centerline vertex `at`.
    fn join_to(&self, out: &mut Vec<PathSeg>, from: Point, to: Point, at: Point, sign: f64) {
        if (to - from).hypot() <= 1e-12 {
            return;
        }
        let (a, b) = (from - at, to - at);
        // Outer side: the offset turns away from the centerline (positive area).
        // At a reversal (a cusp, or a path that doubles back) both offsets are
        // collinear with the vertex and either may be the one still at full
        // width (at a cusp the inner side has collapsed onto the centerline),
        // so both go around the far end of the turn; in the outline the two
        // arcs wind the same way. A cusp is sampled just either side of it,
        // so allow a small angle.
        let reversal = a.cross(b).abs() <= 1e-4 * a.hypot() * b.hypot() && a.dot(b) < 0.;
        let outer = reversal || a.cross(b) * sign < 0.;
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
                let mut sweep = (a1 - a0 + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU)
                    - std::f64::consts::PI;
                // Around the far end of a reversal, through the incoming
                // direction: clockwise from the left offset, counterclockwise
                // from the right.
                if reversal && sign > 0. && sweep > 0. {
                    sweep -= std::f64::consts::TAU;
                } else if reversal && sign < 0. && sweep < 0. {
                    sweep += std::f64::consts::TAU;
                }
                let mut prev = from;
                crate::geometry::arc_to_cubics(
                    at,
                    r,
                    a0,
                    sweep,
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
///
/// The segment is split at cusps, where the normal flips: each returned run
/// of pieces ends just short of a cusp and is paired with that cusp's
/// parameter (`None` for the last run, which ends at the segment end). It is
/// also split where the offset starts or stops folding (|w|·|κ| = 1), so that
/// the clamp in [`offset_point`] falls on piece ends.
fn offset_segment(
    seg: PathSeg,
    w: impl Fn(f64) -> (f64, f64) + Copy,
    tolerance: f64,
) -> Vec<(Vec<PathSeg>, Option<f64>)> {
    let (cusps, folds) = critical_params(seg, w);
    // Step just off each cusp, where the normal is defined on each side.
    const CUSP_GAP: f64 = 1e-6;
    let mut bounds = vec![0.];
    let mut breaks = Vec::new();
    let mut cuts: Vec<(f64, bool)> = cusps.iter().map(|&t| (t, true)).collect();
    cuts.extend(folds.iter().map(|&t| (t, false)));
    cuts.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut runs = Vec::new();
    for (t, is_cusp) in cuts {
        if is_cusp {
            bounds.push(t - CUSP_GAP);
            breaks.push((std::mem::take(&mut bounds), Some(t)));
            bounds.push(t + CUSP_GAP);
        } else if t > bounds[bounds.len() - 1] {
            bounds.push(t);
        }
    }
    bounds.push(1.);
    breaks.push((bounds, None));
    for (range, cusp) in breaks {
        let mut pieces = Vec::new();
        for pair in range.windows(2) {
            if pair[1] > pair[0] {
                // Pieces are split at the fold roots. At a piece's own ends
                // the clamp follows the piece's middle, so rounding at a root
                // cannot give an end the other side's tangent; inside, it is
                // decided point by point.
                let mid = 0.5 * (pair[0] + pair[1]);
                let clamped = Some(w(mid).0 * geom::curvature(seg, mid) > 1.);
                let ends = (clamped, clamped);
                pieces.extend(offset_range(seg, w, pair[0], pair[1], ends, tolerance, 0));
            }
        }
        if !pieces.is_empty() {
            runs.push((pieces, cusp));
        }
    }
    runs
}

/// Interior cusps (zero speed), and the other split points: where the offset
/// distance reaches the radius of curvature, and where the curvature peaks
/// (a cusp of the evolute, which a clamped inner side follows). Found by
/// sampling, then refining.
fn critical_params(seg: PathSeg, w: impl Fn(f64) -> (f64, f64)) -> (Vec<f64>, Vec<f64>) {
    const N: usize = 64;
    let ts: Vec<f64> = (0..=N).map(|i| i as f64 / N as f64).collect();
    let speed = |t: f64| geom::deriv1(seg, t).hypot();
    let speeds: Vec<f64> = ts.iter().map(|&t| speed(t)).collect();
    let scale = speeds.iter().fold(0., |a: f64, &b| a.max(b));
    let mut cusps = Vec::new();
    if scale > 0. && matches!(seg, PathSeg::Cubic(_)) {
        for i in 1..N {
            if speeds[i] <= speeds[i - 1] && speeds[i] <= speeds[i + 1] {
                // Golden-section search for the speed minimum.
                let (mut lo, mut hi) = (ts[i - 1], ts[i + 1]);
                let g = 0.5 * (5f64.sqrt() - 1.);
                for _ in 0..80 {
                    let (a, b) = (hi - g * (hi - lo), lo + g * (hi - lo));
                    if speed(a) < speed(b) {
                        hi = b;
                    } else {
                        lo = a;
                    }
                }
                let t = 0.5 * (lo + hi);
                if speed(t) <= 1e-7 * scale && t > 1e-4 && t < 1. - 1e-4 {
                    cusps.push(t);
                }
            }
        }
    }
    cusps.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    // fold(t) > 0 where |w|·|κ| > 1.
    let fold = |t: f64| (w(t).0 * geom::curvature(seg, t)).abs() - 1.;
    let mut folds = Vec::new();
    for pair in ts.windows(2) {
        let (f0, f1) = (fold(pair[0]), fold(pair[1]));
        if f0.is_finite() && f1.is_finite() && (f0 > 0.) != (f1 > 0.) {
            let (mut lo, mut hi) = (pair[0], pair[1]);
            for _ in 0..60 {
                let mid = 0.5 * (lo + hi);
                if (fold(mid) > 0.) == (f0 > 0.) {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            folds.push(0.5 * (lo + hi));
        }
    }
    // Curvature extrema, where a clamped side's evolute turns back.
    let k = |t: f64| geom::curvature(seg, t).abs();
    let ks: Vec<f64> = ts.iter().map(|&t| k(t)).collect();
    for i in 1..N {
        if ks[i].is_finite() && ks[i] >= ks[i - 1] && ks[i] >= ks[i + 1] && ks[i] > 0. {
            let (mut lo, mut hi) = (ts[i - 1], ts[i + 1]);
            let g = 0.5 * (5f64.sqrt() - 1.);
            for _ in 0..80 {
                let (a, b) = (hi - g * (hi - lo), lo + g * (hi - lo));
                if k(a) > k(b) {
                    hi = b;
                } else {
                    lo = a;
                }
            }
            let t = 0.5 * (lo + hi);
            if !cusps.iter().any(|c| (c - t).abs() < 1e-4) {
                folds.push(t);
            }
        }
    }
    folds.sort_by(f64::total_cmp);
    (cusps, folds)
}

/// The offset point at `t` and its derivative. On the inside of a bend
/// tighter than the offset distance, the distance is clamped to the radius of
/// curvature: past it the offset would turn back on itself in a loop that
/// winds the other way and cancels the fill. Every point within the stroke's
/// half width of the centerline lies within the local radius of its nearest
/// centerline point, so the clamped side still covers the whole stroke.
/// `clamped` overrides the test where the caller knows the side of a root.
fn offset_point(
    seg: PathSeg,
    w: impl Fn(f64) -> (f64, f64),
    t: f64,
    clamped: Option<bool>,
) -> (Point, Vec2) {
    let p = seg.eval(t);
    let d1 = geom::deriv1(seg, t);
    let speed = d1.hypot();
    let n = geom::normal(seg, t).unwrap_or(Vec2::new(0., 1.));
    let tan = Vec2::new(n.y, -n.x);
    let (mut half, dhalf_ds) = w(t);
    let kappa = geom::curvature(seg, t);
    // d/dt of the offset distance.
    let mut dhalf_dt = dhalf_ds * speed;
    if clamped.unwrap_or(half * kappa > 1.) && kappa != 0. {
        // Inner side past the centre of curvature: ride the evolute, P + N/κ.
        half = 1. / kappa;
        let h = 1e-7;
        let (ta, tb) = ((t - h).max(0.), (t + h).min(1.));
        let (ka, kb) = (geom::curvature(seg, ta), geom::curvature(seg, tb));
        dhalf_dt = if tb > ta && ka != 0. && kb != 0. {
            (1. / kb - 1. / ka) / (tb - ta)
        } else {
            0.
        };
    }
    // d/dt (P + w N) with dN/dt = −κ|P′|T.
    let derivative = d1 + n * dhalf_dt - tan * (half * kappa * speed);
    (p + n * half, derivative)
}

fn offset_range(
    seg: PathSeg,
    w: impl Fn(f64) -> (f64, f64) + Copy,
    t0: f64,
    t1: f64,
    ends: (Option<bool>, Option<bool>),
    tolerance: f64,
    depth: u32,
) -> Vec<PathSeg> {
    let h = t1 - t0;
    let (p0, d0) = offset_point(seg, w, t0, ends.0);
    let (p1, d1) = offset_point(seg, w, t1, ends.1);
    let cubic = Hermite {
        p0,
        m0: d0 * h,
        p1,
        m1: d1 * h,
    }
    .to_cubic();
    // Sampled near the ends too: a wrong end tangent bulges there first.
    let error = [0.1, 0.25, 0.5, 0.75, 0.9]
        .iter()
        .map(|&f| (cubic.eval(f) - offset_point(seg, w, t0 + f * h, None).0).hypot())
        .fold(0., f64::max);
    if error <= tolerance || depth >= MAX_DEPTH {
        return vec![PathSeg::Cubic(cubic)];
    }
    let mid = 0.5 * (t0 + t1);
    let mut out = offset_range(seg, w, t0, mid, (ends.0, None), tolerance, depth + 1);
    let mut right = offset_range(seg, w, mid, t1, (None, ends.1), tolerance, depth + 1);
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
                crate::geometry::arc_to_cubics(at, half, a0, sweep, 1e-3 * half, |c1, c2, p| {
                    out.curve_to(c1, c2, p)
                });
            }
            out.line_to(to);
        }
    }
}

fn push_line(out: &mut Vec<PathSeg>, a: Point, b: Point) {
    if (b - a).hypot() > 1e-12 {
        out.push(PathSeg::Line(geom::Line::new(a, b)));
    }
}

fn line_to(out: &mut BezPath, p: Point) {
    let current = out.elements().last().and_then(|e| e.end_point());
    if current.is_none_or(|c| (c - p).hypot() > 1e-12) {
        out.line_to(p);
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

/// Style of a constant-width stroke.
#[derive(Clone, Debug, PartialEq)]
pub struct StrokeStyle {
    /// Width of the stroke.
    pub width: f64,
    /// Join between consecutive segments.
    pub join: Join,
    /// Cap at the start of each open subpath.
    pub start_cap: Cap,
    /// Cap at the end of each open subpath.
    pub end_cap: Cap,
    /// Alternating on/off lengths. An empty pattern gives a solid stroke.
    pub dash_pattern: Vec<f64>,
    /// Offset along the dash pattern at the beginning of a subpath.
    pub dash_offset: f64,
}

impl StrokeStyle {
    /// Creates a solid stroke with round joins and round caps.
    pub const fn new(width: f64) -> Self {
        Self {
            width,
            join: Join::Round,
            start_cap: Cap::Round,
            end_cap: Cap::Round,
            dash_pattern: Vec::new(),
            dash_offset: 0.,
        }
    }
}

/// Returns the outline of a constant-width stroke with the specified style.
pub fn stroke_outline(path: &BezPath, style: &StrokeStyle, tolerance: f64) -> BezPath {
    let (join, miter_limit) = match style.join {
        Join::Bevel => (kurbo::Join::Bevel, 4.),
        Join::Miter(limit) => (kurbo::Join::Miter, limit),
        Join::Round => (kurbo::Join::Round, 4.),
    };
    let cap = |cap| match cap {
        Cap::Butt => kurbo::Cap::Butt,
        Cap::Round => kurbo::Cap::Round,
        Cap::Square => kurbo::Cap::Square,
    };
    let backend_style = kurbo::Stroke::new(style.width)
        .with_join(join)
        .with_miter_limit(miter_limit)
        .with_start_cap(cap(style.start_cap))
        .with_end_cap(cap(style.end_cap))
        .with_dashes(style.dash_offset, &style.dash_pattern);
    BezPath::from_backend(kurbo::stroke(
        path.backend_elements(),
        &backend_style,
        &kurbo::StrokeOpts::default(),
        tolerance.max(1e-9),
    ))
}

/// Coordinate space for the stroke width, dash lengths, dash offset, and outline tolerance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrokeSpace {
    /// Expand in model coordinates, then transform the outline.
    Model,
    /// Transform the centerline first, then expand in logical pixels.
    /// Display scale is applied by the renderer.
    LogicalPixels,
}

/// A styled stroke outline in logical pixels, shared by painting, bounds, and picking.
#[derive(Clone, Debug)]
pub struct StrokeFootprint {
    outline: BezPath,
}

impl StrokeFootprint {
    /// Build a constant-width footprint with a nonzero fill rule.
    ///
    /// Coordinates and style values must be finite. Width, dash lengths,
    /// miter limit, and tolerance must be positive. Outline tolerance is
    /// clamped to at least 1e-9 in the selected stroke space. Dash phase
    /// resets at each contour. Zero-length contours produce no outline.
    /// Dashed paths are limited to 8192 segments and a total length of
    /// one million times the shortest dash or gap.
    pub fn new(
        path: &BezPath,
        style: &StrokeStyle,
        transform: Affine,
        space: StrokeSpace,
        tolerance: f64,
    ) -> Result<Self, CurveError> {
        finite(&[style.width, style.dash_offset])?;
        finite(&style.dash_pattern)?;
        if style.width <= 0. {
            return Err(CurveError::OutOfRange("stroke width"));
        }
        if let Join::Miter(limit) = style.join {
            finite(&[limit])?;
            if limit <= 0. {
                return Err(CurveError::OutOfRange("miter limit"));
            }
        }
        let dash_total = style.dash_pattern.iter().sum::<f64>();
        finite(&[if style.dash_pattern.len() % 2 == 0 {
            dash_total
        } else {
            2. * dash_total
        }])?;
        if style.dash_pattern.iter().any(|&length| length <= 0.) {
            return Err(CurveError::OutOfRange("dash length"));
        }

        Self::build(path, transform, space, tolerance, |path| {
            if let Some(min_dash) = style.dash_pattern.iter().copied().reduce(f64::min) {
                let length = MeasuredPath::new(path, 1e-6)?.length();
                finite(&[length])?;
                if length / min_dash > 1_000_000. {
                    return Err(CurveError::WorkLimit("stroke dash subdivisions"));
                }
            }
            Ok(stroke_outline(path, style, tolerance))
        })
    }

    /// Build a solid variable-width footprint with the nonzero fill rule.
    ///
    /// Profile stops use normalized arc length and restart on each contour.
    /// Length is measured before the transform in model space, and after it
    /// in logical-pixel space. The outline tolerance uses the same space.
    pub fn from_profile(
        path: &BezPath,
        profile: &StrokeProfile,
        transform: Affine,
        space: StrokeSpace,
        tolerance: f64,
    ) -> Result<Self, CurveError> {
        Self::build_profile(path, profile, transform, space, tolerance, |path| {
            profile.outline(path, tolerance)
        })
    }

    /// Build a dashed variable-width footprint with the nonzero fill rule.
    ///
    /// Dash lengths, phase and tolerance use the selected stroke space.
    /// Positive phase advances into the pattern. Each contour restarts the
    /// phase and samples the profile over its full normalized arc length,
    /// including gaps. A dash crossing a closed seam keeps its join and
    /// uses the profile's end and start widths on the respective sides.
    /// An empty pattern is equivalent to [`Self::from_profile`].
    /// The dash pattern and fragment limits are those of [`PathEffect`].
    pub fn from_dashed_profile(
        path: &BezPath,
        profile: &StrokeProfile,
        dash_pattern: &[f64],
        phase: f64,
        transform: Affine,
        space: StrokeSpace,
        tolerance: f64,
    ) -> Result<Self, CurveError> {
        let effect = PathEffect::new().dash(dash_pattern, phase)?;
        if dash_pattern.is_empty() {
            return Self::from_profile(path, profile, transform, space, tolerance);
        }
        Self::build_profile(path, profile, transform, space, tolerance, |path| {
            profile.dashed_outline(path, &effect, tolerance)
        })
    }

    fn build_profile(
        path: &BezPath,
        profile: &StrokeProfile,
        transform: Affine,
        space: StrokeSpace,
        tolerance: f64,
        expand: impl FnOnce(&BezPath) -> Result<BezPath, CurveError>,
    ) -> Result<Self, CurveError> {
        if let Join::Miter(limit) = profile.join {
            finite(&[limit])?;
            if limit <= 0. {
                return Err(CurveError::OutOfRange("miter limit"));
            }
        }
        Self::build(path, transform, space, tolerance, |path| {
            if profile.stops.iter().all(|&(_, width)| width == 0.) {
                return Ok(BezPath::new());
            }
            expand(path)
        })
    }

    fn build(
        path: &BezPath,
        transform: Affine,
        space: StrokeSpace,
        tolerance: f64,
        expand: impl FnOnce(&BezPath) -> Result<BezPath, CurveError>,
    ) -> Result<Self, CurveError> {
        finite(&transform.as_coeffs())?;
        finite(&[tolerance])?;
        if tolerance <= 0. {
            return Err(CurveError::OutOfRange("stroke tolerance"));
        }
        if !path.backend_elements().all(|el| el.is_finite()) {
            return Err(CurveError::NonFinite);
        }
        let centerline;
        let (path, outline_transform) = match space {
            StrokeSpace::Model => (path, transform),
            StrokeSpace::LogicalPixels => {
                centerline = transform * path;
                (&centerline, Affine::IDENTITY)
            }
        };
        if !path.backend_elements().all(|el| el.is_finite()) {
            return Err(CurveError::NonFinite);
        }
        let mut outline = expand(path)?;
        outline.apply_affine(outline_transform);
        if !outline.backend_elements().all(|el| el.is_finite()) {
            return Err(CurveError::NonFinite);
        }
        Ok(Self { outline })
    }

    /// The closed outline to paint with a nonzero fill rule.
    pub fn outline(&self) -> &BezPath {
        &self.outline
    }

    /// Tight bounds of the outline, absent for an empty stroke.
    pub fn bounds(&self) -> Option<Rect> {
        (!self.outline.elements().is_empty()).then(|| self.outline.bounding_box())
    }

    /// Pick the nonzero fill or a point within `slop` logical pixels of
    /// its outline. A boundary tolerance of 1e-6 logical pixels applies
    /// even at zero slop. Invalid points and slop values miss.
    pub fn contains(&self, point: Point, slop: f64) -> bool {
        if !point.x.is_finite() || !point.y.is_finite() || !slop.is_finite() || slop < 0. {
            return false;
        }
        geom::fill_contains(&self.outline, point, FillRule::NonZero)
            || geom::nearest_on_path(&self.outline, point)
                .is_some_and(|hit| hit.distance <= slop + 1e-6)
    }
}

#[cfg(test)]
#[path = "stroke_tests.rs"]
mod tests;
