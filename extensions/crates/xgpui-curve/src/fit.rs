//! Freehand curve fitting: Ramer–Douglas–Peucker simplification and
//! Schneider's least-squares cubic fitting ("An Algorithm for Automatically
//! Fitting Digitized Curves", Graphics Gems, 1990), with corner splitting.

use crate::error::{CurveError, finite};
use crate::geom::{BezPath, CubicBez, ParamCurve, Point, Vec2};

/// Options for [`fit_cubic_path`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FitOptions {
    /// Largest allowed distance from an input point to the fitted curve.
    pub tolerance: f64,
    /// Turns sharper than this (radians) between successive input steps
    /// split the fit and stay sharp. `None` disables corner detection.
    pub corner_angle: Option<f64>,
    /// Optional RDP pre-pass epsilon to thin dense input first.
    pub simplify: Option<f64>,
}

impl Default for FitOptions {
    fn default() -> Self {
        Self {
            tolerance: 1.,
            corner_angle: Some(std::f64::consts::FRAC_PI_3),
            simplify: None,
        }
    }
}

/// Most Newton reparameterization passes per fit attempt.
const MAX_ITERATIONS: usize = 4;

/// Ramer–Douglas–Peucker: the subset of `points` within `epsilon` of the
/// original polyline, always keeping the first and last point.
pub fn rdp(points: &[Point], epsilon: f64) -> Vec<Point> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut stack = vec![(0, points.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        let (pa, pb) = (points[a], points[b]);
        let chord = pb - pa;
        let len = chord.hypot();
        let mut best = (a, 0.);
        for (i, &p) in points.iter().enumerate().take(b).skip(a + 1) {
            let d = if len > 0. {
                chord.cross(p - pa).abs() / len
            } else {
                (p - pa).hypot()
            };
            if d > best.1 {
                best = (i, d);
            }
        }
        if best.1 > epsilon {
            keep[best.0] = true;
            stack.push((a, best.0));
            stack.push((best.0, b));
        }
    }
    points
        .iter()
        .zip(keep)
        .filter_map(|(p, k)| k.then_some(*p))
        .collect()
}

/// Fit a smooth cubic path through a pointer or pen point stream.
pub fn fit_cubic_path(points: &[Point], options: FitOptions) -> Result<BezPath, CurveError> {
    for p in points {
        finite(&[p.x, p.y])?;
    }
    if !(options.tolerance.is_finite() && options.tolerance > 0.) {
        return Err(CurveError::OutOfRange("fit tolerance"));
    }
    let mut pts: Vec<Point> = Vec::with_capacity(points.len());
    for &p in points {
        if pts.last() != Some(&p) {
            pts.push(p);
        }
    }
    if let Some(eps) = options.simplify {
        // RDP can bring equal points together again (a stroke that returns
        // to its start), and a zero step has no tangent.
        pts = rdp(&pts, eps.max(0.));
        pts.dedup();
    }
    let mut path = BezPath::new();
    let Some(&first) = pts.first() else {
        return Ok(path);
    };
    path.move_to(first);
    if pts.len() == 1 {
        return Ok(path);
    }
    // Split at corners.
    let mut breaks = vec![0];
    if let Some(limit) = options.corner_angle {
        for i in 1..pts.len() - 1 {
            let (a, b) = (pts[i] - pts[i - 1], pts[i + 1] - pts[i]);
            if a.cross(b).atan2(a.dot(b)).abs() > limit {
                breaks.push(i);
            }
        }
    }
    breaks.push(pts.len() - 1);
    for w in breaks.windows(2) {
        let piece = &pts[w[0]..=w[1]];
        let t1 = (piece[1] - piece[0]).normalize();
        let t2 = (piece[piece.len() - 2] - piece[piece.len() - 1]).normalize();
        fit_piece(piece, t1, t2, options.tolerance, &mut path);
    }
    Ok(path)
}

fn fit_piece(p: &[Point], t1: Vec2, t2: Vec2, tolerance: f64, out: &mut BezPath) {
    let n = p.len();
    if n == 2 {
        let dist = (p[1] - p[0]).hypot() / 3.;
        out.curve_to(p[0] + t1 * dist, p[1] + t2 * dist, p[1]);
        return;
    }
    let mut u = chord_params(p);
    let mut bez = generate(p, &u, t1, t2);
    let (mut error, mut split) = max_error(p, &bez, &u);
    if error <= tolerance {
        out.curve_to(bez.p1, bez.p2, bez.p3);
        return;
    }
    if error <= tolerance * 4. {
        for _ in 0..MAX_ITERATIONS {
            u = reparameterize(p, &u, &bez);
            bez = generate(p, &u, t1, t2);
            (error, split) = max_error(p, &bez, &u);
            if error <= tolerance {
                out.curve_to(bez.p1, bez.p2, bez.p3);
                return;
            }
        }
    }
    let split = split.clamp(1, n - 2);
    let mut center = p[split - 1] - p[split + 1];
    if center.hypot() <= 1e-300 {
        center = Vec2::new(-(p[split] - p[split - 1]).y, (p[split] - p[split - 1]).x);
    }
    let center = center.normalize();
    fit_piece(&p[..=split], t1, center, tolerance, out);
    fit_piece(&p[split..], -center, t2, tolerance, out);
}

fn chord_params(p: &[Point]) -> Vec<f64> {
    let mut u = vec![0.; p.len()];
    for i in 1..p.len() {
        u[i] = u[i - 1] + (p[i] - p[i - 1]).hypot();
    }
    let total = u[p.len() - 1];
    u.iter()
        .map(|v| if total > 0. { v / total } else { 0. })
        .collect()
}

fn bernstein(u: f64) -> [f64; 4] {
    let v = 1. - u;
    [v * v * v, 3. * u * v * v, 3. * u * u * v, u * u * u]
}

fn generate(p: &[Point], u: &[f64], t1: Vec2, t2: Vec2) -> CubicBez {
    let (first, last) = (p[0], p[p.len() - 1]);
    let (mut c00, mut c01, mut c11, mut x0, mut x1) = (0., 0., 0., 0., 0.);
    for (pi, &ui) in p.iter().zip(u) {
        let b = bernstein(ui);
        let (a1, a2) = (t1 * b[1], t2 * b[2]);
        c00 += a1.dot(a1);
        c01 += a1.dot(a2);
        c11 += a2.dot(a2);
        let tmp =
            *pi - (first.to_vec2() * (b[0] + b[1]) + last.to_vec2() * (b[2] + b[3])).to_point();
        x0 += a1.dot(tmp);
        x1 += a2.dot(tmp);
    }
    let det = c00 * c11 - c01 * c01;
    let (mut al, mut ar) = if det.abs() > 1e-12 * (c00 * c11).max(1e-300) {
        ((x0 * c11 - x1 * c01) / det, (c00 * x1 - c01 * x0) / det)
    } else {
        (0., 0.)
    };
    let seg = (last - first).hypot();
    let eps = 1e-6 * seg;
    if al < eps || ar < eps || !al.is_finite() || !ar.is_finite() {
        al = seg / 3.;
        ar = seg / 3.;
    }
    CubicBez::new(first, first + t1 * al, last + t2 * ar, last)
}

fn max_error(p: &[Point], bez: &CubicBez, u: &[f64]) -> (f64, usize) {
    let mut best = (0., p.len() / 2);
    for i in 1..p.len() - 1 {
        let d = (bez.eval(u[i]) - p[i]).hypot();
        if d > best.0 {
            best = (d, i);
        }
    }
    best
}

fn reparameterize(p: &[Point], u: &[f64], bez: &CubicBez) -> Vec<f64> {
    let d1 = |t: f64| {
        let v = 1. - t;
        3. * v * v * (bez.p1 - bez.p0)
            + 6. * v * t * (bez.p2 - bez.p1)
            + 3. * t * t * (bez.p3 - bez.p2)
    };
    let d2 = |t: f64| {
        6. * (1. - t) * (bez.p2 - 2. * bez.p1.to_vec2() + bez.p0.to_vec2()).to_vec2()
            + 6. * t * (bez.p3 - 2. * bez.p2.to_vec2() + bez.p1.to_vec2()).to_vec2()
    };
    p.iter()
        .zip(u)
        .map(|(pi, &t)| {
            let diff = bez.eval(t) - *pi;
            let (q1, q2) = (d1(t), d2(t));
            let den = q1.dot(q1) + diff.dot(q2);
            if den.abs() < 1e-300 {
                t
            } else {
                (t - diff.dot(q1) / den).clamp(0., 1.)
            }
        })
        .collect()
}

#[cfg(test)]
#[path = "fit_tests.rs"]
mod tests;
