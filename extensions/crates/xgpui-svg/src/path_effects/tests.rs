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
fn trimmed_pieces_share_endpoints_and_lie_on_the_original_curve() {
    let original = Contour::new(p(0., 0.)).cubic_to(p(20., 80.), p(80., -20.), p(100., 50.));
    let path = measured(original);
    let first = path
        .apply(&PathEffect::new().trim(0. ..0.3).unwrap())
        .unwrap();
    let rest = path
        .apply(&PathEffect::new().trim(0.3..1.).unwrap())
        .unwrap();
    assert_eq!(first[0].segments()[0].to(), rest[0].start());
    // Points of the middle piece lie on the original cubic.
    let middle = path
        .apply(&PathEffect::new().trim(0.2..0.8).unwrap())
        .unwrap();
    let Segment::Cubic {
        control_a,
        control_b,
        to,
    } = middle[0].segments()[0]
    else {
        panic!("cubic expected");
    };
    let f = |q: Point<Pixels>| [f32::from(q.x) as f64, f32::from(q.y) as f64];
    let piece = [f(middle[0].start()), f(control_a), f(control_b), f(to)];
    let eval = |c: [[f64; 2]; 4], t: f64| {
        let u = 1. - t;
        let w = [u * u * u, 3. * u * u * t, 3. * u * t * t, t * t * t];
        [0, 1].map(|k| (0..4).map(|i| w[i] * c[i][k]).sum::<f64>())
    };
    let source = [[0., 0.], [20., 80.], [80., -20.], [100., 50.]];
    for t in [0., 0.25, 0.5, 0.75, 1.] {
        let q = eval(piece, t);
        let nearest = (0..=20_000)
            .map(|i| eval(source, i as f64 / 20_000.))
            .map(|s| (s[0] - q[0]).hypot(s[1] - q[1]))
            .fold(f64::MAX, f64::min);
        assert!(nearest < 0.02, "{t}: {nearest}");
    }
}

#[test]
fn extreme_curvature_is_measured_within_bounded_work() {
    // The previous adaptive table rejected this at 65536 samples; quadrature
    // measures it directly, still bounded per segment.
    let c = Contour::new(p(0., 0.)).cubic_to(
        p(1_000_000., 1_000_000.),
        p(-1_000_000., -1_000_000.),
        p(100., 0.),
    );
    let path = MeasuredPath::new(&vec![c; 64], px(0.001)).unwrap();
    let length = f32::from(path.length());
    assert!(length.is_finite() && length > 1_000_000.);
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
