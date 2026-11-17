//! Curved field spines: a quadratic Bézier from an origin to an endpoint that
//! a color field can bend along, with closed-form projection, (s, d⊥) local
//! coordinates, an arrowhead from the terminal tangent, a seam rule, and
//! matching WGSL and MSL sources.
//!
//! Spines are quadratic so the per-pixel projection has a closed form (a cubic
//! equation). The projection follows the same math as the reference GPU
//! technique (depressed cubic, trigonometric roots for three real roots,
//! hyperbolic/Cardano for one), reimplemented here from the published math,
//! with every degenerate case kept finite.

use crate::error::{CurveError, finite};
use crate::geom::{self, ParamCurve, PathSeg, Point, QuadBez, Vec2};
use crate::measure::quad_arclen;

/// WGSL helpers: `curve_spine_eval`, `curve_spine_arclen` and
/// `curve_spine_project(p, p0, c, p1, t_min, t_max) -> vec4<f32>` returning
/// (signed distance, t, normalized arc position s, total length).
pub const CURVE_SPINE_WGSL: &str = include_str!("spine.wgsl");

/// The same helpers in the Metal Shading Language (`float2`/`float4`).
pub const CURVE_SPINE_MSL: &str = include_str!("spine.metal");

/// A pixel's coordinates relative to a spine.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct SpinePoint {
    /// Parameter of the nearest spine point.
    pub t: f64,
    /// The nearest spine point.
    pub point: Point,
    /// Perpendicular distance, positive on the normal side (the tangent turned +90°).
    pub distance: f64,
    /// Arc length from the origin to the nearest point.
    pub arc_length: f64,
    /// `arc_length` divided by the spine length (0 for a zero-length spine).
    pub s: f64,
}

/// A quadratic spine `p0 → control → p1`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CurveSpine {
    /// Origin.
    pub p0: Point,
    /// Bend control point.
    pub control: Point,
    /// Endpoint.
    pub p1: Point,
}

impl CurveSpine {
    /// A spine through explicit control points.
    pub fn new(p0: Point, control: Point, p1: Point) -> Result<Self, CurveError> {
        finite(&[p0.x, p0.y, control.x, control.y, p1.x, p1.y])?;
        Ok(Self { p0, control, p1 })
    }

    /// A straight spine (the control at the chord midpoint).
    pub fn straight(p0: Point, p1: Point) -> Self {
        Self {
            p0,
            control: p0.midpoint(p1),
            p1,
        }
    }

    /// A spine whose control sits `bend` chord lengths off the chord midpoint,
    /// on the normal side. `bend = 0` is straight.
    pub fn with_bend(p0: Point, p1: Point, bend: f64) -> Self {
        let chord = p1 - p0;
        let normal = Vec2::new(-chord.y, chord.x);
        Self {
            p0,
            control: p0.midpoint(p1) + normal * bend,
            p1,
        }
    }

    /// The spine as a kurbo quadratic.
    pub fn quad(&self) -> QuadBez {
        QuadBez::new(self.p0, self.control, self.p1)
    }

    /// Total arc length.
    pub fn length(&self) -> f64 {
        quad_arclen(self.quad(), 0., 1.)
    }

    /// Unit tangent at the endpoint, for orienting an arrowhead.
    pub fn terminal_tangent(&self) -> Option<Vec2> {
        geom::tangent(PathSeg::Quad(self.quad()), 1.)
    }

    /// Arrowhead `[left wing, tip, right wing]` at the endpoint, with wings
    /// `length` long at `half_angle` radians either side of the reversed
    /// terminal tangent.
    pub fn arrowhead(&self, length: f64, half_angle: f64) -> Option<[Point; 3]> {
        let back = -self.terminal_tangent()?;
        let rotate = |v: Vec2, a: f64| {
            Vec2::new(v.x * a.cos() - v.y * a.sin(), v.x * a.sin() + v.y * a.cos())
        };
        Some([
            self.p1 + rotate(back, half_angle) * length,
            self.p1,
            self.p1 + rotate(back, -half_angle) * length,
        ])
    }

    /// Project `p` onto the whole spine.
    pub fn project(&self, p: Point) -> SpinePoint {
        self.project_range(p, 0., 1.)
    }

    /// Project `p` onto the part of the spine with `t_min ≤ t ≤ t_max`
    /// (swapped if reversed). Both range ends are candidates, not just the
    /// clamped roots.
    pub fn project_range(&self, p: Point, t_min: f64, t_max: f64) -> SpinePoint {
        let (lo, hi) = if t_min <= t_max {
            (t_min, t_max)
        } else {
            (t_max, t_min)
        };
        let (lo, hi) = (lo.clamp(0., 1.), hi.clamp(0., 1.));
        let q = self.quad();
        let a = self.control - self.p0;
        let b = self.p0.to_vec2() - 2. * self.control.to_vec2() + self.p1.to_vec2();
        let d = self.p0 - p;
        // (B(t) − p)·B′(t)/2 = k3 t³ + k2 t² + k1 t + k0.
        let k = [b.dot(b), 3. * a.dot(b), 2. * a.dot(a) + d.dot(b), d.dot(a)];
        let mut best = (lo, f64::INFINITY);
        let mut consider = |t: f64| {
            let t = t.clamp(lo, hi);
            let dist2 = (q.eval(t) - p).hypot2();
            if dist2 < best.1 {
                best = (t, dist2);
            }
        };
        consider(lo);
        consider(hi);
        for root in cubic_roots(k[0], k[1], k[2], k[3]) {
            consider(polish(k, root));
        }
        let t = best.0;
        let point = q.eval(t);
        let distance = best.1.sqrt();
        let normal = geom::normal(PathSeg::Quad(q), t);
        let side = normal.map_or(1., |n| n.dot(p - point));
        let length = self.length();
        let arc_length = quad_arclen(q, 0., t);
        SpinePoint {
            t,
            point,
            distance: if side < 0. { -distance } else { distance },
            arc_length,
            s: if length > 0. { arc_length / length } else { 0. },
        }
    }

    /// Largest curvature on the spine (at the parabola vertex, or the nearer
    /// end); infinite when the spine folds back on itself in a cusp.
    pub fn max_curvature(&self) -> f64 {
        let a = self.control - self.p0;
        let b = self.p0.to_vec2() - 2. * self.control.to_vec2() + self.p1.to_vec2();
        let bb = b.dot(b);
        if bb <= 1e-300 {
            return 0.;
        }
        let t = (-a.dot(b) / bb).clamp(0., 1.);
        // A control point collinear with the ends but outside the chord folds
        // the spine back on itself: the speed vanishes at the vertex (a cusp,
        // unbounded curvature) while the cross product is zero everywhere.
        if (a + b * t).hypot() <= 1e-12 * bb.sqrt() {
            return f64::INFINITY;
        }
        geom::curvature(PathSeg::Quad(self.quad()), t).abs()
    }

    /// The seam distance: 1 / max curvature (infinite when straight, zero at a cusp).
    /// Within this distance of the spine every point has a unique, continuous
    /// projection; beyond it, on the inside of the bend, the nearest point can
    /// jump between arms (the inner-bend seam).
    pub fn reach(&self) -> f64 {
        let k = self.max_curvature();
        if k > 0. { 1. / k } else { f64::INFINITY }
    }

    /// The seam rule: scale the bend down (toward straight) until
    /// [`Self::reach`] is at least `radius`, the distance over which a field
    /// kernel is evaluated. Returns whether the bend changed.
    pub fn limit_bend(&mut self, radius: f64) -> bool {
        if !radius.is_finite() || radius <= 0. || self.reach() >= radius {
            return false;
        }
        let mid = self.p0.midpoint(self.p1);
        let offset = self.control - mid;
        let at = |s: f64| Self {
            control: mid + offset * s,
            ..*self
        };
        let (mut lo, mut hi) = (0., 1.);
        for _ in 0..60 {
            let m = 0.5 * (lo + hi);
            if at(m).reach() >= radius {
                lo = m
            } else {
                hi = m
            }
        }
        *self = at(lo);
        true
    }

    /// Shader parameters `[p0.x, p0.y, c.x, c.y, p1.x, p1.y, t_min, t_max]`.
    pub fn shader_params(&self, t_min: f32, t_max: f32) -> [f32; 8] {
        [
            self.p0.x as f32,
            self.p0.y as f32,
            self.control.x as f32,
            self.control.y as f32,
            self.p1.x as f32,
            self.p1.y as f32,
            t_min,
            t_max,
        ]
    }
}

/// One Newton step on the projection cubic, kept only if it improves |f|.
fn polish(k: [f64; 4], t: f64) -> f64 {
    let f = |t: f64| ((k[0] * t + k[1]) * t + k[2]) * t + k[3];
    let mut t = t;
    for _ in 0..3 {
        let df = (3. * k[0] * t + 2. * k[1]) * t + k[2];
        if df == 0. {
            break;
        }
        let next = t - f(t) / df;
        if next.is_finite() && f(next).abs() <= f(t).abs() {
            t = next;
        } else {
            break;
        }
    }
    t
}

/// Real roots of k3 t³ + k2 t² + k1 t + k0, degrading to quadratic and linear
/// solves when leading coefficients vanish. Never returns NaN.
fn cubic_roots(k3: f64, k2: f64, k1: f64, k0: f64) -> Vec<f64> {
    let scale = k2.abs().max(k1.abs()).max(k0.abs());
    if k3.abs() <= 1e-14 * scale || k3 == 0. {
        return quadratic_roots(k2, k1, k0);
    }
    let (b, c, d) = (k2 / k3, k1 / k3, k0 / k3);
    let p = c - b * b / 3.;
    let q = 2. * b * b * b / 27. - b * c / 3. + d;
    let shift = -b / 3.;
    let disc = 0.25 * q * q + p * p * p / 27.;
    if disc > 0. {
        // One real root (the hyperbolic case), via a cancellation-free Cardano form.
        let a = -q.signum() * (0.5 * q.abs() + disc.sqrt()).cbrt();
        let root = if a != 0. { a - p / (3. * a) } else { 0. };
        return vec![root + shift];
    }
    if p >= 0. {
        // p = q = 0: a triple root.
        return vec![shift];
    }
    let r = (-p / 3.).sqrt();
    let cos = (1.5 * q / p * (-3. / p).sqrt()).clamp(-1., 1.);
    let phi = cos.acos() / 3.;
    (0..3)
        .map(|k| 2. * r * (phi - 2. * std::f64::consts::PI * k as f64 / 3.).cos() + shift)
        .collect()
}

fn quadratic_roots(a: f64, b: f64, c: f64) -> Vec<f64> {
    if a.abs() <= 1e-14 * b.abs().max(c.abs()) || a == 0. {
        return if b != 0. { vec![-c / b] } else { Vec::new() };
    }
    let disc = b * b - 4. * a * c;
    if disc < 0. {
        return vec![-b / (2. * a)];
    }
    let q = -0.5 * (b + b.signum() * disc.sqrt());
    let mut roots = vec![q / a];
    if q != 0. {
        roots.push(c / q);
    }
    roots
}

#[cfg(test)]
#[path = "spine_tests.rs"]
mod tests;
