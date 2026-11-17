//! Arc length, arc-length parameterization and contour trim/dash effects.
//!
//! Lines and quadratics are measured in closed form; cubics use kurbo's
//! adaptive Gauss–Legendre quadrature to the requested accuracy.

use std::ops::Range;

use crate::error::{CurveError, finite};
use crate::geom::{
    self, BezPath, ParamCurve, ParamCurveArclen, PathEl, PathSeg, Point, QuadBez, Vec2,
};

/// Most segments (including implicit closing lines) a [`MeasuredPath`] accepts.
pub const MAX_SEGMENTS: usize = 8192;
/// Most output pieces [`MeasuredPath::apply`] produces.
pub const MAX_FRAGMENTS: usize = 16384;
/// Most dash entries.
pub const MAX_DASH_ENTRIES: usize = 64;
const TABLE_STEPS: usize = 16;

/// Closed-form arc length of a quadratic Bézier between `t0` and `t1`
/// (absolute value, in either order). Exact, including straight and
/// back-tracking collinear quadratics where the general formula divides by zero.
pub fn quad_arclen(q: QuadBez, t0: f64, t1: f64) -> f64 {
    let (lo, hi) = if t0 <= t1 { (t0, t1) } else { (t1, t0) };
    // B′(t) = 2(A + tB).
    let a_vec = q.p1 - q.p0;
    let b_vec = q.p2.to_vec2() - 2. * q.p1.to_vec2() + q.p0.to_vec2();
    let a = b_vec.dot(b_vec);
    let b = a_vec.dot(b_vec);
    let c = a_vec.dot(a_vec);
    if a <= 1e-24 * c.max(1e-300) || a == 0. {
        // Uniform straight line.
        return 2. * c.sqrt() * (hi - lo);
    }
    let disc = a * c - b * b;
    if disc <= 1e-12 * a * c {
        // Collinear controls: |B′(t)| = 2√a·|t − r|.
        let r = -b / a;
        let g = |t: f64| 0.5 * (t - r) * (t - r).abs();
        return 2. * a.sqrt() * (g(hi) - g(lo));
    }
    let q_at = |t: f64| (a * t * t + 2. * b * t + c).max(0.).sqrt();
    let root = disc.sqrt();
    let f = |t: f64| (t + b / a) * q_at(t) + disc / a.powf(1.5) * ((a * t + b) / root).asinh();
    f(hi) - f(lo)
}

/// Arc length of any segment between `t0` and `t1` (absolute value).
/// `accuracy` is an absolute length tolerance used for cubics.
pub fn arclen(seg: PathSeg, t0: f64, t1: f64, accuracy: f64) -> f64 {
    match seg {
        PathSeg::Line(l) => l.length() * (t1 - t0).abs(),
        PathSeg::Quad(q) => quad_arclen(q, t0, t1),
        PathSeg::Cubic(_) => {
            let (lo, hi) = if t0 <= t1 { (t0, t1) } else { (t1, t0) };
            seg.subsegment(lo..hi).arclen(accuracy.max(1e-14))
        }
    }
}

#[derive(Clone, Debug)]
struct MeasuredSeg {
    seg: PathSeg,
    /// (t, arc length from the segment start), monotone.
    table: Vec<(f64, f64)>,
    length: f64,
    /// Arc length at the segment start, within its contour.
    start: f64,
}

#[derive(Clone, Debug)]
struct Contour {
    segs: Vec<MeasuredSeg>,
    length: f64,
    closed: bool,
}

/// Where an arc-length position falls on a [`MeasuredPath`].
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct Location {
    /// Contour (subpath) index.
    pub contour: usize,
    /// Segment index within the contour (a closing line counts as a segment).
    pub segment: usize,
    /// Parameter on that segment.
    pub t: f64,
    /// The point.
    pub point: Point,
    /// Unit tangent there, if defined.
    pub tangent: Option<Vec2>,
}

/// Arc-length tables for a path, reusable while animating trims and dashes.
#[derive(Clone, Debug)]
pub struct MeasuredPath {
    contours: Vec<Contour>,
    length: f64,
    accuracy: f64,
}

impl MeasuredPath {
    /// Measure `path`. `accuracy` is an absolute length tolerance in
    /// `1e-12..=16`, applied per table interval of each cubic.
    pub fn new(path: &BezPath, accuracy: f64) -> Result<Self, CurveError> {
        if !accuracy.is_finite() || !(1e-12..=16.).contains(&accuracy) {
            return Err(CurveError::OutOfRange("accuracy"));
        }
        let mut contours: Vec<Contour> = Vec::new();
        let mut count = 0usize;
        let mut start = Point::ZERO;
        let mut pen = Point::ZERO;
        let mut open: Option<Contour> = None;
        let mut push = |open: &mut Option<Contour>, seg: PathSeg| -> Result<(), CurveError> {
            count += 1;
            if count > MAX_SEGMENTS {
                return Err(CurveError::WorkLimit("path exceeds 8192 segments"));
            }
            let contour = open.get_or_insert_with(|| Contour {
                segs: Vec::new(),
                length: 0.,
                closed: false,
            });
            let measured = measure_seg(seg, contour.length, accuracy);
            contour.length += measured.length;
            contour.segs.push(measured);
            Ok(())
        };
        for el in path.elements() {
            match *el {
                PathEl::MoveTo(p) => {
                    finite(&[p.x, p.y])?;
                    if let Some(c) = open.take() {
                        contours.push(c);
                    }
                    open = Some(Contour {
                        segs: Vec::new(),
                        length: 0.,
                        closed: false,
                    });
                    start = p;
                    pen = p;
                }
                PathEl::LineTo(p) => {
                    finite(&[p.x, p.y])?;
                    push(&mut open, PathSeg::Line(geom::Line::new(pen, p)))?;
                    pen = p;
                }
                PathEl::QuadTo(c, p) => {
                    finite(&[c.x, c.y, p.x, p.y])?;
                    push(&mut open, PathSeg::Quad(QuadBez::new(pen, c, p)))?;
                    pen = p;
                }
                PathEl::CurveTo(c1, c2, p) => {
                    finite(&[c1.x, c1.y, c2.x, c2.y, p.x, p.y])?;
                    push(
                        &mut open,
                        PathSeg::Cubic(geom::CubicBez::new(pen, c1, c2, p)),
                    )?;
                    pen = p;
                }
                PathEl::ClosePath => {
                    if pen != start {
                        push(&mut open, PathSeg::Line(geom::Line::new(pen, start)))?;
                    }
                    if let Some(mut c) = open.take() {
                        c.closed = true;
                        contours.push(c);
                    }
                    pen = start;
                }
            }
        }
        if let Some(c) = open.take() {
            contours.push(c);
        }
        contours.retain(|c| !c.segs.is_empty());
        let length = contours.iter().map(|c| c.length).sum();
        Ok(Self {
            contours,
            length,
            accuracy,
        })
    }

    /// Total length of all contours.
    pub fn length(&self) -> f64 {
        self.length
    }

    /// Number of non-empty contours.
    pub fn contour_count(&self) -> usize {
        self.contours.len()
    }

    /// Length of one contour.
    pub fn contour_length(&self, contour: usize) -> Option<f64> {
        self.contours.get(contour).map(|c| c.length)
    }

    /// Whether a contour was closed.
    pub fn is_closed(&self, contour: usize) -> Option<bool> {
        self.contours.get(contour).map(|c| c.closed)
    }

    /// Locate the arc-length position `s`, measured over all contours in order
    /// and clamped to `0..=length`.
    pub fn locate(&self, s: f64) -> Option<Location> {
        let mut s = s.clamp(0., self.length);
        let last = self.contours.len().checked_sub(1)?;
        for (index, contour) in self.contours.iter().enumerate() {
            // A zero-length contour is skipped unless nothing follows it.
            if (s <= contour.length && contour.length > 0.) || index == last {
                let (segment, t) = self.locate_in(contour, s);
                let seg = contour.segs[segment].seg;
                return Some(Location {
                    contour: index,
                    segment,
                    t,
                    point: seg.eval(t),
                    tangent: geom::tangent(seg, t),
                });
            }
            s -= contour.length;
        }
        None
    }

    /// The point at arc length `s`.
    pub fn point_at(&self, s: f64) -> Option<Point> {
        self.locate(s).map(|l| l.point)
    }

    fn locate_in(&self, contour: &Contour, s: f64) -> (usize, f64) {
        let s = s.clamp(0., contour.length);
        let index = contour
            .segs
            .partition_point(|m| m.start + m.length < s)
            .min(contour.segs.len() - 1);
        let m = &contour.segs[index];
        (index, self.t_at(m, s - m.start))
    }

    /// Invert s(t) on one segment: table bracket, then safeguarded Newton.
    fn t_at(&self, m: &MeasuredSeg, s: f64) -> f64 {
        if s <= 0. || m.length <= 0. {
            return 0.;
        }
        if s >= m.length {
            return 1.;
        }
        let i = m
            .table
            .partition_point(|(_, l)| *l < s)
            .clamp(1, m.table.len() - 1);
        let (mut lo, s_lo) = m.table[i - 1];
        let (mut hi, s_hi) = m.table[i];
        let mut t = if s_hi > s_lo {
            lo + (hi - lo) * (s - s_lo) / (s_hi - s_lo)
        } else {
            hi
        };
        let base = m.table[i - 1];
        for _ in 0..32 {
            let at = base.1 + arclen(m.seg, base.0, t, self.accuracy * 1e-3);
            let error = at - s;
            if error.abs() <= 1e-12 * (1. + m.length) {
                break;
            }
            if error > 0. {
                hi = t;
            } else {
                lo = t;
            }
            let speed = geom::deriv1(m.seg, t).hypot();
            let next = if speed > 0. {
                t - error / speed
            } else {
                f64::NAN
            };
            t = if next.is_finite() && next > lo && next < hi {
                next
            } else {
                0.5 * (lo + hi)
            };
        }
        t
    }

    /// Apply a trim and dash effect, returning the visible pieces as
    /// subpaths that keep their curve segments.
    pub fn apply(&self, effect: &PathEffect) -> Result<BezPath, CurveError> {
        let begin = self.length * effect.trim.start;
        let end = self.length * effect.trim.end;
        let mut out = BezPath::new();
        if begin >= end {
            return Ok(out);
        }
        let mut pieces = 0usize;
        let mut base = 0.;
        for contour in &self.contours {
            let from = if effect.trim.start == 0. {
                0.
            } else {
                (begin - base).max(0.)
            };
            let to = if effect.trim.end == 1. {
                contour.length
            } else {
                (end - base).min(contour.length)
            };
            base += contour.length;
            if from >= to {
                continue;
            }
            if effect.dash.is_empty() {
                let closed = contour.closed && from == 0. && to == contour.length;
                self.slice(contour, from, to, closed, &mut out, &mut pieces)?;
                continue;
            }
            let period: f64 = effect.dash.iter().sum();
            let mut offset = effect.phase.rem_euclid(period);
            let mut i = 0;
            while offset >= effect.dash[i] {
                offset -= effect.dash[i];
                i = (i + 1) % effect.dash.len();
            }
            let mut remaining = effect.dash[i] - offset;
            let mut at = from;
            let mut visible: Vec<(f64, f64)> = Vec::new();
            let mut steps = 0usize;
            while at < to {
                steps += 1;
                if steps > 2 * MAX_FRAGMENTS {
                    return Err(CurveError::WorkLimit("dash pattern exceeds work limit"));
                }
                let next = (at + remaining).min(to);
                if next <= at {
                    return Err(CurveError::WorkLimit(
                        "dash length is below numeric resolution",
                    ));
                }
                if i % 2 == 0 {
                    visible.push((at, next));
                }
                at = next;
                i = (i + 1) % effect.dash.len();
                remaining = effect.dash[i];
            }
            // Join the dash that crosses the start of an untrimmed closed
            // contour: it is emitted last, in path order, as the piece that
            // runs from the final dash through the start.
            let wraps = contour.closed && from == 0. && to == contour.length;
            if wraps && visible.len() == 1 && visible[0] == (0., to) {
                self.slice(contour, 0., to, true, &mut out, &mut pieces)?;
                continue;
            }
            let mut joined = None;
            if wraps && visible.len() > 1 && visible[0].0 == 0. && visible.last().unwrap().1 == to {
                let first_end = visible.remove(0).1;
                let last_start = visible.pop().unwrap().0;
                joined = Some((last_start, first_end));
            }
            for (a, b) in visible {
                self.slice(contour, a, b, false, &mut out, &mut pieces)?;
            }
            if let Some((last_start, first_end)) = joined {
                self.slice(contour, last_start, to, false, &mut out, &mut pieces)?;
                self.append(contour, 0., first_end, &mut out);
            }
        }
        Ok(out)
    }

    fn slice(
        &self,
        contour: &Contour,
        from: f64,
        to: f64,
        close: bool,
        out: &mut BezPath,
        pieces: &mut usize,
    ) -> Result<(), CurveError> {
        *pieces += 1;
        if *pieces > MAX_FRAGMENTS {
            return Err(CurveError::WorkLimit("path exceeds fragment limit"));
        }
        let (i0, t0) = self.locate_in(contour, from);
        out.move_to(contour.segs[i0].seg.eval(t0));
        self.append(contour, from, to, out);
        if close {
            out.close_path();
        }
        Ok(())
    }

    /// Append the drawing elements for `from..to` to the current subpath.
    fn append(&self, contour: &Contour, from: f64, to: f64, out: &mut BezPath) {
        let (i0, t0) = self.locate_in(contour, from);
        let (i1, t1) = self.locate_in(contour, to);
        for (i, m) in contour.segs.iter().enumerate().take(i1 + 1).skip(i0) {
            let a = if i == i0 { t0 } else { 0. };
            let b = if i == i1 { t1 } else { 1. };
            if b > a {
                geom::push_seg(out, m.seg.subsegment(a..b));
            }
        }
    }
}

fn measure_seg(seg: PathSeg, start: f64, accuracy: f64) -> MeasuredSeg {
    let mut table = Vec::with_capacity(TABLE_STEPS + 1);
    table.push((0., 0.));
    let mut s = 0.;
    for k in 1..=TABLE_STEPS {
        let (a, b) = (
            (k - 1) as f64 / TABLE_STEPS as f64,
            k as f64 / TABLE_STEPS as f64,
        );
        s += arclen(seg, a, b, accuracy / TABLE_STEPS as f64);
        table.push((b, s));
    }
    MeasuredSeg {
        seg,
        table,
        length: s,
        start,
    }
}

/// A trim range plus an optional dash pattern.
#[derive(Clone, Debug, PartialEq)]
pub struct PathEffect {
    trim: Range<f64>,
    dash: Vec<f64>,
    phase: f64,
}

impl Default for PathEffect {
    fn default() -> Self {
        Self {
            trim: 0. ..1.,
            dash: Vec::new(),
            phase: 0.,
        }
    }
}

impl PathEffect {
    /// No trim and no dash.
    pub fn new() -> Self {
        Self::default()
    }

    /// Keep the fraction `range` (within `0..=1`) of the total length.
    pub fn trim(mut self, range: Range<f64>) -> Result<Self, CurveError> {
        if !(range.start.is_finite()
            && range.end.is_finite()
            && range.start >= 0.
            && range.end <= 1.
            && range.start <= range.end)
        {
            return Err(CurveError::OutOfRange(
                "trim must be an ordered range within 0..1",
            ));
        }
        self.trim = range;
        Ok(self)
    }

    /// Alternate on/off lengths, restarting on each contour, shifted by
    /// `phase`. An empty pattern is solid; an odd pattern repeats once to
    /// make it even. Entries must be finite and positive.
    pub fn dash(mut self, pattern: &[f64], phase: f64) -> Result<Self, CurveError> {
        if pattern.len() > MAX_DASH_ENTRIES {
            return Err(CurveError::WorkLimit("dash pattern exceeds 64 entries"));
        }
        if !phase.is_finite() || pattern.iter().any(|p| !p.is_finite()) {
            return Err(CurveError::NonFinite);
        }
        if pattern.iter().any(|p| *p <= 0.) {
            return Err(CurveError::OutOfRange("dash lengths must be positive"));
        }
        self.dash = pattern.to_vec();
        if self.dash.len() % 2 == 1 {
            self.dash.extend_from_slice(pattern);
        }
        self.phase = phase;
        Ok(self)
    }
}

#[cfg(test)]
#[path = "measure_tests.rs"]
mod tests;
