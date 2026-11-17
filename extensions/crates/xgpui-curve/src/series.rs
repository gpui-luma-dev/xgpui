//! Data-series curves for plots, following d3-shape's curve family, plus area
//! fills. Samples are connected in input order; a non-finite sample breaks the
//! line and each finite run is drawn independently.

use crate::func::pchip_slopes;
use crate::geom::{BezPath, Point, Rect, clip_to_rect};

/// How a series connects its samples.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum SeriesCurve {
    /// Straight segments.
    #[default]
    Linear,
    /// Horizontal then vertical, changing value halfway between samples.
    Step,
    /// Change value at each sample's x (vertical first).
    StepBefore,
    /// Hold each value until the next sample's x (horizontal first).
    StepAfter,
    /// Uniform cubic B-spline; passes through the first and last samples only.
    Basis,
    /// Cardinal spline through every sample; tension 0 is Catmull-Rom with
    /// uniform parameterization, 1 gives straight lines.
    Cardinal {
        /// Tension in `0..=1`.
        tension: f64,
    },
    /// Catmull-Rom through every sample; alpha 0.5 is centripetal (no cusps or
    /// self-intersections within a segment), 0 uniform, 1 chordal.
    CatmullRom {
        /// Alpha in `0..=1`.
        alpha: f64,
    },
    /// Natural cubic spline (C²) through every sample.
    Natural,
    /// Monotone in x (PCHIP); requires strictly increasing x, otherwise the run
    /// is drawn with straight segments.
    MonotoneX,
    /// Monotone in y (PCHIP with x and y swapped); requires strictly increasing y.
    MonotoneY,
    /// Basis spline over samples straightened toward the first–last chord;
    /// beta 1 is plain basis, 0 a straight line.
    Bundle {
        /// Beta in `0..=1`.
        beta: f64,
    },
}

/// Build the path for `points`, starting a new subpath after every non-finite sample.
pub fn series_path(points: &[Point], curve: SeriesCurve) -> BezPath {
    let mut path = BezPath::new();
    for run in finite_runs(points) {
        draw(&mut path, run, curve, true);
    }
    path
}

/// [`series_path`] clipped to `viewport` with [`clip_to_rect`].
pub fn series_path_clipped(points: &[Point], curve: SeriesCurve, viewport: Rect) -> BezPath {
    clip_to_rect(&series_path(points, curve), viewport)
}

/// The lower edge of an area fill.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Baseline<'a> {
    /// A horizontal line at this y.
    Constant(f64),
    /// A second series, sample for sample (same length as the top series).
    Series(&'a [Point]),
}

/// A closed area between the series and `baseline`, one subpath per finite
/// run. Both edges use `curve`, as d3's area does.
pub fn series_area(points: &[Point], curve: SeriesCurve, baseline: Baseline<'_>) -> BezPath {
    let mut path = BezPath::new();
    let bottom: Vec<Point> = match baseline {
        Baseline::Constant(y) => points.iter().map(|p| Point::new(p.x, y)).collect(),
        Baseline::Series(series) => (0..points.len())
            .map(|i| {
                series
                    .get(i)
                    .copied()
                    .unwrap_or(Point::new(f64::NAN, f64::NAN))
            })
            .collect(),
    };
    let mut start = 0;
    while start < points.len() {
        let ok = |i: usize| finite(points[i]) && finite(bottom[i]);
        if !ok(start) {
            start += 1;
            continue;
        }
        let mut end = start;
        while end < points.len() && ok(end) {
            end += 1;
        }
        draw(&mut path, &points[start..end], curve, true);
        let reversed: Vec<Point> = bottom[start..end].iter().rev().copied().collect();
        draw(&mut path, &reversed, curve, false);
        path.close_path();
        start = end;
    }
    path
}

fn finite(p: Point) -> bool {
    p.x.is_finite() && p.y.is_finite()
}

fn finite_runs(points: &[Point]) -> impl Iterator<Item = &[Point]> {
    points.split(|p| !finite(*p)).filter(|run| !run.is_empty())
}

/// Draw one run. `start` moves to the first point; otherwise the run
/// continues the current subpath with a line to its first point.
fn draw(path: &mut BezPath, p: &[Point], curve: SeriesCurve, start: bool) {
    if start {
        path.move_to(p[0]);
    } else {
        path.line_to(p[0]);
    }
    let n = p.len();
    if n == 1 {
        return;
    }
    match curve {
        SeriesCurve::Linear => p[1..].iter().for_each(|q| path.line_to(*q)),
        SeriesCurve::Step => step(path, p, 0.5),
        SeriesCurve::StepBefore => step(path, p, 0.),
        SeriesCurve::StepAfter => step(path, p, 1.),
        SeriesCurve::Basis => basis(path, p),
        SeriesCurve::Cardinal { tension } => cardinal(path, p, tension.clamp(0., 1.)),
        SeriesCurve::CatmullRom { alpha } => {
            let alpha = alpha.clamp(0., 1.);
            if alpha == 0. {
                cardinal(path, p, 0.)
            } else {
                catmull_rom(path, p, alpha)
            }
        }
        SeriesCurve::Natural => natural(path, p),
        SeriesCurve::MonotoneX => monotone(path, p, false),
        SeriesCurve::MonotoneY => monotone(path, p, true),
        SeriesCurve::Bundle { beta } => {
            let beta = beta.clamp(0., 1.);
            let (first, last) = (p[0], p[n - 1]);
            let straightened: Vec<Point> = p
                .iter()
                .enumerate()
                .map(|(i, q)| {
                    let t = i as f64 / (n - 1) as f64;
                    let chord = first.lerp(last, t);
                    Point::new(
                        beta * q.x + (1. - beta) * chord.x,
                        beta * q.y + (1. - beta) * chord.y,
                    )
                })
                .collect();
            basis(path, &straightened);
        }
    }
}

fn step(path: &mut BezPath, p: &[Point], t: f64) {
    for w in p.windows(2) {
        let (a, b) = (w[0], w[1]);
        if t <= 0. {
            path.line_to((a.x, b.y));
            path.line_to(b);
        } else {
            let x = a.x * (1. - t) + b.x * t;
            path.line_to((x, a.y));
            path.line_to((x, b.y));
        }
    }
    if 0. < t && t < 1. {
        path.line_to(p[p.len() - 1]);
    }
}

fn basis(path: &mut BezPath, p: &[Point]) {
    let n = p.len();
    if n == 2 {
        path.line_to(p[1]);
        return;
    }
    let lerp = |a: Point, b: Point, wa: f64, wb: f64| {
        Point::new(
            (wa * a.x + wb * b.x) / (wa + wb),
            (wa * a.y + wb * b.y) / (wa + wb),
        )
    };
    path.line_to(lerp(p[0], p[1], 5., 1.));
    // Each sample after the second emits the B-spline span for (x0, x1, x).
    let mut span = |x0: Point, x1: Point, x: Point| {
        path.curve_to(
            lerp(x0, x1, 2., 1.),
            lerp(x0, x1, 1., 2.),
            Point::new((x0.x + 4. * x1.x + x.x) / 6., (x0.y + 4. * x1.y + x.y) / 6.),
        );
    };
    for i in 2..n {
        span(p[i - 2], p[i - 1], p[i]);
    }
    span(p[n - 2], p[n - 1], p[n - 1]);
    path.line_to(p[n - 1]);
}

fn cardinal(path: &mut BezPath, p: &[Point], tension: f64) {
    let n = p.len();
    if n == 2 {
        path.line_to(p[1]);
        return;
    }
    let k = (1. - tension) / 6.;
    for i in 0..n - 1 {
        let prev = if i > 0 { p[i - 1] } else { p[i + 1] };
        let next = if i + 2 < n { p[i + 2] } else { p[i] };
        let (a, b) = (p[i], p[i + 1]);
        path.curve_to(a + k * (b - prev), b + k * (a - next), b);
    }
}

fn catmull_rom(path: &mut BezPath, p: &[Point], alpha: f64) {
    let n = p.len();
    if n == 2 {
        path.line_to(p[1]);
        return;
    }
    let pow = |a: Point, b: Point| (a - b).hypot2().powf(alpha);
    const EPS: f64 = 1e-12;
    for i in 0..n - 1 {
        let (p1, p2) = (p[i], p[i + 1]);
        let l12_2a = pow(p1, p2);
        let l12_a = l12_2a.sqrt();
        let mut c1 = p1;
        let mut c2 = p2;
        if i > 0 {
            let p0 = p[i - 1];
            let l01_2a = pow(p0, p1);
            let l01_a = l01_2a.sqrt();
            if l01_a > EPS {
                let a = 2. * l01_2a + 3. * l01_a * l12_a + l12_2a;
                let m = 3. * l01_a * (l01_a + l12_a);
                c1 = Point::new(
                    (p1.x * a - p0.x * l12_2a + p2.x * l01_2a) / m,
                    (p1.y * a - p0.y * l12_2a + p2.y * l01_2a) / m,
                );
            }
        }
        if i + 2 < n {
            let p3 = p[i + 2];
            let l23_2a = pow(p2, p3);
            let l23_a = l23_2a.sqrt();
            if l23_a > EPS {
                let b = 2. * l23_2a + 3. * l23_a * l12_a + l12_2a;
                let m = 3. * l23_a * (l23_a + l12_a);
                c2 = Point::new(
                    (p2.x * b + p1.x * l23_2a - p3.x * l12_2a) / m,
                    (p2.y * b + p1.y * l23_2a - p3.y * l12_2a) / m,
                );
            }
        }
        path.curve_to(c1, c2, p2);
    }
}

/// d3's natural-spline control points for one coordinate.
fn natural_controls(x: &[f64]) -> (Vec<f64>, Vec<f64>) {
    let n = x.len() - 1;
    let mut a = vec![0.; n];
    let mut b = vec![0.; n];
    let mut r = vec![0.; n];
    a[0] = 0.;
    b[0] = 2.;
    r[0] = x[0] + 2. * x[1];
    for i in 1..n - 1 {
        a[i] = 1.;
        b[i] = 4.;
        r[i] = 4. * x[i] + 2. * x[i + 1];
    }
    a[n - 1] = 2.;
    b[n - 1] = 7.;
    r[n - 1] = 8. * x[n - 1] + x[n];
    for i in 1..n {
        let m = a[i] / b[i - 1];
        b[i] -= m;
        r[i] -= m * r[i - 1];
    }
    a[n - 1] = r[n - 1] / b[n - 1];
    for i in (0..n - 1).rev() {
        a[i] = (r[i] - a[i + 1]) / b[i];
    }
    b[n - 1] = (x[n] + a[n - 1]) / 2.;
    for i in 0..n - 1 {
        b[i] = 2. * x[i + 1] - a[i + 1];
    }
    (a, b)
}

fn natural(path: &mut BezPath, p: &[Point]) {
    if p.len() == 2 {
        path.line_to(p[1]);
        return;
    }
    let xs: Vec<f64> = p.iter().map(|q| q.x).collect();
    let ys: Vec<f64> = p.iter().map(|q| q.y).collect();
    let (ax, bx) = natural_controls(&xs);
    let (ay, by) = natural_controls(&ys);
    for i in 0..p.len() - 1 {
        path.curve_to(Point::new(ax[i], ay[i]), Point::new(bx[i], by[i]), p[i + 1]);
    }
}

fn monotone(path: &mut BezPath, p: &[Point], swap: bool) {
    let flip = |q: Point| if swap { Point::new(q.y, q.x) } else { q };
    let q: Vec<Point> = p.iter().map(|v| flip(*v)).collect();
    let xs: Vec<f64> = q.iter().map(|v| v.x).collect();
    let ys: Vec<f64> = q.iter().map(|v| v.y).collect();
    let slopes = (q.len() >= 3)
        .then(|| pchip_slopes(&xs, &ys).ok())
        .flatten();
    let Some(d) = slopes else {
        p[1..].iter().for_each(|v| path.line_to(*v));
        return;
    };
    for i in 0..q.len() - 1 {
        let h = (xs[i + 1] - xs[i]) / 3.;
        let (lo, hi) = (ys[i].min(ys[i + 1]), ys[i].max(ys[i + 1]));
        let c1 = Point::new(xs[i] + h, (ys[i] + h * d[i]).clamp(lo, hi));
        let c2 = Point::new(xs[i + 1] - h, (ys[i + 1] - h * d[i + 1]).clamp(lo, hi));
        path.curve_to(flip(c1), flip(c2), p[i + 1]);
    }
}

#[cfg(test)]
#[path = "series_tests.rs"]
mod tests;
