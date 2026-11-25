use super::*;
use crate::geom::{self, PathSeg};

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

#[test]
fn arc_padding_order_and_full_turns() {
    let c = Point::new(0., 0.);
    let at = |angle: f64, r: f64| {
        let a = angle - FRAC_PI_2;
        Point::new(r * a.cos(), r * a.sin())
    };
    // Padding applies to sectors of half a turn or more too.
    let mut half = ArcShape::new(c, 20., 40., 0., PI);
    half.pad_angle = 0.2;
    assert!(!half.contains(at(0.05, 30.)));
    assert!(half.contains(at(0.2, 30.)));
    // No padding means none: the start edge is inside.
    assert!(ArcShape::new(c, 20., 40., 0., PI).contains(at(1e-6, 30.)));
    // Reversed angles describe the same sector.
    let forward = ArcShape::new(c, 20., 40., 0., 1.);
    let reversed = ArcShape::new(c, 20., 40., 1., 0.);
    assert!(reversed.contains(at(0.5, 30.)));
    assert!(
        (reversed.to_path(0.01).area().abs() - forward.to_path(0.01).area().abs()).abs() < 1e-6
    );
    // More than a full turn is a full ring, not an overlapping outline.
    let ring = ArcShape::new(c, 20., 40., 0., 7.);
    let annulus = PI * (40. * 40. - 20. * 20.);
    assert!((ring.to_path(0.001).area().abs() - annulus).abs() < 0.01 * annulus);
    assert!(ring.contains(at(3., 30.)));
}

/// `contains` agrees with the filled `to_path` everywhere except within
/// `margin` of the outline.
fn assert_contains_matches_path(arc: &ArcShape, margin: f64) {
    let path = arc.to_path(1e-6);
    let r = arc.inner_radius.max(arc.outer_radius) + 5.;
    let mut checked = 0;
    for i in 0..90 {
        for j in 0..90 {
            let q =
                arc.center + Vec2::new(-r + i as f64 * 2. * r / 89., -r + j as f64 * 2. * r / 89.);
            let near =
                crate::geom::nearest_on_path(&path, q).is_some_and(|h| h.distance.abs() < margin);
            if near {
                continue;
            }
            checked += 1;
            let filled = crate::geom::fill_contains(&path, q, crate::geom::FillRule::NonZero);
            assert_eq!(arc.contains(q), filled, "{arc:?} at {q:?}");
        }
    }
    assert!(checked > 1000);
}

#[test]
fn contains_follows_the_straight_pad_edges() {
    let mut arc = ArcShape::new(Point::new(0., 0.), 10., 100., 0.5, 2.0);
    arc.pad_angle = 0.1;
    assert_contains_matches_path(&arc, 0.05);
    let q = Point::ZERO + Vec2::from_angle(0.7) * 55.;
    assert_eq!(
        arc.contains(q),
        crate::geom::fill_contains(&arc.to_path(1e-6), q, crate::geom::FillRule::NonZero)
    );
    // Large pads, a sweep past half a turn, and a pie slice.
    for (r0, r1, a0, a1, pad) in [
        (40., 100., 0., 4.5, 0.3),
        (5., 100., 1., 2.2, 0.4),
        (0., 100., 0.3, 2.9, 0.2),
    ] {
        let mut arc = ArcShape::new(Point::new(3., -2.), r0, r1, a0, a1);
        arc.pad_angle = pad;
        assert_contains_matches_path(&arc, 0.05);
    }
}

#[test]
fn swapped_radii_draw_and_hit_the_same_ring() {
    let swapped = ArcShape::new(Point::ZERO, 100., 50., 0., 1.);
    let ordered = ArcShape::new(Point::ZERO, 50., 100., 0., 1.);
    let q = Point::ZERO + Vec2::from_angle(0.5 - std::f64::consts::FRAC_PI_2) * 75.;
    assert!(swapped.contains(q));
    assert_eq!(swapped.to_path(1e-6), ordered.to_path(1e-6));
    assert_contains_matches_path(&swapped, 0.05);
}
