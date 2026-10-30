use super::*;
fn near(a: [f64; 3], b: [f64; 3], eps: f64) {
    for (x, y) in a.into_iter().zip(b) {
        assert!((x - y).abs() < eps, "{a:?} != {b:?}");
    }
}
fn rgba(v: [f64; 3], alpha: f64) -> LinearRgba {
    LinearRgba::new(LinearRgb::new(RgbSpace::Srgb, v).unwrap(), alpha).unwrap()
}
fn gradient(a: LinearRgba, b: LinearRgba, space: ColorInterpolation) -> LinearGradient {
    LinearGradient::new([0., 0.], [1., 0.], [a, b], RgbSpace::Srgb, space).unwrap()
}
#[test]
fn alpha_weighted_rgb_and_association() {
    let g = gradient(
        rgba([1., 0., 0.], 0.25),
        rgba([0., 0., 1.], 0.75),
        ColorInterpolation::LinearRgb,
    );
    let mid = g.sample_at(0.5).unwrap();
    near(mid.rgb().components(), [0.25, 0., 0.75], 1e-14);
    assert_eq!(mid.alpha(), 0.5);
    assert_eq!(mid.premultiplied(), [0.125, 0., 0.375, 0.5]);
    for interpolation in [
        ColorInterpolation::LinearRgb,
        ColorInterpolation::Oklab,
        ColorInterpolation::Oklch(HueInterpolation::Shorter),
    ] {
        let g = gradient(
            rgba([1., 0., 0.], 1.),
            rgba([0., 0., 1.], 0.),
            interpolation,
        );
        near(
            g.sample_at(0.5).unwrap().rgb().components(),
            [1., 0., 0.],
            5e-7,
        );
        assert_eq!(g.sample_at(1.).unwrap().premultiplied(), [0.; 4]);
        let empty = gradient(
            rgba([1., 0., 0.], 0.),
            rgba([0., 1., 0.], 0.),
            interpolation,
        );
        assert_eq!(empty.sample_at(0.5).unwrap().rgb().components(), [0.; 3]);
    }
}
#[test]
fn oklab_midpoint_reference() {
    let g = gradient(
        rgba([1., 0., 0.], 1.),
        rgba([0., 0., 1.], 1.),
        ColorInterpolation::Oklab,
    );
    // Published OkLab interpolation of linear sRGB red and blue.
    near(
        g.sample_at(0.5).unwrap().rgb().components(),
        [0.2637342898, 0.0865716757, 0.3628242654],
        1e-9,
    );
}
#[test]
fn hue_arcs_and_ties() {
    use HueInterpolation::*;
    for (policy, expected) in [
        (Shorter, 20.),
        (Longer, -340.),
        (Increasing, 20.),
        (Decreasing, -340.),
    ] {
        assert_eq!(hue_delta(350., 10., policy), expected);
    }
    assert_eq!(hue_delta(10., 350., Shorter), -20.);
    assert_eq!(hue_delta(10., 350., Longer), 340.);
    assert_eq!(hue_delta(0., 180., Shorter), 180.);
    assert_eq!(hue_delta(180., 0., Longer), -180.);
    assert_eq!(hue_delta(30., 30., Longer), 360.);
    let make = |h| {
        LinearRgba::new(
            Oklch::new(0.6, 0.1, h)
                .unwrap()
                .to_oklab()
                .unwrap()
                .to_linear(RgbSpace::Srgb)
                .unwrap(),
            1.,
        )
        .unwrap()
    };
    for (policy, hue) in [
        (Shorter, 0.),
        (Longer, 180.),
        (Increasing, 0.),
        (Decreasing, 180.),
    ] {
        let mid = gradient(make(350.), make(10.), ColorInterpolation::Oklch(policy))
            .sample_at(0.5)
            .unwrap();
        near(mid.rgb().components(), make(hue).rgb().components(), 2e-7);
    }
}
#[test]
fn neutral_hue_is_carried_from_colored_stop() {
    let gray = rgba([0.125; 3], 1.);
    let color = LinearRgba::new(
        Oklch::new(0.5, 0.1, 120.)
            .unwrap()
            .to_oklab()
            .unwrap()
            .to_linear(RgbSpace::Srgb)
            .unwrap(),
        1.,
    )
    .unwrap();
    let mid = gradient(
        gray,
        color,
        ColorInterpolation::Oklch(HueInterpolation::Shorter),
    )
    .sample_at(0.5)
    .unwrap()
    .rgb()
    .to_oklab()
    .unwrap()
    .to_oklch()
    .unwrap()
    .components();
    near(mid, [0.5, 0.05, 120.], 2e-5);
}
#[test]
fn geometry_extends_endpoints_and_handles_degenerate_axis() {
    let a = rgba([1., 0., 0.], 1.);
    let b = rgba([0., 0., 1.], 1.);
    let g = LinearGradient::new(
        [2., 3.],
        [4., 5.],
        [a, b],
        RgbSpace::Srgb,
        ColorInterpolation::LinearRgb,
    )
    .unwrap();
    assert_eq!(g.sample([0., 0.]).unwrap(), a);
    assert_eq!(g.sample([10., 10.]).unwrap(), b);
    near(
        g.sample([3., 4.]).unwrap().rgb().components(),
        [0.5, 0., 0.5],
        1e-14,
    );
    let degenerate = LinearGradient::new(
        [2.; 2],
        [2.; 2],
        [a, b],
        RgbSpace::Srgb,
        ColorInterpolation::Oklab,
    )
    .unwrap();
    assert_eq!(degenerate.sample([0.; 2]).unwrap(), b);
    assert!(g.sample_at(f64::NAN).is_err());
    assert!(g.sample([f64::INFINITY, 0.]).is_err());
    assert!(LinearRgba::new(a.rgb(), 1.01).is_err());
    assert!(
        LinearGradient::new(
            [f64::NAN, 0.],
            [1.; 2],
            [a, b],
            RgbSpace::Srgb,
            ColorInterpolation::LinearRgb
        )
        .is_err()
    );
}
#[test]
fn mapping_bounds_preservation_and_perceptual_policy() {
    for space in [
        RgbSpace::Srgb,
        RgbSpace::DisplayP3,
        RgbSpace::Rec2020,
        RgbSpace::AdobeRgb1998,
    ] {
        let inside = LinearRgb::new(space, [0.1, 0.5, 0.9]).unwrap();
        for policy in [GamutMapping::Clip, GamutMapping::ReduceChroma] {
            assert_eq!(inside.map_gamut(space, policy).unwrap(), inside);
        }
        let source = Oklch::new(0.6, 0.4, 40.)
            .unwrap()
            .to_oklab()
            .unwrap()
            .to_linear(space)
            .unwrap();
        let mapped = source.map_gamut(space, GamutMapping::ReduceChroma).unwrap();
        assert!(mapped.is_in_gamut());
        let [l, c, h] = mapped.to_oklab().unwrap().to_oklch().unwrap().components();
        assert!((l - 0.6).abs() < 1e-7);
        assert!(c < 0.4 && c > 0.05);
        assert!((h - 40.).abs() < 1e-4);
        assert_eq!(
            mapped,
            source.map_gamut(space, GamutMapping::ReduceChroma).unwrap()
        );
    }
    let v = LinearRgb::new(RgbSpace::Srgb, [-0.2, 0.5, 1.2]).unwrap();
    assert_eq!(
        v.map_gamut(RgbSpace::Srgb, GamutMapping::Clip)
            .unwrap()
            .components(),
        [0., 0.5, 1.]
    );
    for (l, expected) in [(-0.1, [0.; 3]), (1.1, [1.; 3])] {
        let source = Oklch::new(l, 0.1, 40.)
            .unwrap()
            .to_oklab()
            .unwrap()
            .to_linear(RgbSpace::Srgb)
            .unwrap();
        assert_eq!(
            source
                .map_gamut(RgbSpace::Srgb, GamutMapping::ReduceChroma)
                .unwrap()
                .components(),
            expected
        );
    }
}

#[test]
fn mapping_grid_is_bounded_and_idempotent() {
    for destination in [
        RgbSpace::Srgb,
        RgbSpace::DisplayP3,
        RgbSpace::Rec2020,
        RgbSpace::AdobeRgb1998,
    ] {
        for l in [0.05, 0.3, 0.7, 0.99] {
            for h in (0..360).step_by(15) {
                let source = Oklch::new(l, 0.5, h as f64)
                    .unwrap()
                    .to_oklab()
                    .unwrap()
                    .to_linear(RgbSpace::Rec2020)
                    .unwrap();
                let result = source
                    .map_gamut(destination, GamutMapping::ReduceChroma)
                    .unwrap();
                assert!(result.is_in_gamut());
                assert_eq!(
                    result
                        .map_gamut(destination, GamutMapping::ReduceChroma)
                        .unwrap(),
                    result
                );
            }
        }
    }
}

#[test]
fn mixed_primary_stops_are_converted_before_rgb_interpolation() {
    let a = rgba([1., 0., 0.], 1.);
    let b = LinearRgba::new(
        LinearRgb::new(RgbSpace::DisplayP3, [1., 0., 0.]).unwrap(),
        1.,
    )
    .unwrap();
    let g = gradient(a, b, ColorInterpolation::LinearRgb);
    let mid = g.sample_at(0.5).unwrap();
    near(
        mid.rgb().components(),
        [1.11247008814028, -0.0210284773548441, -0.0098187772951672],
        1e-13,
    );
    assert!(!mid.rgb().is_in_gamut());
    assert!(
        mid.map_gamut(RgbSpace::Srgb, GamutMapping::Clip)
            .unwrap()
            .rgb()
            .is_in_gamut()
    );
}
