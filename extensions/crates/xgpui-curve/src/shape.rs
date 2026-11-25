//! Shape generators: continuous-curvature rounded rectangles (squircles) and
//! annular arc sectors.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use crate::error::{CurveError, finite};
use crate::geom::{Affine, BezPath, Point, Rect, Vec2};

/// A rounded rectangle whose corners blend into the edges with
/// continuous curvature (Figma-style corner smoothing).
///
/// Each corner is a circular arc of the corner radius, entered and left
/// through a cubic that starts with zero curvature on the straight edge and
/// ends with the arc's curvature (G² at both joints). The arc and transition
/// angles follow Figma's published construction; the handle split differs so
/// curvature is continuous where the transition meets the arc. Measured
/// curvature steps at the joints stay below 1e-4 × 1/R at a 1e-7 tolerance.
/// `smoothing` (0..=1) lengthens that transition: 0 is a plain circular
/// corner, 1 removes the arc's straight-edge kink entirely (≈ 0.6 is
/// Apple's icon shape). Corners take at most half the shorter side; when
/// space runs out the smoothing is reduced first, then the radius.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Squircle {
    /// Bounds.
    pub rect: Rect,
    /// Corner radii: top-left, top-right, bottom-right, bottom-left (y down).
    pub radii: [f64; 4],
    /// Corner smoothing in `0..=1`.
    pub smoothing: f64,
}

/// Geometry of one smoothed corner, in a frame where the corner's edge runs
/// along +x and turns toward +y.
#[derive(Clone, Copy, Debug)]
struct CornerParams {
    radius: f64,
    /// Total edge length the corner consumes on each side.
    p: f64,
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    arc_section: f64,
}

fn corner_params(radius: f64, smoothing: f64, budget: f64) -> CornerParams {
    let mut smoothing = smoothing.clamp(0., 1.);
    let mut radius = radius.max(0.).min(budget);
    if radius > 0. {
        // Reduce smoothing first so the corner fits the budget.
        smoothing = smoothing.min((budget / radius - 1.).max(0.));
    }
    radius = radius.min(budget);
    let p = ((1. + smoothing) * radius).min(budget);
    let arc_measure = FRAC_PI_2 * (1. - smoothing);
    let arc_section = (arc_measure / 2.).sin() * radius * 2f64.sqrt();
    let angle_alpha = (FRAC_PI_2 - arc_measure) / 2.;
    let p3_to_p4 = radius * (angle_alpha / 2.).tan();
    let angle_beta = PI / 4. * smoothing;
    let c = p3_to_p4 * angle_beta.cos();
    let d = c * angle_beta.tan();
    // The transition cubic is (0,0), (a,0), (a+b,0), (a+b+c, d): zero
    // curvature where it leaves the edge. Its end curvature is
    // (2/3)·b·d / (c² + d²)^{3/2}; choose b to make that 1/R (exact G² with the
    // arc) and give the rest of the budget to a. Figma instead fixes a = 2b,
    // which leaves a curvature step at the arc.
    let free = (p - arc_section - c - d).max(0.);
    let mut b = if d > 1e-12 * radius.max(1e-300) {
        1.5 * (c * c + d * d).powf(1.5) / (radius * d)
    } else {
        free / 3.
    };
    if b > free {
        b = free;
    }
    let a = free - b;
    CornerParams {
        radius,
        p,
        a,
        b,
        c,
        d,
        arc_section,
    }
}

impl Squircle {
    /// One radius for every corner.
    pub fn new(rect: Rect, radius: f64, smoothing: f64) -> Result<Self, CurveError> {
        Self::with_radii(rect, [radius; 4], smoothing)
    }

    /// Per-corner radii (top-left, top-right, bottom-right, bottom-left).
    pub fn with_radii(rect: Rect, radii: [f64; 4], smoothing: f64) -> Result<Self, CurveError> {
        finite(&[rect.x0, rect.y0, rect.x1, rect.y1, smoothing])?;
        finite(&radii)?;
        Ok(Self {
            rect: rect.abs(),
            radii,
            smoothing,
        })
    }

    /// The outline as a closed path, clockwise on screen (y down), starting
    /// at the top edge. Arcs are approximated by cubics within `tolerance`.
    pub fn to_path(&self, tolerance: f64) -> BezPath {
        let r = self.rect;
        let budget = 0.5 * r.width().min(r.height());
        // Corner frames: origin at the corner, x along the incoming edge, y
        // toward the outgoing edge (both pointing into the rectangle side).
        let frames = [
            // top-right: incoming edge +x, turns to +y.
            (
                Point::new(r.x1, r.y0),
                Vec2::new(1., 0.),
                Vec2::new(0., 1.),
                self.radii[1],
            ),
            (
                Point::new(r.x1, r.y1),
                Vec2::new(0., 1.),
                Vec2::new(-1., 0.),
                self.radii[2],
            ),
            (
                Point::new(r.x0, r.y1),
                Vec2::new(-1., 0.),
                Vec2::new(0., -1.),
                self.radii[3],
            ),
            (
                Point::new(r.x0, r.y0),
                Vec2::new(0., -1.),
                Vec2::new(1., 0.),
                self.radii[0],
            ),
        ];
        let tolerance = tolerance.max(1e-9);
        let mut path = BezPath::new();
        for (i, &(corner, ex, ey, radius)) in frames.iter().enumerate() {
            let k = corner_params(radius, self.smoothing, budget);
            // Local frame: the corner at the origin, the incoming edge along
            // −x → 0 on y = 0, the outgoing edge along x = 0 toward +y.
            let at = |x: f64, y: f64| corner + ex * x + ey * y;
            let start = at(-k.p, 0.);
            if i == 0 {
                path.move_to(start);
            } else {
                path.line_to(start);
            }
            if k.radius <= 0. {
                path.line_to(corner);
                continue;
            }
            // Incoming transition: (a, 0), (a + b, 0), (a + b + c, d) from the start.
            let x1 = -k.p + k.a + k.b + k.c;
            if k.a + k.b + k.c + k.d > 1e-12 {
                path.curve_to(at(-k.p + k.a, 0.), at(-k.p + k.a + k.b, 0.), at(x1, k.d));
            }
            // The circular arc, centered `radius` inside the corner, ending
            // `arc_section` further along both axes.
            let center = at(-k.radius, k.radius);
            let (x2, y2) = (x1 + k.arc_section, k.d + k.arc_section);
            let a0 = (at(x1, k.d) - center).atan2();
            let a1 = (at(x2, y2) - center).atan2();
            let sweep = (a1 - a0 + PI).rem_euclid(TAU) - PI;
            if sweep.abs() > 1e-12 {
                crate::geometry::arc_to_cubics(
                    center,
                    k.radius,
                    a0,
                    sweep,
                    tolerance,
                    |c1, c2, p| path.curve_to(c1, c2, p),
                );
            }
            // Outgoing transition, the mirror image: (d, c), (d, b + c), then the edge at p.
            if k.a + k.b + k.c + k.d > 1e-12 {
                path.curve_to(
                    at(x2 + k.d, y2 + k.c),
                    at(x2 + k.d, y2 + k.b + k.c),
                    at(0., k.p),
                );
            }
        }
        path.close_path();
        path
    }
}

/// An annular sector (d3's arc): angles in radians measured clockwise from
/// 12 o'clock on screen (y down), as in the xgpui-luma sdk arc helper.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArcShape {
    /// Center.
    pub center: Point,
    /// Inner radius (0 for a pie slice).
    pub inner_radius: f64,
    /// Outer radius.
    pub outer_radius: f64,
    /// Start angle.
    pub start_angle: f64,
    /// End angle.
    pub end_angle: f64,
    /// Padding angle split between both ends, kept at a constant linear width
    /// on the inner edge as d3 does.
    pub pad_angle: f64,
}

impl ArcShape {
    /// A sector without padding.
    pub fn new(
        center: Point,
        inner_radius: f64,
        outer_radius: f64,
        start_angle: f64,
        end_angle: f64,
    ) -> Self {
        Self {
            center,
            inner_radius,
            outer_radius,
            start_angle,
            end_angle,
            pad_angle: 0.,
        }
    }

    /// Screen angle (radians from +x, y down) for a d3 angle.
    fn screen(angle: f64) -> f64 {
        angle - FRAC_PI_2
    }

    /// The swept screen-angle range, ordered, with sweeps of a full turn or
    /// more reduced to one turn (as d3 does). The sector is the same whichever
    /// order the angles are given in.
    fn span(&self) -> (f64, f64) {
        let (s, e) = (Self::screen(self.start_angle), Self::screen(self.end_angle));
        let (lo, hi) = if s <= e { (s, e) } else { (e, s) };
        (lo, lo + (hi - lo).min(TAU))
    }

    /// (inner, outer) radii, swapped if given the other way round (as d3
    /// does), with a negative inner radius treated as 0.
    fn radii(&self) -> (f64, f64) {
        let (a, b) = (self.inner_radius.max(0.), self.outer_radius.max(0.));
        if a <= b { (a, b) } else { (b, a) }
    }

    /// The padded angle ranges (outer, inner) in screen angles, or `None` when empty.
    fn ranges(&self) -> Option<((f64, f64), (f64, f64))> {
        let (s, e) = self.span();
        let da = e - s;
        let (r0, r1) = self.radii();
        if r1 < 1e-12 || da < 1e-12 {
            return None;
        }
        // A full ring has no ends to pad.
        let pad = if da >= TAU - 1e-12 {
            0.
        } else {
            self.pad_angle.max(0.)
        };
        let (outer, inner) = if r0 > 1e-12 && pad > 0. {
            let width = r1 * pad;
            let inner_pad = (width / r0).min(da * 0.8);
            (
                (s + pad * 0.5, e - pad * 0.5),
                (s + inner_pad * 0.5, e - inner_pad * 0.5),
            )
        } else {
            (
                (s + pad * 0.5, e - pad * 0.5),
                (s + pad * 0.5, e - pad * 0.5),
            )
        };
        (outer.1 > outer.0).then_some((outer, inner))
    }

    /// The closed outline (outer arc clockwise, inner arc back).
    pub fn to_path(&self, tolerance: f64) -> BezPath {
        let mut path = BezPath::new();
        let Some(((o0, o1), (i0, i1))) = self.ranges() else {
            return path;
        };
        let (r0, r1) = self.radii();
        let c = self.center;
        let tolerance = tolerance.max(1e-9);
        path.move_to(c + Vec2::from_angle(o0) * r1);
        crate::geometry::arc_to_cubics(c, r1, o0, o1 - o0, tolerance, |a, b, p| {
            path.curve_to(a, b, p)
        });
        if r0 > 1e-12 {
            path.line_to(c + Vec2::from_angle(i1) * r0);
            crate::geometry::arc_to_cubics(c, r0, i1, i0 - i1, tolerance, |a, b, p| {
                path.curve_to(a, b, p)
            });
        } else {
            path.line_to(c);
        }
        path.close_path();
        path
    }

    /// The point midway in angle and radius.
    pub fn centroid(&self) -> Point {
        let a = 0.5 * (Self::screen(self.start_angle) + Self::screen(self.end_angle));
        let (r0, r1) = self.radii();
        let r = 0.5 * (r0 + r1);
        self.center + Vec2::from_angle(a) * r
    }

    /// Whether `p` is inside the padded sector (boundary inclusive), analytically.
    pub fn contains(&self, p: Point) -> bool {
        let Some(((o0, o1), (i0, i1))) = self.ranges() else {
            return false;
        };
        let (r0, r1) = self.radii();
        let v = p - self.center;
        let r = v.hypot();
        if r > r1 + 1e-12 || r < r0 - 1e-12 {
            return false;
        }
        // The ends are the straight edges `to_path` draws, from the inner to
        // the outer arc: test against where they cross the circle of radius r.
        let lo = edge_angle(i0, r0, o0, r1, r);
        let hi = edge_angle(i1, r0, o1, r1, r);
        let angle = v.atan2();
        let rel = (angle - lo).rem_euclid(TAU);
        rel <= hi - lo + 1e-12
    }

    /// Exact distance from `p` to the circular arc of `radius` spanning the
    /// sector's (unpadded) angles: for hit-testing arc strokes such as range
    /// slider tracks.
    pub fn distance_to_arc(&self, p: Point, radius: f64) -> f64 {
        let (lo, hi) = self.span();
        let v = p - self.center;
        let rel = (v.atan2() - lo).rem_euclid(TAU);
        if rel <= hi - lo && v.hypot() > 0. {
            return (v.hypot() - radius).abs();
        }
        let end = |a: f64| (self.center + Vec2::from_angle(a) * radius).distance(p);
        if v.hypot() == 0. {
            radius
        } else {
            end(lo).min(end(hi))
        }
    }

    /// The same sector moved by `affine` (useful when angles are in another frame).
    pub fn path_transformed(&self, affine: Affine, tolerance: f64) -> BezPath {
        affine * self.to_path(tolerance)
    }
}

/// The screen angle at which the straight end edge from (`inner_angle`, `r0`)
/// to (`outer_angle`, `r1`) crosses the circle of radius `r`, unwrapped to lie
/// near `inner_angle`.
fn edge_angle(inner_angle: f64, r0: f64, outer_angle: f64, r1: f64, r: f64) -> f64 {
    let p0 = Vec2::from_angle(inner_angle) * r0;
    let d = Vec2::from_angle(outer_angle) * r1 - p0;
    let a = d.dot(d);
    let s = if r0 <= 1e-12 || a <= 1e-300 {
        // A pie slice's edge is radial: one angle at every radius.
        return if r0 <= 1e-12 {
            outer_angle
        } else {
            inner_angle
        };
    } else {
        // |p0 + s·d| = r, taking the root along the edge (radius grows with s).
        let b = 2. * p0.dot(d);
        let c = p0.dot(p0) - r * r;
        let disc = (b * b - 4. * a * c).max(0.);
        ((-b + disc.sqrt()) / (2. * a)).clamp(0., 1.)
    };
    let angle = (p0 + d * s).atan2();
    inner_angle + (angle - inner_angle + std::f64::consts::PI).rem_euclid(TAU)
        - std::f64::consts::PI
}

#[cfg(test)]
#[path = "shape_tests.rs"]
mod tests;
