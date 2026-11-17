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
