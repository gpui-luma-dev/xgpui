use super::*;
use crate::geom::{self, ParamCurve, PathSeg};

fn rounded_rect_sdf(rect: Rect, radius: f64, p: Point) -> f64 {
    let c = rect.center();
    let half = Vec2::new(rect.width() / 2. - radius, rect.height() / 2. - radius);
    let q = Vec2::new((p.x - c.x).abs() - half.x, (p.y - c.y).abs() - half.y);
    Vec2::new(q.x.max(0.), q.y.max(0.)).hypot() + q.x.max(q.y).min(0.) - radius
}

#[test]
fn zero_smoothing_is_a_circular_rounded_rect() {
    let rect = Rect::new(10., 20., 110., 80.);
    let path = Squircle::new(rect, 12., 0.).unwrap().to_path(1e-6);
    for seg in path.segments() {
        for i in 0..=8 {
            let p = seg.eval(i as f64 / 8.);
            assert!(rounded_rect_sdf(rect, 12., p).abs() < 1e-4, "{p:?}");
        }
    }
}

/// (worst tangent-angle jump, worst |Δκ|·R) over all joints.
fn joint_quality(path: &BezPath, radius: f64) -> (f64, f64) {
    let segs: Vec<PathSeg> = path.segments().collect();
    let (mut angle, mut kink): (f64, f64) = (0., 0.);
    for w in segs.windows(2) {
        let (a, b) = (w[0], w[1]);
        let (ta, tb) = (geom::tangent(a, 1.).unwrap(), geom::tangent(b, 0.).unwrap());
        angle = angle.max(ta.cross(tb).abs().asin());
        kink = kink.max((geom::curvature(a, 1.) - geom::curvature(b, 0.)).abs() * radius);
    }
    (angle, kink)
}

#[test]
fn smoothed_corners_are_tangent_and_curvature_continuous() {
    let rect = Rect::new(0., 0., 200., 120.);
    for smoothing in [0.3, 0.6, 1.] {
        let path = Squircle::new(rect, 24., smoothing).unwrap().to_path(1e-7);
        let (angle, kink) = joint_quality(&path, 24.);
        assert!(angle < 1e-6, "smoothing {smoothing}: tangent jump {angle}");
        // Documented G² tolerance: curvature steps below 1e-4 × 1/R (the residual
        // is the cubic approximation of the arc at this tolerance).
        assert!(kink < 1e-4, "smoothing {smoothing}: curvature jump {kink}");
        for seg in path.segments() {
            for i in 0..=8 {
                let p = seg.eval(i as f64 / 8.);
                assert!(p.x >= -1e-9 && p.x <= 200. + 1e-9 && p.y >= -1e-9 && p.y <= 120. + 1e-9);
            }
        }
    }
    // A circular corner (no smoothing) has the familiar curvature jump at the edge.
    let (_, kink) = joint_quality(&Squircle::new(rect, 24., 0.).unwrap().to_path(1e-7), 24.);
    assert!(kink > 0.9);
}

#[test]
fn short_edges_clamp_smoothing_then_radius() {
    let small = Rect::new(0., 0., 40., 30.);
    let path = Squircle::new(small, 50., 0.8).unwrap().to_path(1e-6);
    for seg in path.segments() {
        for i in 0..=8 {
            let p = seg.eval(i as f64 / 8.);
            assert!(
                p.x >= -1e-9 && p.x <= 40. + 1e-9 && p.y >= -1e-9 && p.y <= 30. + 1e-9,
                "{p:?}"
            );
        }
    }
    let per_corner =
        Squircle::with_radii(Rect::new(0., 0., 100., 100.), [0., 10., 20., 30.], 0.5).unwrap();
    let p = per_corner.to_path(1e-6);
    assert!(geom::fill_contains(
        &p,
        Point::new(0.5, 0.5),
        geom::FillRule::NonZero
    ));
    assert!(!geom::fill_contains(
        &p,
        Point::new(99.5, 99.5),
        geom::FillRule::NonZero
    ));
}

#[test]
fn arc_sectors_contain_and_outline() {
    let ring = ArcShape::new(Point::new(50., 50.), 20., 40., 0., FRAC_PI_2);
    // d3 angles: 0 at 12 o'clock, clockwise. A quarter from 12 to 3 o'clock.
    assert!(ring.contains(Point::new(70., 30.)));
    assert!(!ring.contains(Point::new(30., 30.)));
    assert!(!ring.contains(Point::new(52., 48.)));
    let path = ring.to_path(1e-6);
    assert!(geom::fill_contains(
        &path,
        Point::new(70., 30.),
        geom::FillRule::NonZero
    ));
    assert!(!geom::fill_contains(
        &path,
        Point::new(30., 70.),
        geom::FillRule::NonZero
    ));
    let c = ring.centroid();
    assert!(ring.contains(c));
    assert!((c.distance(Point::new(50., 50.)) - 30.).abs() < 1e-12);
    // Padding removes a constant-width gap at the ends.
    let padded = ArcShape {
        pad_angle: 0.2,
        ..ring
    };
    assert!(!padded.contains(Point::new(50. + 1., 50. - 30.)));
    assert!(padded.contains(Point::new(
        50. + 30. * 0.2f64.sin(),
        50. - 30. * 0.2f64.cos()
    )));
    let pie = ArcShape::new(Point::new(0., 0.), 0., 10., 0., PI);
    assert!(pie.contains(Point::new(5., 1.)));
    assert!(
        ArcShape::new(Point::ZERO, 0., 10., 1., 1.)
            .to_path(1e-6)
            .elements()
            .is_empty()
    );
}

#[test]
fn arc_distance_is_exact() {
    let arc = ArcShape::new(Point::new(0., 0.), 0., 10., 0., FRAC_PI_2);
    // On the span: radial distance.
    assert!((arc.distance_to_arc(Point::new(0., -15.), 10.) - 5.).abs() < 1e-12);
    // Off the span: distance to the nearer end point.
    let d = arc.distance_to_arc(Point::new(-10., 0.), 10.);
    assert!((d - Point::new(-10., 0.).distance(Point::new(0., -10.))).abs() < 1e-12);
}
