//! One-dimensional functional curves y = f(x): monotone (PCHIP), linear and
//! x-clamped Bézier transfer curves, CSS easing functions, and LUT baking.

use crate::error::{CurveError, finite};
use crate::geom::Point;

fn sign(v: f64) -> f64 {
    if v > 0. {
        1.
    } else if v < 0. {
        -1.
    } else {
        0.
    }
}

/// PCHIP slopes (shape-preserving piecewise cubic Hermite), matching scipy's
/// `PchipInterpolator`: weighted harmonic means of adjacent secants inside,
/// three-point shape-preserving estimates at the ends, zero at local extrema.
/// Two points give a straight line.
pub fn pchip_slopes(x: &[f64], y: &[f64]) -> Result<Vec<f64>, CurveError> {
    check_knots(x, y)?;
    let n = x.len();
    let h: Vec<f64> = x.windows(2).map(|w| w[1] - w[0]).collect();
    let m: Vec<f64> = (0..n - 1).map(|i| (y[i + 1] - y[i]) / h[i]).collect();
    if n == 2 {
        return Ok(vec![m[0], m[0]]);
    }
    let mut d = vec![0.; n];
    for k in 1..n - 1 {
        if m[k - 1] * m[k] > 0. {
            let w1 = 2. * h[k] + h[k - 1];
            let w2 = h[k] + 2. * h[k - 1];
            d[k] = (w1 + w2) / (w1 / m[k - 1] + w2 / m[k]);
        }
    }
    d[0] = pchip_end(h[0], h[1], m[0], m[1]);
    d[n - 1] = pchip_end(h[n - 2], h[n - 3], m[n - 2], m[n - 3]);
    Ok(d)
}

fn pchip_end(h0: f64, h1: f64, m0: f64, m1: f64) -> f64 {
    let d = ((2. * h0 + h1) * m0 - h0 * m1) / (h0 + h1);
    if sign(d) != sign(m0) {
        0.
    } else if sign(m0) != sign(m1) && d.abs() > 3. * m0.abs() {
        3. * m0
    } else {
        d
    }
}

fn check_knots(x: &[f64], y: &[f64]) -> Result<(), CurveError> {
    if x.len() != y.len() {
        return Err(CurveError::Dimensions);
    }
    if x.len() < 2 {
        return Err(CurveError::TooFewPoints { required: 2, actual: x.len() });
    }
    finite(x)?;
    finite(y)?;
    if x.windows(2).any(|w| w[1] <= w[0]) {
        return Err(CurveError::NotIncreasing);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Segment {
    Linear { y0: f64, y1: f64 },
    Hermite { y0: f64, y1: f64, d0: f64, d1: f64 },
    /// x-monotone cubic Bézier.
    Bezier { p0: Point, c1: Point, c2: Point, p3: Point },
}

/// y = f(x) over strictly increasing knots. Outside the knot range the
/// curve holds its end values.
#[derive(Clone, Debug, PartialEq)]
pub struct FunctionalCurve {
    xs: Vec<f64>,
    segments: Vec<Segment>,
}

impl FunctionalCurve {
    /// Straight segments between knots.
    pub fn linear(points: &[Point]) -> Result<Self, CurveError> {
        let (xs, ys) = split_points(points);
        check_knots(&xs, &ys)?;
        let segments = ys.windows(2).map(|w| Segment::Linear { y0: w[0], y1: w[1] }).collect();
        Ok(Self { xs, segments })
    }

    /// Monotone cubic Hermite through the knots with [`pchip_slopes`]: no
    /// overshoot on monotone or step data.
    pub fn monotone(points: &[Point]) -> Result<Self, CurveError> {
        let (xs, ys) = split_points(points);
        let d = pchip_slopes(&xs, &ys)?;
        let segments = (0..xs.len() - 1)
            .map(|i| Segment::Hermite { y0: ys[i], y1: ys[i + 1], d0: d[i], d1: d[i + 1] })
            .collect();
        Ok(Self { xs, segments })
    }

    /// Cubic Bézier segments between knots, with `handles[i] = (c1, c2)` for
    /// the segment from knot `i` to `i + 1`. Handle x is clamped into the
    /// segment's x range, which keeps x(t) monotone so every x has one y.
    pub fn bezier(points: &[Point], handles: &[(Point, Point)]) -> Result<Self, CurveError> {
        let (xs, ys) = split_points(points);
        check_knots(&xs, &ys)?;
        if handles.len() != points.len() - 1 {
            return Err(CurveError::Dimensions);
        }
        let mut segments = Vec::with_capacity(handles.len());
        for (i, &(c1, c2)) in handles.iter().enumerate() {
            finite(&[c1.x, c1.y, c2.x, c2.y])?;
            let (x0, x1) = (xs[i], xs[i + 1]);
            segments.push(Segment::Bezier {
                p0: points[i],
                c1: Point::new(c1.x.clamp(x0, x1), c1.y),
                c2: Point::new(c2.x.clamp(x0, x1), c2.y),
                p3: points[i + 1],
            });
        }
        Ok(Self { xs, segments })
    }

    /// The knot x range.
    pub fn domain(&self) -> (f64, f64) {
        (self.xs[0], *self.xs.last().unwrap())
    }

    fn locate(&self, x: f64) -> (usize, f64) {
        let (lo, hi) = self.domain();
        let x = if x.is_nan() { lo } else { x.clamp(lo, hi) };
        let i = self.xs.partition_point(|k| *k <= x).saturating_sub(1).min(self.segments.len() - 1);
        (i, x)
    }

    /// f(x), in O(log n).
    pub fn eval(&self, x: f64) -> f64 {
        let (i, x) = self.locate(x);
        let (x0, x1) = (self.xs[i], self.xs[i + 1]);
        let h = x1 - x0;
        let s = (x - x0) / h;
        match self.segments[i] {
            Segment::Linear { y0, y1 } => y0 + (y1 - y0) * s,
            Segment::Hermite { y0, y1, d0, d1 } => {
                let (s2, s3) = (s * s, s * s * s);
                (2. * s3 - 3. * s2 + 1.) * y0 + (s3 - 2. * s2 + s) * h * d0 + (-2. * s3 + 3. * s2) * y1 + (s3 - s2) * h * d1
            }
            Segment::Bezier { p0, c1, c2, p3 } => {
                let t = solve_monotone_cubic(p0.x, c1.x, c2.x, p3.x, x);
                cubic(p0.y, c1.y, c2.y, p3.y, t)
            }
        }
    }

    /// f′(x). At a knot, the derivative of the segment starting there.
    pub fn eval_derivative(&self, x: f64) -> f64 {
        let (i, x) = self.locate(x);
        let (x0, x1) = (self.xs[i], self.xs[i + 1]);
        let h = x1 - x0;
        let s = (x - x0) / h;
        match self.segments[i] {
            Segment::Linear { y0, y1 } => (y1 - y0) / h,
            Segment::Hermite { y0, y1, d0, d1 } => {
                let s2 = s * s;
                ((6. * s2 - 6. * s) * y0 + (3. * s2 - 4. * s + 1.) * h * d0 + (-6. * s2 + 6. * s) * y1 + (3. * s2 - 2. * s) * h * d1) / h
            }
            Segment::Bezier { p0, c1, c2, p3 } => {
                let t = solve_monotone_cubic(p0.x, c1.x, c2.x, p3.x, x);
                let dx = cubic_deriv(p0.x, c1.x, c2.x, p3.x, t);
                let dy = cubic_deriv(p0.y, c1.y, c2.y, p3.y, t);
                if dx.abs() > 1e-300 { dy / dx } else { 0. }
            }
        }
    }

    /// Sample `N` evenly spaced x values across the domain into a table
    /// clamped to `[0, 1]` (non-finite results become 0).
    pub fn to_lut_f32<const N: usize>(&self) -> [f32; N] {
        let mut lut = [0f32; N];
        self.fill_lut_f32(&mut lut);
        lut
    }

    /// Fill `lut` like [`Self::to_lut_f32`].
    pub fn fill_lut_f32(&self, lut: &mut [f32]) {
        let (lo, hi) = self.domain();
        let n = lut.len();
        for (i, slot) in lut.iter_mut().enumerate() {
            let x = if n <= 1 { lo } else { lo + (hi - lo) * i as f64 / (n - 1) as f64 };
            let y = self.eval(x);
            *slot = if y.is_finite() { y.clamp(0., 1.) as f32 } else { 0. };
        }
    }
}

fn split_points(points: &[Point]) -> (Vec<f64>, Vec<f64>) {
    (points.iter().map(|p| p.x).collect(), points.iter().map(|p| p.y).collect())
}

fn cubic(a: f64, b: f64, c: f64, d: f64, t: f64) -> f64 {
    let u = 1. - t;
    u * u * u * a + 3. * u * u * t * b + 3. * u * t * t * c + t * t * t * d
}

fn cubic_deriv(a: f64, b: f64, c: f64, d: f64, t: f64) -> f64 {
    let u = 1. - t;
    3. * u * u * (b - a) + 6. * u * t * (c - b) + 3. * t * t * (d - c)
}

/// t in [0, 1] with x(t) = x for a non-decreasing cubic x(t): Newton steps
/// kept inside a shrinking bisection bracket.
fn solve_monotone_cubic(a: f64, b: f64, c: f64, d: f64, x: f64) -> f64 {
    if x <= a {
        return 0.;
    }
    if x >= d {
        return 1.;
    }
    let (mut lo, mut hi) = (0., 1.);
    let mut t = (x - a) / (d - a);
    for _ in 0..64 {
        let f = cubic(a, b, c, d, t) - x;
        if f.abs() <= 1e-15 * (1. + x.abs()) {
            break;
        }
        if f > 0. {
            hi = t;
        } else {
            lo = t;
        }
        let df = cubic_deriv(a, b, c, d, t);
        let next = t - f / df;
        t = if df > 0. && next > lo && next < hi { next } else { 0.5 * (lo + hi) };
        if hi - lo <= 1e-16 {
            break;
        }
    }
    t
}

/// Where the jumps of a CSS `steps()` function fall.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StepPosition {
    /// `jump-start` / `start`: the first jump happens at 0.
    JumpStart,
    /// `jump-end` / `end`: the last jump happens at 1.
    #[default]
    JumpEnd,
    /// `jump-none`: no jump at either end.
    JumpNone,
    /// `jump-both`: jumps at both 0 and 1.
    JumpBoth,
}

/// One stop of a CSS `linear()` easing function.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinearStop {
    /// Output value.
    pub output: f64,
    /// Input position; `None` is filled in as CSS does.
    pub input: Option<f64>,
}

/// CSS easing functions (CSS Easing Functions Level 2).
#[derive(Clone, Debug, PartialEq)]
pub enum Easing {
    /// `linear`: the identity.
    Linear,
    /// `cubic-bezier(x1, y1, x2, y2)` with x1, x2 in `[0, 1]`.
    CubicBezier {
        /// First control x.
        x1: f64,
        /// First control y.
        y1: f64,
        /// Second control x.
        x2: f64,
        /// Second control y.
        y2: f64,
    },
    /// `steps(count, position)`.
    Steps {
        /// Number of steps.
        count: u32,
        /// Jump placement.
        position: StepPosition,
    },
    /// `linear(...)` with resolved stops as (input, output).
    Piecewise(Vec<(f64, f64)>),
}

impl Easing {
    /// `ease`.
    pub const EASE: Easing = Easing::CubicBezier { x1: 0.25, y1: 0.1, x2: 0.25, y2: 1. };
    /// `ease-in`.
    pub const EASE_IN: Easing = Easing::CubicBezier { x1: 0.42, y1: 0., x2: 1., y2: 1. };
    /// `ease-out`.
    pub const EASE_OUT: Easing = Easing::CubicBezier { x1: 0., y1: 0., x2: 0.58, y2: 1. };
    /// `ease-in-out`.
    pub const EASE_IN_OUT: Easing = Easing::CubicBezier { x1: 0.42, y1: 0., x2: 0.58, y2: 1. };
    /// `step-start`.
    pub const STEP_START: Easing = Easing::Steps { count: 1, position: StepPosition::JumpStart };
    /// `step-end`.
    pub const STEP_END: Easing = Easing::Steps { count: 1, position: StepPosition::JumpEnd };

    /// Validated `cubic-bezier()`.
    pub fn cubic_bezier(x1: f64, y1: f64, x2: f64, y2: f64) -> Result<Self, CurveError> {
        finite(&[x1, y1, x2, y2])?;
        if !(0. ..=1.).contains(&x1) || !(0. ..=1.).contains(&x2) {
            return Err(CurveError::OutOfRange("cubic-bezier x1 and x2 must be within 0..=1"));
        }
        Ok(Self::CubicBezier { x1, y1, x2, y2 })
    }

    /// Validated `steps()`: at least 1 step, or 2 for `jump-none`.
    pub fn steps(count: u32, position: StepPosition) -> Result<Self, CurveError> {
        let min = if position == StepPosition::JumpNone { 2 } else { 1 };
        if count < min {
            return Err(CurveError::OutOfRange("steps count"));
        }
        Ok(Self::Steps { count, position })
    }

    /// `linear()` from at least two stops, resolving missing inputs as CSS
    /// does: first 0, last 1, inputs never decrease, gaps spaced evenly.
    pub fn linear_stops(stops: &[LinearStop]) -> Result<Self, CurveError> {
        if stops.len() < 2 {
            return Err(CurveError::TooFewPoints { required: 2, actual: stops.len() });
        }
        let mut inputs: Vec<Option<f64>> = stops.iter().map(|s| s.input).collect();
        for s in stops {
            finite(&[s.output])?;
            if let Some(i) = s.input {
                finite(&[i])?;
            }
        }
        let last = inputs.len() - 1;
        inputs[0].get_or_insert(0.);
        inputs[last].get_or_insert(1.);
        let mut largest = f64::NEG_INFINITY;
        for v in inputs.iter_mut().flatten() {
            largest = largest.max(*v);
            *v = largest;
        }
        let mut i = 0;
        while i < inputs.len() {
            if inputs[i].is_some() {
                i += 1;
                continue;
            }
            let start = i - 1;
            let mut end = i;
            while inputs[end].is_none() {
                end += 1;
            }
            let (a, b) = (inputs[start].unwrap(), inputs[end].unwrap());
            for (k, slot) in inputs.iter_mut().enumerate().take(end).skip(i) {
                *slot = Some(a + (b - a) * (k - start) as f64 / (end - start) as f64);
            }
            i = end;
        }
        Ok(Self::Piecewise(inputs.into_iter().zip(stops).map(|(i, s)| (i.unwrap(), s.output)).collect()))
    }

    /// Output progress for input progress `x` (any real; extrapolates as CSS does).
    pub fn eval(&self, x: f64) -> f64 {
        match *self {
            Self::Linear => x,
            Self::CubicBezier { x1, y1, x2, y2 } => {
                if x < 0. {
                    let slope = if x1 > 0. {
                        y1 / x1
                    } else if y1 == 0. && x2 > 0. {
                        y2 / x2
                    } else {
                        0.
                    };
                    return slope * x;
                }
                if x > 1. {
                    let slope = if x2 < 1. {
                        (y2 - 1.) / (x2 - 1.)
                    } else if y2 == 1. && x1 < 1. {
                        (y1 - 1.) / (x1 - 1.)
                    } else {
                        0.
                    };
                    return 1. + slope * (x - 1.);
                }
                let t = solve_monotone_cubic(0., x1, x2, 1., x);
                cubic(0., y1, y2, 1., t)
            }
            Self::Steps { count, position } => {
                let n = count as f64;
                let mut step = (x * n).floor();
                if matches!(position, StepPosition::JumpStart | StepPosition::JumpBoth) {
                    step += 1.;
                }
                let jumps = match position {
                    StepPosition::JumpStart | StepPosition::JumpEnd => n,
                    StepPosition::JumpNone => n - 1.,
                    StepPosition::JumpBoth => n + 1.,
                };
                if x >= 0. && step < 0. {
                    step = 0.;
                }
                if x <= 1. && step > jumps {
                    step = jumps;
                }
                step / jumps
            }
            Self::Piecewise(ref points) => {
                let a = points.iter().rposition(|(i, _)| *i <= x).unwrap_or(0).min(points.len() - 2);
                let (pa, pb) = (points[a], points[a + 1]);
                if pb.0 == pa.0 {
                    return pb.1;
                }
                pa.1 + (pb.1 - pa.1) * (x - pa.0) / (pb.0 - pa.0)
            }
        }
    }
}

#[cfg(test)]
#[path = "func_tests.rs"]
mod tests;
