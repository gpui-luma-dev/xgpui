use super::*;
fn p(x: f32, y: f32) -> Point<Pixels> {
    point(px(x), px(y))
}
fn measured(c: Contour) -> MeasuredPath {
    MeasuredPath::new(&[c], px(0.01)).unwrap()
}
#[test]
fn exact_line_trim_and_dash_phase() {
    let path = measured(Contour::new(p(0., 0.)).line_to(p(100., 0.)));
    let effect = PathEffect::new()
        .trim(0.25..0.75)
        .unwrap()
        .dash(&[px(10.), px(10.)], px(5.))
        .unwrap();
    let out = path.apply(&effect).unwrap();
    assert_eq!(out.len(), 3);
    assert_eq!(out[0].start(), p(25., 0.));
    assert_eq!(out[0].segments()[0].to(), p(30., 0.));
    assert_eq!(out[1].start(), p(40., 0.));
    assert_eq!(out[2].segments()[0].to(), p(70., 0.));
    let negative = PathEffect::new()
        .dash(&[px(10.), px(10.)], px(-5.))
        .unwrap();
    assert_eq!(path.apply(&negative).unwrap()[0].start(), p(5., 0.));
}
#[test]
fn trim_spans_contours_and_preserves_curve_verbs() {
    let contours = [
        Contour::new(p(0., 0.)).line_to(p(20., 0.)),
        Contour::new(p(100., 0.))
            .quadratic_to(p(110., 10.), p(120., 0.))
            .cubic_to(p(130., 0.), p(130., 20.), p(140., 20.)),
    ];
    let path = MeasuredPath::new(&contours, px(0.01)).unwrap();
    let all = path.apply(&PathEffect::new()).unwrap();
    assert_eq!(all, contours);
    let trimmed = path
        .apply(&PathEffect::new().trim(0.1..0.9).unwrap())
        .unwrap();
    assert_eq!(trimmed.len(), 2);
    assert!(matches!(trimmed[1].segments[0], Segment::Quadratic { .. }));
    assert!(matches!(trimmed[1].segments[1], Segment::Cubic { .. }));
    assert_ne!(trimmed[1].segments[1].to(), p(140., 20.));
}
#[test]
fn collinear_nonlinear_speed_uses_arc_length() {
    let path = measured(Contour::new(p(0., 0.)).cubic_to(p(0., 0.), p(0., 0.), p(100., 0.)));
    let out = path
        .apply(&PathEffect::new().trim(0. ..0.5).unwrap())
        .unwrap();
    let x = f32::from(out[0].segments[0].to().x);
    assert!((x - 50.).abs() < 0.03, "nonuniform parameterization: {x}");
}
#[test]
fn closed_seam_and_odd_patterns() {
    let path = measured(
        Contour::new(p(0., 0.))
            .line_to(p(10., 0.))
            .line_to(p(10., 10.))
            .line_to(p(0., 10.))
            .close(),
    );
    assert_eq!(path.length(), px(40.));
    assert!(path.apply(&PathEffect::new()).unwrap()[0].is_closed());
    let dashed = path
        .apply(&PathEffect::new().dash(&[px(15.), px(10.)], px(0.)).unwrap())
        .unwrap();
    assert_eq!(dashed.len(), 1, "visible pieces must join across seam");
    assert_eq!(dashed[0].start(), p(5., 10.));
    assert!(!dashed[0].is_closed());
    let odd = PathEffect::new().dash(&[px(3.)], px(0.)).unwrap();
    let even = PathEffect::new().dash(&[px(3.), px(3.)], px(0.)).unwrap();
    assert_eq!(path.apply(&odd).unwrap(), path.apply(&even).unwrap());
}
#[test]
fn empty_degenerate_and_invalid_inputs() {
    let path = measured(Contour::new(p(0., 0.)).line_to(p(0., 0.)));
    assert!(path.apply(&PathEffect::new()).unwrap().is_empty());
    assert!(
        path.stroke(&PathEffect::new(), px(2.))
            .unwrap()
            .vertices
            .is_empty()
    );
    assert!(PathEffect::new().trim(0.8..0.2).is_err());
    assert!(PathEffect::new().trim(0. ..f32::NAN).is_err());
    assert!(PathEffect::new().dash(&[px(0.)], px(0.)).is_err());
    assert!(PathEffect::new().dash(&[px(-1.)], px(0.)).is_err());
    assert!(PathEffect::new().dash(&[], px(f32::INFINITY)).is_err());
    assert!(MeasuredPath::new(&[Contour::new(p(f32::NAN, 0.))], px(0.1)).is_err());
    assert!(MeasuredPath::new(&[], px(0.)).is_err());
    let line = measured(Contour::new(p(0., 0.)).line_to(p(10., 0.)));
    assert!(
        line.apply(&PathEffect::new().trim(0.5..0.5).unwrap())
            .unwrap()
            .is_empty()
    );
    assert!(line.stroke(&PathEffect::new(), px(-1.)).is_err());
}
#[test]
fn pathological_work_is_bounded() {
    let line = measured(Contour::new(p(0., 0.)).line_to(p(100., 0.)));
    assert!(
        line.apply(&PathEffect::new().dash(&[px(0.000001)], px(0.)).unwrap())
            .is_err()
    );
    let contours = vec![Contour::new(p(0., 0.)); 8193];
    assert!(MeasuredPath::new(&contours, px(0.1)).is_err());
}
#[test]
fn de_casteljau_slices_share_endpoints_and_match_original() {
    let curve = Curve {
        points: vec![[0., 0.], [20., 80.], [80., -20.], [100., 50.]],
    };
    let a = curve.slice(0., 0.3);
    let b = curve.slice(0.3, 1.);
    assert!(distance(*a.points.last().unwrap(), b.points[0]) < 1e-10);
    let sliced = curve.slice(0.2, 0.8);
    for t in [0., 0.25, 0.5, 0.75, 1.] {
        let original = curve.split(0.2 + 0.6 * t).0.points.last().copied().unwrap();
        let actual = sliced.split(t).0.points.last().copied().unwrap();
        assert!(distance(original, actual) < 1e-10);
    }
}

#[test]
fn lookup_budget_rejects_excessive_curvature_work() {
    let c = Contour::new(p(0., 0.)).cubic_to(
        p(1_000_000., 1_000_000.),
        p(-1_000_000., -1_000_000.),
        p(100., 0.),
    );
    let error = MeasuredPath::new(&vec![c; 64], px(0.001)).unwrap_err();
    assert!(error.to_string().contains("65536 samples"), "{error}");
}

#[test]
fn full_trim_retains_closed_tail_after_fractional_contour_lengths() {
    let contours = [
        Contour::new(p(0., 0.)).quadratic_to(p(3.1, 17.9), p(19.3, 0.7)),
        Contour::new(p(1.2, 3.4))
            .line_to(p(11.3, 17.8))
            .line_to(p(19.7, 1.8))
            .close(),
    ];
    let measured = MeasuredPath::new(&contours, px(0.01)).unwrap();
    assert!(measured.apply(&PathEffect::new()).unwrap()[1].is_closed());
}
