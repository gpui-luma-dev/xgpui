use super::*;

/// The PCHIP slopes in xgpui-luma `plot/src/render/interpolation.rs`, kept
/// here as the oracle for the plot migration.
fn plot_oracle(x: &[f64], y: &[f64]) -> Vec<f64> {
    let n = x.len();
    let h: Vec<f64> = (0..n - 1).map(|i| x[i + 1] - x[i]).collect();
    let s: Vec<f64> = (0..n - 1).map(|i| (y[i + 1] - y[i]) / h[i]).collect();
    let end = |h0: f64, h1: f64, d0: f64, d1: f64| {
        let d = ((2.0 * h0 + h1) * d0 - h0 * d1) / (h0 + h1);
        if d.signum() != d0.signum() {
            0.0
        } else if d0.signum() != d1.signum() && d.abs() > 3.0 * d0.abs() {
            3.0 * d0
        } else {
            d
        }
    };
    let mut d = vec![0.0; n];
    d[0] = end(h[0], h[1], s[0], s[1]);
    for i in 1..n - 1 {
        let (l, r) = (s[i - 1], s[i]);
        if l != 0.0 && r != 0.0 && l.signum() == r.signum() {
            let w1 = 2.0 * h[i] + h[i - 1];
            let w2 = h[i] + 2.0 * h[i - 1];
            d[i] = (w1 + w2) / (w1 / l + w2 / r);
        }
    }
    let last = s.len() - 1;
    d[n - 1] = end(h[last], h[last - 1], s[last], s[last - 1]);
    d
}

fn pts(xy: &[(f64, f64)]) -> Vec<Point> {
    xy.iter().map(|&(x, y)| Point::new(x, y)).collect()
}

#[test]
fn pchip_matches_the_plot_implementation() {
    let cases: [&[(f64, f64)]; 4] = [
        &[(0., 0.), (1., 1.), (2., 0.5), (4., 3.), (5., 3.)],
        &[(0., 0.), (0.1, 5.), (3., 5.2), (3.5, -1.)],
        &[(0., 1.), (1., 1.), (2., 0.), (3., 0.)],
        &[(-2., 4.), (-1., 1.), (0., 0.), (1., 1.), (2., 4.)],
    ];
    for case in cases {
        let (x, y): (Vec<f64>, Vec<f64>) = case.iter().copied().unzip();
        let ours = pchip_slopes(&x, &y).unwrap();
        for (a, b) in ours.iter().zip(plot_oracle(&x, &y)) {
            assert!((a - b).abs() <= 1e-15 * (1. + b.abs()), "{case:?}");
        }
    }
}

#[test]
fn pchip_is_exact_on_lines_and_never_overshoots() {
    let line = FunctionalCurve::monotone(&pts(&[(0., 1.), (1., 3.), (3., 7.)])).unwrap();
    for i in 0..=30 {
        let x = i as f64 / 10.;
        assert!((line.eval(x) - (1. + 2. * x)).abs() < 1e-12);
    }
    // Step data: values stay within each interval's endpoints.
    let knots = pts(&[
        (0., 0.),
        (1., 0.),
        (1.5, 1.),
        (3., 1.),
        (3.2, 0.2),
        (5., 0.),
    ]);
    let step = FunctionalCurve::monotone(&knots).unwrap();
    for w in knots.windows(2) {
        let (lo, hi) = (w[0].y.min(w[1].y), w[0].y.max(w[1].y));
        for i in 0..=100 {
            let x = w[0].x + (w[1].x - w[0].x) * i as f64 / 100.;
            let y = step.eval(x);
            assert!(y >= lo - 1e-12 && y <= hi + 1e-12, "{x} {y}");
        }
    }
}

#[test]
fn derivatives_match_finite_differences() {
    let knots = pts(&[(0., 0.), (0.3, 0.6), (0.7, 0.65), (1., 1.)]);
    let curves = [
        FunctionalCurve::monotone(&knots).unwrap(),
        FunctionalCurve::linear(&knots).unwrap(),
        FunctionalCurve::bezier(
            &knots,
            &[
                (Point::new(0.1, 0.4), Point::new(0.2, 0.6)),
                (Point::new(0.4, 0.6), Point::new(0.6, 0.7)),
                (Point::new(0.8, 0.7), Point::new(0.9, 1.)),
            ],
        )
        .unwrap(),
    ];
    for curve in &curves {
        for x in [0.05, 0.2, 0.5, 0.65, 0.9] {
            let h = 1e-6;
            let fd = (curve.eval(x + h) - curve.eval(x - h)) / (2. * h);
            assert!((curve.eval_derivative(x) - fd).abs() < 1e-5, "{x}");
        }
    }
}

#[test]
fn clamped_bezier_handles_keep_one_y_per_x() {
    // Handles far outside the interval are clamped, so x(t) stays monotone.
    let curve = FunctionalCurve::bezier(
        &pts(&[(0., 0.), (1., 1.)]),
        &[(Point::new(5., 0.), Point::new(-5., 1.))],
    )
    .unwrap();
    let mut previous = curve.eval(0.);
    for i in 1..=1000 {
        let y = curve.eval(i as f64 / 1000.);
        assert!(y.is_finite() && y >= previous - 1e-12);
        previous = y;
    }
    assert_eq!(curve.eval(-1.), 0.);
    assert_eq!(curve.eval(2.), 1.);
}

#[test]
fn luts_are_finite_and_unit_range() {
    let curve = FunctionalCurve::monotone(&pts(&[(0., -0.2), (0.5, 0.4), (1., 1.4)])).unwrap();
    let lut = curve.to_lut_f32::<256>();
    assert!(lut.iter().all(|v| v.is_finite() && (0. ..=1.).contains(v)));
    assert_eq!(lut[0], 0.);
    assert_eq!(lut[255], 1.);
    let identity = FunctionalCurve::linear(&pts(&[(0., 0.), (1., 1.)])).unwrap();
    let lut = identity.to_lut_f32::<5>();
    assert_eq!(lut, [0., 0.25, 0.5, 0.75, 1.]);
}

#[test]
fn knot_validation() {
    assert_eq!(
        FunctionalCurve::linear(&pts(&[(0., 0.)])).unwrap_err(),
        CurveError::TooFewPoints {
            required: 2,
            actual: 1
        }
    );
    assert_eq!(
        FunctionalCurve::monotone(&pts(&[(0., 0.), (0., 1.)])).unwrap_err(),
        CurveError::NotIncreasing
    );
    assert_eq!(
        FunctionalCurve::monotone(&pts(&[(0., 0.), (f64::NAN, 1.)])).unwrap_err(),
        CurveError::NonFinite
    );
}

#[test]
fn css_cubic_bezier_matches_spec_shapes() {
    assert_eq!(Easing::EASE.eval(0.), 0.);
    assert_eq!(Easing::EASE.eval(1.), 1.);
    // ease at 0.5, from an independent 200-step bisection in f64.
    assert!((Easing::EASE.eval(0.5) - 0.802_403_387_584_857).abs() < 1e-9);
    // ease-in-out is point-symmetric about (0.5, 0.5).
    for x in [0.1, 0.25, 0.4] {
        assert!(
            (Easing::EASE_IN_OUT.eval(x) + Easing::EASE_IN_OUT.eval(1. - x) - 1.).abs() < 1e-12
        );
    }
    // Linear control points give the identity.
    let linear = Easing::cubic_bezier(0.25, 0.25, 0.75, 0.75).unwrap();
    assert!((linear.eval(0.3) - 0.3).abs() < 1e-12);
    // Extrapolation uses the end tangents.
    let overshoot = Easing::cubic_bezier(0.5, -0.5, 0.5, 1.5).unwrap();
    assert!((overshoot.eval(-1.) - 1.).abs() < 1e-12);
    // End slope (y2 − 1)/(x2 − 1) = −1.
    assert!(overshoot.eval(2.).abs() < 1e-12);
    assert!(Easing::cubic_bezier(1.2, 0., 0.5, 1.).is_err());
}

#[test]
fn css_steps_match_spec_examples() {
    let end = Easing::steps(4, StepPosition::JumpEnd).unwrap();
    assert_eq!(
        [0., 0.24, 0.25, 0.99, 1.].map(|x| end.eval(x)),
        [0., 0., 0.25, 0.75, 1.]
    );
    let start = Easing::steps(4, StepPosition::JumpStart).unwrap();
    assert_eq!(
        [0., 0.24, 0.25, 0.99, 1.].map(|x| start.eval(x)),
        [0.25, 0.25, 0.5, 1., 1.]
    );
    let none = Easing::steps(5, StepPosition::JumpNone).unwrap();
    assert_eq!(
        [0., 0.19, 0.2, 0.99, 1.].map(|x| none.eval(x)),
        [0., 0., 0.25, 1., 1.]
    );
    let both = Easing::steps(3, StepPosition::JumpBoth).unwrap();
    assert_eq!(
        [0., 0.33, 0.34, 1.].map(|x| both.eval(x)),
        [0.25, 0.25, 0.5, 1.]
    );
    assert!(Easing::steps(1, StepPosition::JumpNone).is_err());
    assert_eq!(Easing::STEP_START.eval(0.), 1.);
    assert_eq!(Easing::STEP_END.eval(0.99), 0.);
}

#[test]
fn css_linear_resolves_inputs_like_the_spec() {
    let stop = |output, input| LinearStop { output, input };
    // linear(0, 0.25, 1) → inputs 0, 0.5, 1.
    let e = Easing::linear_stops(&[stop(0., None), stop(0.25, None), stop(1., None)]).unwrap();
    assert_eq!(e, Easing::Piecewise(vec![(0., 0.), (0.5, 0.25), (1., 1.)]));
    assert!((e.eval(0.75) - 0.625).abs() < 1e-12);
    // Inputs never decrease: linear(0, 0.5 60%, 1 40%) clamps 40% up to 60%.
    let e =
        Easing::linear_stops(&[stop(0., None), stop(0.5, Some(0.6)), stop(1., Some(0.4))]).unwrap();
    assert_eq!(e, Easing::Piecewise(vec![(0., 0.), (0.6, 0.5), (0.6, 1.)]));
    assert_eq!(e.eval(0.6), 1.);
    assert_eq!(e.eval(0.7), 1.);
    // Extrapolates beyond the ends from the outer segments.
    let e = Easing::linear_stops(&[stop(0., None), stop(1., None)]).unwrap();
    assert_eq!(e.eval(1.5), 1.5);
    assert_eq!(e.eval(-0.5), -0.5);
}
