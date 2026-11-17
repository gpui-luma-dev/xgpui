//! Segment evaluation, Hermite conversion, queries, intersection, clipping,
//! flattening and hit testing.
//!
//! Every segment kind is evaluated through its exact cubic form
//! ([`to_cubic`]), which keeps the parameterization of lines and quadratics
//! unchanged. Normals are the tangent rotated by +90° in the coordinate system
//! the points are in: counter-clockwise for y-up math coordinates, clockwise on
//! screen with y down.

pub use kurbo::{
    Affine, BezPath, CubicBez, Line, ParamCurve, ParamCurveArclen, ParamCurveNearest, PathEl,
    PathSeg, Point, QuadBez, Rect, Shape, Vec2,
};

const EPS: f64 = 1e-12;

/// Accuracy passed to kurbo's nearest-point solver.
pub const NEAREST_ACCURACY: f64 = 1e-12;

/// The exact cubic form of a segment, with the same parameterization.
pub fn to_cubic(seg: PathSeg) -> CubicBez {
    match seg {
        PathSeg::Line(l) => CubicBez::new(
            l.p0,
            l.p0.lerp(l.p1, 1. / 3.),
            l.p0.lerp(l.p1, 2. / 3.),
            l.p1,
        ),
        PathSeg::Quad(q) => q.raise(),
        PathSeg::Cubic(c) => c,
    }
}

/// Position B(t).
pub fn eval(seg: PathSeg, t: f64) -> Point {
    seg.eval(t)
}

/// First derivative B′(t).
pub fn deriv1(seg: PathSeg, t: f64) -> Vec2 {
    let c = to_cubic(seg);
    let u = 1. - t;
    3. * u * u * (c.p1 - c.p0) + 6. * u * t * (c.p2 - c.p1) + 3. * t * t * (c.p3 - c.p2)
}

/// Second derivative B″(t).
pub fn deriv2(seg: PathSeg, t: f64) -> Vec2 {
    let c = to_cubic(seg);
    let a = c.p2 - 2. * c.p1.to_vec2() + c.p0.to_vec2();
    let b = c.p3 - 2. * c.p2.to_vec2() + c.p1.to_vec2();
    6. * ((1. - t) * a.to_vec2() + t * b.to_vec2())
}

/// Unit tangent T(t). Where B′(t) vanishes (a control point coincides with an
/// endpoint, or a cusp) the one-sided limit direction is used. `None` only for
/// a segment collapsed to a point.
pub fn tangent(seg: PathSeg, t: f64) -> Option<Vec2> {
    let d1 = deriv1(seg, t);
    if d1.hypot() > EPS {
        return Some(d1.normalize());
    }
    let d2 = deriv2(seg, t);
    if d2.hypot() > EPS {
        // Near t = 1 the curve arrives along −B″(1).
        return Some(if t >= 1. {
            -d2.normalize()
        } else {
            d2.normalize()
        });
    }
    let chord = seg.end() - seg.start();
    (chord.hypot() > EPS).then(|| chord.normalize())
}

/// Unit normal N(t): [`tangent`] rotated by +90°.
pub fn normal(seg: PathSeg, t: f64) -> Option<Vec2> {
    tangent(seg, t).map(|v| Vec2::new(-v.y, v.x))
}

/// Signed curvature κ(t) = (x′y″ − y′x″) / (x′² + y′²)^{3/2}; 0 where B′(t) vanishes.
pub fn curvature(seg: PathSeg, t: f64) -> f64 {
    let d1 = deriv1(seg, t);
    let speed = d1.hypot();
    if speed <= EPS {
        return 0.;
    }
    d1.cross(deriv2(seg, t)) / (speed * speed * speed)
}

/// Cubic Hermite form: endpoints and their derivatives.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hermite {
    /// Start point.
    pub p0: Point,
    /// Derivative at the start.
    pub m0: Vec2,
    /// End point.
    pub p1: Point,
    /// Derivative at the end.
    pub m1: Vec2,
}

impl Hermite {
    /// Bézier controls c₁ = p₀ + m₀/3 and c₂ = p₁ − m₁/3.
    pub fn to_cubic(self) -> CubicBez {
        CubicBez::new(
            self.p0,
            self.p0 + self.m0 / 3.,
            self.p1 - self.m1 / 3.,
            self.p1,
        )
    }

    /// m₀ = 3(c₁ − p₀), m₁ = 3(p₁ − c₂).
    pub fn from_cubic(c: CubicBez) -> Self {
        Self {
            p0: c.p0,
            m0: 3. * (c.p1 - c.p0),
            p1: c.p3,
            m1: 3. * (c.p3 - c.p2),
        }
    }
}

/// Tight axis-aligned bounds from the exact x(t) and y(t) extrema.
pub fn tight_bounds(seg: PathSeg) -> Rect {
    seg.bounding_box()
}

/// The nearest point on a segment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NearestPoint {
    /// Parameter of the nearest point, in 0..=1.
    pub t: f64,
    /// The nearest point.
    pub point: Point,
    /// Unsigned distance.
    pub distance: f64,
    /// Distance signed positive on the [`normal`] side.
    pub signed_distance: f64,
}

/// Nearest point on `seg` to `p`.
pub fn nearest(seg: PathSeg, p: Point) -> NearestPoint {
    let hit = seg.nearest(p, NEAREST_ACCURACY);
    let point = seg.eval(hit.t);
    let distance = hit.distance_sq.max(0.).sqrt();
    let side = normal(seg, hit.t).map_or(1., |n| n.dot(p - point));
    NearestPoint {
        t: hit.t,
        point,
        distance,
        signed_distance: if side < 0. { -distance } else { distance },
    }
}

/// Split at `t` with de Casteljau subdivision. The halves share the split point exactly.
pub fn split(seg: PathSeg, t: f64) -> (PathSeg, PathSeg) {
    let t = t.clamp(0., 1.);
    let left = seg.subsegment(0. ..t);
    let mut right = seg.subsegment(t..1.);
    set_start(&mut right, left.end());
    (left, right)
}

/// The part of `seg` between `t0` and `t1` (which may be reversed).
pub fn subrange(seg: PathSeg, t0: f64, t1: f64) -> PathSeg {
    seg.subsegment(t0.clamp(0., 1.)..t1.clamp(0., 1.))
}

fn set_start(seg: &mut PathSeg, p: Point) {
    match seg {
        PathSeg::Line(l) => l.p0 = p,
        PathSeg::Quad(q) => q.p0 = p,
        PathSeg::Cubic(c) => c.p0 = p,
    }
}

/// One crossing of two curves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Intersection {
    /// Parameter on the first curve.
    pub t_a: f64,
    /// Parameter on the second curve.
    pub t_b: f64,
    /// Intersection point (on the first curve).
    pub point: Point,
}

/// Crossings of a segment with a line segment, from kurbo's analytic solver.
pub fn intersect_line(seg: PathSeg, line: Line) -> Vec<Intersection> {
    seg.intersect_line(line)
        .into_iter()
        .map(|hit| Intersection {
            t_a: hit.segment_t,
            t_b: hit.line_t,
            point: seg.eval(hit.segment_t),
        })
        .collect()
}

/// Maximum subdivision steps for [`intersect_segments`].
pub const MAX_INTERSECTION_STEPS: usize = 4096;

/// Crossings of two segments within `tolerance`, by bounding-box subdivision
/// followed by Newton refinement. Lines use the analytic solver. Overlapping
/// (coincident) curves return a bounded sample of shared points rather than
/// the full overlap; at most [`MAX_INTERSECTION_STEPS`] subdivisions are done.
pub fn intersect_segments(a: PathSeg, b: PathSeg, tolerance: f64) -> Vec<Intersection> {
    let tolerance = tolerance.max(1e-12);
    if let PathSeg::Line(line) = b {
        return intersect_line(a, line);
    }
    if let PathSeg::Line(line) = a {
        return intersect_line(b, line)
            .into_iter()
            .map(|hit| Intersection {
                t_a: hit.t_b,
                t_b: hit.t_a,
                point: hit.point,
            })
            .collect();
    }
    let mut raw = Vec::new();
    let mut stack = vec![(0., 1., 0., 1.)];
    let mut steps = 0;
    while let Some((a0, a1, b0, b1)) = stack.pop() {
        steps += 1;
        if steps > MAX_INTERSECTION_STEPS {
            break;
        }
        let ra = a
            .subsegment(a0..a1)
            .bounding_box()
            .inflate(tolerance, tolerance);
        let rb = b.subsegment(b0..b1).bounding_box();
        if !overlaps(ra, rb) {
            continue;
        }
        let size_a = ra.width().max(ra.height()) - 2. * tolerance;
        let size_b = rb.width().max(rb.height());
        if size_a <= tolerance && size_b <= tolerance {
            raw.push((0.5 * (a0 + a1), 0.5 * (b0 + b1)));
            continue;
        }
        if size_a >= size_b {
            let mid = 0.5 * (a0 + a1);
            stack.push((a0, mid, b0, b1));
            stack.push((mid, a1, b0, b1));
        } else {
            let mid = 0.5 * (b0 + b1);
            stack.push((a0, a1, b0, mid));
            stack.push((a0, a1, mid, b1));
        }
    }
    let mut hits: Vec<Intersection> = Vec::new();
    for (ta, tb) in raw {
        let (ta, tb) = refine(a, b, ta, tb);
        let point = a.eval(ta);
        if point.distance(b.eval(tb)) > 4. * tolerance {
            continue;
        }
        // One crossing found from several cells refines to the same point and
        // parameters; a self-intersection of either curve gives the same point
        // at different parameters, which is a separate crossing.
        if hits.iter().all(|h| {
            h.point.distance(point) > 4. * tolerance
                || (h.t_a - ta).abs() > 1e-3
                || (h.t_b - tb).abs() > 1e-3
        }) {
            hits.push(Intersection {
                t_a: ta,
                t_b: tb,
                point,
            });
        }
    }
    hits.sort_by(|x, y| x.t_a.total_cmp(&y.t_a));
    hits
}

fn overlaps(a: Rect, b: Rect) -> bool {
    a.x0 <= b.x1 && b.x0 <= a.x1 && a.y0 <= b.y1 && b.y0 <= a.y1
}

fn refine(a: PathSeg, b: PathSeg, mut s: f64, mut t: f64) -> (f64, f64) {
    for _ in 0..8 {
        let f = a.eval(s) - b.eval(t);
        let da = deriv1(a, s);
        let db = deriv1(b, t);
        let det = -da.cross(db);
        if det.abs() < 1e-18 {
            break;
        }
        // Solve [da, −db] · [Δs, Δt]ᵀ = −f.
        let ds = (-f).cross(-db) / da.cross(-db);
        let dt = da.cross(-f) / da.cross(-db);
        if !ds.is_finite() || !dt.is_finite() {
            break;
        }
        s = (s + ds).clamp(0., 1.);
        t = (t + dt).clamp(0., 1.);
        if ds.abs() < 1e-14 && dt.abs() < 1e-14 {
            break;
        }
    }
    (s, t)
}

/// Clip a path's outline to `rect`, keeping the parts inside (boundary
/// inclusive) with their original curve segments. Each input subpath gives
/// its own output subpaths; a closed subpath entirely inside stays closed, and
/// any other piece is open. This clips strokes and polylines such as plot
/// series, not fill areas.
pub fn clip_to_rect(path: &BezPath, rect: Rect) -> BezPath {
    let rect = rect.abs();
    let edges = [
        Line::new((rect.x0, rect.y0), (rect.x1, rect.y0)),
        Line::new((rect.x1, rect.y0), (rect.x1, rect.y1)),
        Line::new((rect.x1, rect.y1), (rect.x0, rect.y1)),
        Line::new((rect.x0, rect.y1), (rect.x0, rect.y0)),
    ];
    let slack = 1e-9 * (1. + rect.width().abs().max(rect.height().abs()));
    let inside = |p: Point| {
        p.x >= rect.x0 - slack
            && p.x <= rect.x1 + slack
            && p.y >= rect.y0 - slack
            && p.y <= rect.y1 + slack
    };
    let mut out = BezPath::new();
    // Pen: where the current output subpath ends. `whole`: the current input
    // subpath has been kept without a break so far.
    let mut pen: Option<Point> = None;
    let mut whole = true;
    let clip = |seg: PathSeg, out: &mut BezPath, pen: &mut Option<Point>, whole: &mut bool| {
        let mut ts = vec![0., 1.];
        for edge in edges {
            ts.extend(seg.intersect_line(edge).iter().map(|hit| hit.segment_t));
        }
        ts.sort_by(f64::total_cmp);
        ts.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
        for pair in ts.windows(2) {
            let (t0, t1) = (pair[0], pair[1]);
            if !inside(seg.eval(0.5 * (t0 + t1))) {
                *pen = None;
                *whole = false;
                continue;
            }
            let piece = seg.subsegment(t0..t1);
            if pen.is_none_or(|p| p.distance(piece.start()) > 1e-9) {
                out.move_to(piece.start());
            }
            push_seg(out, piece);
            *pen = Some(piece.end());
        }
    };
    let (mut start, mut current) = (Point::ZERO, Point::ZERO);
    for el in path.elements() {
        let seg = match *el {
            PathEl::MoveTo(p) => {
                // Each input subpath starts its own output subpath.
                (start, current, pen, whole) = (p, p, None, true);
                continue;
            }
            PathEl::LineTo(p) => PathSeg::Line(Line::new(current, p)),
            PathEl::QuadTo(c, p) => PathSeg::Quad(kurbo::QuadBez::new(current, c, p)),
            PathEl::CurveTo(c1, c2, p) => PathSeg::Cubic(CubicBez::new(current, c1, c2, p)),
            PathEl::ClosePath => {
                if current != start {
                    clip(
                        Line::new(current, start).into(),
                        &mut out,
                        &mut pen,
                        &mut whole,
                    );
                }
                // A closed subpath that is entirely inside stays closed.
                if whole && pen.is_some() {
                    out.close_path();
                }
                (current, pen, whole) = (start, None, true);
                continue;
            }
        };
        current = seg.end();
        clip(seg, &mut out, &mut pen, &mut whole);
    }
    out
}

/// Append a segment's drawing element (without a move).
pub fn push_seg(path: &mut BezPath, seg: PathSeg) {
    match seg {
        PathSeg::Line(l) => path.line_to(l.p1),
        PathSeg::Quad(q) => path.quad_to(q.p1, q.p2),
        PathSeg::Cubic(c) => path.curve_to(c.p1, c.p2, c.p3),
    }
}

/// Flatten to polylines within `tolerance`, one per subpath. Closed subpaths
/// end with their first point repeated.
pub fn flatten_polylines(path: &BezPath, tolerance: f64) -> Vec<Vec<Point>> {
    let mut lines: Vec<Vec<Point>> = Vec::new();
    kurbo::flatten(path.iter(), tolerance.max(1e-9), |el| match el {
        PathEl::MoveTo(p) => lines.push(vec![p]),
        PathEl::LineTo(p) => {
            if let Some(line) = lines.last_mut() {
                line.push(p);
            }
        }
        PathEl::ClosePath => {
            if let Some(line) = lines.last_mut()
                && let Some(&first) = line.first()
                && line.last() != Some(&first)
            {
                line.push(first);
            }
        }
        _ => {}
    });
    lines
}

/// Fill rule for [`fill_contains`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FillRule {
    /// Inside where the winding number is not zero.
    #[default]
    NonZero,
    /// Inside where the winding number is odd.
    EvenOdd,
}

/// Whether `p` is inside the filled path. Open subpaths are treated as closed.
pub fn fill_contains(path: &BezPath, p: Point, rule: FillRule) -> bool {
    let winding = path.winding(p);
    match rule {
        FillRule::NonZero => winding != 0,
        FillRule::EvenOdd => winding % 2 != 0,
    }
}

/// The nearest point on a multi-segment path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PathHit {
    /// Index into [`BezPath::segments`].
    pub segment: usize,
    /// Parameter on that segment.
    pub t: f64,
    /// The nearest point.
    pub point: Point,
    /// Unsigned distance.
    pub distance: f64,
}

/// Nearest point on any segment of `path`; `None` for an empty path.
pub fn nearest_on_path(path: &BezPath, p: Point) -> Option<PathHit> {
    path.segments()
        .enumerate()
        .map(|(segment, seg)| {
            let hit = nearest(seg, p);
            PathHit {
                segment,
                t: hit.t,
                point: hit.point,
                distance: hit.distance,
            }
        })
        .min_by(|a, b| a.distance.total_cmp(&b.distance))
}

/// Whether `p` is within `width / 2` of the path's centerline.
pub fn stroke_contains(path: &BezPath, p: Point, width: f64) -> bool {
    nearest_on_path(path, p).is_some_and(|hit| hit.distance <= 0.5 * width)
}

#[cfg(test)]
#[path = "geom_tests.rs"]
mod tests;
