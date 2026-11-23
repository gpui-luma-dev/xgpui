use super::*;
use crate::geom::{CubicBez, Line};

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1e-300)
}

#[test]
fn quadratic_closed_form_matches_analytic_parabola() {
    // y = x² on −1..1: √5 + asinh(2)/2.
    let q = QuadBez::new((-1., 1.), (0., -1.), (1., 1.));
    let expected = 5f64.sqrt() + 2f64.asinh() / 2.;
    assert!(rel(quad_arclen(q, 0., 1.), expected) <= 1e-12);
    assert!(rel(quad_arclen(q, 1., 0.), expected) <= 1e-12);
    // Half the parabola is half the length by symmetry.
    assert!(rel(quad_arclen(q, 0., 0.5), expected / 2.) <= 1e-12);
}

#[test]
fn quadratic_closed_form_matches_quadrature() {
    for q in [
        QuadBez::new((0., 0.), (2., 3.), (4., -1.)),
        QuadBez::new((0., 0.), (10., 0.1), (0., 0.2)),
        QuadBez::new((1., 1.), (1.5, 9.), (2., 1.)),
    ] {
        for (t0, t1) in [(0f64, 1f64), (0.2, 0.7), (0.9, 0.1)] {
            let reference = PathSeg::Cubic(q.raise())
                .subsegment(t0.min(t1)..t0.max(t1))
                .arclen(1e-14);
            assert!(
                rel(quad_arclen(q, t0, t1), reference) <= 1e-9,
                "{q:?} {t0} {t1}"
            );
        }
    }
}

#[test]
fn degenerate_quadratics_stay_finite_and_exact() {
    // Straight, uniform.
    let straight = QuadBez::new((0., 0.), (1., 1.), (2., 2.));
    assert!(rel(quad_arclen(straight, 0., 1.), 8f64.sqrt()) <= 1e-12);
    // Collinear and back-tracking: x(t) = 4t − 3t² peaks at 4/3, ends at 1.
    let back = QuadBez::new((0., 0.), (2., 0.), (1., 0.));
    assert!(rel(quad_arclen(back, 0., 1.), 5. / 3.) <= 1e-12);
    // Collinear but non-uniform, no reversal.
    let skew = QuadBez::new((0., 0.), (0.2, 0.), (1., 0.));
    assert!(rel(quad_arclen(skew, 0., 1.), 1.) <= 1e-12);
    let point = QuadBez::new((3., 3.), (3., 3.), (3., 3.));
    assert_eq!(quad_arclen(point, 0., 1.), 0.);
}

fn square() -> BezPath {
    let mut p = BezPath::new();
    p.move_to((0., 0.));
    p.line_to((1., 0.));
    p.line_to((1., 1.));
    p.line_to((0., 1.));
    p.close_path();
    p
}

#[test]
fn measured_path_locates_and_trims() {
    let m = MeasuredPath::new(&square(), 1e-9).unwrap();
    assert!(rel(m.length(), 4.) <= 1e-12);
    assert_eq!(m.is_closed(0), Some(true));
    let at = m.locate(2.5).unwrap();
    assert_eq!((at.segment, at.contour), (2, 0));
    assert!((at.point - Point::new(0.5, 1.)).hypot() < 1e-12);
    let trimmed = m
        .apply(&PathEffect::new().trim(0.25..0.75).unwrap())
        .unwrap();
    let segs: Vec<_> = trimmed.segments().collect();
    assert!((segs[0].start() - Point::new(1., 0.)).hypot() < 1e-12);
    assert!((segs.last().unwrap().end() - Point::new(0., 1.)).hypot() < 1e-12);
    // An untrimmed solid closed contour stays closed.
    let whole = m.apply(&PathEffect::new()).unwrap();
    assert!(matches!(whole.elements().last(), Some(PathEl::ClosePath)));
}

#[test]
fn dashes_restart_and_join_across_the_closed_seam() {
    let m = MeasuredPath::new(&square(), 1e-9).unwrap();
    let dashed = m
        .apply(&PathEffect::new().dash(&[1., 1.], 0.).unwrap())
        .unwrap();
    assert_eq!(
        dashed
            .elements()
            .iter()
            .filter(|e| matches!(e, PathEl::MoveTo(_)))
            .count(),
        2
    );
    let shifted = m
        .apply(&PathEffect::new().dash(&[1., 1.], 0.5).unwrap())
        .unwrap();
    // [3.5, 4] and [0, 0.5] join into one piece through the corner at the start.
    assert_eq!(
        shifted
            .elements()
            .iter()
            .filter(|e| matches!(e, PathEl::MoveTo(_)))
            .count(),
        2
    );
    assert!(PathEffect::new().dash(&[1., 0.], 0.).is_err());
    assert!(PathEffect::new().trim(0.5..0.2).is_err());
    assert!(
        m.apply(&PathEffect::new().dash(&[1e-9], 0.).unwrap())
            .is_err()
    );
}

#[test]
fn inverse_lookup_is_accurate_on_cubics() {
    let mut path = BezPath::new();
    path.move_to((0., 0.));
    path.curve_to((1., 3.), (3., -2.), (4., 1.));
    path.quad_to((5., 3.), (7., 0.));
    let m = MeasuredPath::new(&path, 1e-10).unwrap();
    let segs: Vec<_> = path.segments().collect();
    let first = arclen(segs[0], 0., 1., 1e-13);
    for i in 0..=40 {
        let s = m.length() * i as f64 / 40.;
        let at = m.locate(s).unwrap();
        let measured = if at.segment == 0 {
            arclen(segs[0], 0., at.t, 1e-13)
        } else {
            first + arclen(segs[1], 0., at.t, 1e-13)
        };
        assert!((measured - s).abs() <= 1e-9 * (1. + s), "{s} {measured}");
    }
    let line = PathSeg::Line(Line::new((0., 0.), (3., 4.)));
    assert_eq!(arclen(line, 0., 1., 1e-9), 5.);
    let straight = PathSeg::Cubic(CubicBez::new((0., 0.), (1., 0.), (2., 0.), (3., 0.)));
    assert!(rel(arclen(straight, 0., 1., 1e-12), 3.) <= 1e-9);
}

#[test]
fn invalid_input_is_rejected() {
    let mut path = BezPath::new();
    path.move_to((0., 0.));
    path.line_to((f64::NAN, 0.));
    assert_eq!(
        MeasuredPath::new(&path, 1e-9).unwrap_err(),
        CurveError::NonFinite
    );
    assert!(MeasuredPath::new(&square(), 0.).is_err());
    let empty = MeasuredPath::new(&BezPath::new(), 1e-9).unwrap();
    assert_eq!(empty.length(), 0.);
    assert!(empty.locate(1.).is_none());
}

#[test]
fn locate_skips_a_leading_zero_length_contour() {
    let mut path = BezPath::new();
    path.move_to((5., 5.));
    path.line_to((5., 5.));
    path.move_to((0., 0.));
    path.line_to((10., 0.));
    let measured = MeasuredPath::new(&path, 1e-3).unwrap();
    let start = measured.locate(0.).unwrap();
    assert_eq!(start.contour, 1);
    assert_eq!(start.point, Point::new(0., 0.));
    assert!(start.tangent.is_some());
}

#[test]
fn locate_at_a_leading_zero_length_segment_has_the_next_tangent() {
    let path = crate::svg_path::parse("M0,0 L0,0 L10,0").unwrap();
    let m = MeasuredPath::new(&path, 1e-6).unwrap();
    let at = m.locate(0.).unwrap();
    assert_eq!(at.tangent, Some(Vec2::new(1., 0.)));
    assert_eq!(at.point, Point::ZERO);
    // Between segments too: M0,0 L5,0 L5,0 L10,0 at s = 5.
    let path = crate::svg_path::parse("M0,0 L5,0 L5,0 L5,10").unwrap();
    let m = MeasuredPath::new(&path, 1e-6).unwrap();
    let at = m.locate(5.).unwrap();
    assert_eq!(at.point, Point::new(5., 0.));
    assert!(at.tangent.is_some());
}

#[test]
fn geometry_query_matches_analytic_parabola_and_scales() {
    let mut path = BezPath::new();
    path.move_to((0., 0.));
    path.quad_to((0.5, 0.), (1., 1.));
    let q = QuadBez::new((0., 0.), (0.5, 0.), (1., 1.));
    let s = quad_arclen(q, 0., 0.5);
    let measured = MeasuredPath::new(&path, 1e-10).unwrap();
    let g = measured.geometry_at(s).unwrap();
    assert!(g.location.point.distance(Point::new(0.5, 0.25)) < 1e-9);
    let tangent = Vec2::new(1., 1.).normalize();
    assert!((g.location.tangent.unwrap() - tangent).hypot() < 1e-9);
    assert!((g.normal.unwrap() - Vec2::new(-tangent.y, tangent.x)).hypot() < 1e-9);
    let expected = 1. / 2f64.sqrt();
    assert!((g.curvature.unwrap() - expected).abs() < 1e-9);
    for (transform, expected_k) in [
        (crate::geom::Affine::scale(3.), expected / 3.),
        (crate::geom::Affine::scale_non_uniform(1., -1.), -expected),
    ] {
        let transformed = transform * &path;
        let m = MeasuredPath::new(&transformed, 1e-10).unwrap();
        let scaled_s = if expected_k > 0. { s * 3. } else { s };
        assert!((m.geometry_at(scaled_s).unwrap().curvature.unwrap() - expected_k).abs() < 1e-9);
    }
}

#[test]
fn geometry_distinguishes_straight_and_stationary_points() {
    let mut straight = BezPath::new();
    straight.move_to((0., 0.));
    straight.line_to((3., 0.));
    let m = MeasuredPath::new(&straight, 1e-9).unwrap();
    let g = m.geometry_at(1.).unwrap();
    assert_eq!(g.curvature, Some(0.));
    assert!((g.normal.unwrap() - Vec2::new(0., 1.)).hypot() < 1e-12);
    let mut stationary = BezPath::new();
    stationary.move_to((0., 0.));
    stationary.curve_to((0., 0.), (1., 0.), (2., 0.));
    let m = MeasuredPath::new(&stationary, 1e-9).unwrap();
    let start = m.geometry_at(0.).unwrap();
    assert!((start.location.tangent.unwrap() - Vec2::new(1., 0.)).hypot() < 1e-12);
    assert!((start.normal.unwrap() - Vec2::new(0., 1.)).hypot() < 1e-12);
    assert_eq!(start.curvature, None);
    assert_eq!(m.geometry_at(0.5).unwrap().curvature, Some(0.));
    let mut collapsed = BezPath::new();
    collapsed.move_to((1., 2.));
    collapsed.line_to((1., 2.));
    let g = MeasuredPath::new(&collapsed, 1e-9)
        .unwrap()
        .geometry_at(0.)
        .unwrap();
    assert_eq!(g.location.point, Point::new(1., 2.));
    assert_eq!(g.location.tangent, None);
    assert_eq!(g.normal, None);
    assert_eq!(g.curvature, None);
}

#[test]
fn geometry_query_preserves_boundary_and_contour_selection() {
    let mut path = BezPath::new();
    path.move_to((0., 0.));
    path.line_to((0., 0.));
    path.line_to((2., 0.));
    path.move_to((10., 10.));
    path.line_to((10., 12.));
    let m = MeasuredPath::new(&path, 1e-9).unwrap();
    assert_eq!(m.geometry_at(0.).unwrap().location.segment, 1);
    assert_eq!(m.geometry_at(2.).unwrap().location.contour, 0);
    let next = m.geometry_at(3.).unwrap();
    assert_eq!(next.location.contour, 1);
    assert_eq!(next.location.point, Point::new(10., 11.));
    assert_eq!(next.normal, Some(Vec2::new(-1., 0.)));
    assert_eq!(m.geometry_at(-100.).unwrap().location.point, Point::ZERO);
    assert_eq!(
        m.geometry_at(100.).unwrap().location.point,
        Point::new(10., 12.)
    );
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(m.geometry_at(bad).is_none());
    }
    assert!(
        MeasuredPath::new(&BezPath::new(), 1e-9)
            .unwrap()
            .geometry_at(0.)
            .is_none()
    );
    let mut closed = BezPath::new();
    closed.move_to((0., 0.));
    closed.line_to((1., 0.));
    closed.line_to((1., 1.));
    closed.close_path();
    let m = MeasuredPath::new(&closed, 1e-9).unwrap();
    let end = m.geometry_at(m.length()).unwrap();
    assert!(end.location.point.distance(Point::ZERO) < 1e-12);
    assert_eq!(end.location.segment, 2);
    assert_ne!(
        end.location.tangent,
        m.geometry_at(0.).unwrap().location.tangent
    );
}
