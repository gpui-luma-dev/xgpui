use xgpui_color::{
    BradfordAdaptation, ColorInterpolation, EncodedRgb, GamutMapping, LinearGradient, LinearRgb,
    LinearRgba, RgbSpace, WhitePoint, XyzD65,
};

#[allow(dead_code)]
#[path = "reference/prophoto_rgb.rs"]
mod reference;

const PROPHOTO: RgbSpace = RgbSpace::ProPhotoRgb;
const SPACES: [RgbSpace; 5] = [
    RgbSpace::Srgb,
    RgbSpace::DisplayP3,
    RgbSpace::Rec2020,
    RgbSpace::AdobeRgb1998,
    PROPHOTO,
];
const SUBNORMAL: f64 = f64::from_bits(1);

fn scalar_close(actual: f64, expected: f64, budget: f64, context: &str) {
    assert!(
        actual.is_finite() && (actual - expected).abs() <= budget,
        "{context}: actual={actual:e}, reference={expected:e}, error={:e}, budget={budget:e}",
        (actual - expected).abs()
    );
}

#[test]
fn prophoto_transfer_matches_120_digit_oracle_across_binary64_range() {
    for (encode, values) in [
        (false, reference::DECODE_F64),
        (true, reference::ENCODE_F64),
    ] {
        for &(input, expected) in values {
            // Linear branches divide or multiply by 16 exactly. The binary64
            // exponents 1.8 and 1/1.8 are each within one EPSILON (relative) of
            // 9/5 and 5/9; that error scales with |ln(input)|. Add 16 eps for
            // powf evaluation/rounding, plus eight minimum subnormals.
            let power = if encode {
                input >= 1. / 512.
            } else {
                input > 16. / 512.
            };
            let exponent_budget = if power {
                input.ln().abs() * f64::EPSILON
            } else {
                0.
            };
            let budget = expected.abs() * (16. * f64::EPSILON + exponent_budget) + 8. * SUBNORMAL;
            for sign in [-1., 1.] {
                let actual = if encode {
                    LinearRgb::new(PROPHOTO, [input * sign; 3])
                        .unwrap()
                        .encode()
                        .unwrap()
                        .components()
                } else {
                    EncodedRgb::new(PROPHOTO, [input * sign; 3])
                        .unwrap()
                        .decode()
                        .unwrap()
                        .components()
                };
                for actual in actual {
                    scalar_close(
                        actual,
                        expected * sign,
                        budget,
                        if encode { "encode" } else { "decode" },
                    );
                }
            }
        }
    }
}

#[test]
fn prophoto_transfer_breakpoints_are_exact_and_continuous() {
    // 16/512 == (1/512)^(1/1.8): both branches meet at the knee.
    let decoded = EncodedRgb::new(PROPHOTO, [16. / 512., -16. / 512., 0.])
        .unwrap()
        .decode()
        .unwrap()
        .components();
    assert_eq!(decoded, [1. / 512., -1. / 512., 0.]);
    let encoded = LinearRgb::new(PROPHOTO, [1. / 512., -1. / 512., 0.])
        .unwrap()
        .encode()
        .unwrap()
        .components();
    for (actual, expected) in encoded.into_iter().zip([16. / 512., -16. / 512., 0.]) {
        scalar_close(
            actual,
            expected,
            4. * f64::EPSILON * 16. / 512.,
            "encode knee",
        );
    }
    // Just below the knee is the exact linear segment, not gamma 1.8.
    let below = (16_f64 / 512.).next_down();
    assert_eq!(
        EncodedRgb::new(PROPHOTO, [below; 3])
            .unwrap()
            .decode()
            .unwrap()
            .components(),
        [below / 16.; 3]
    );
    for zero in [0_f64, -0_f64] {
        let decoded = EncodedRgb::new(PROPHOTO, [zero; 3])
            .unwrap()
            .decode()
            .unwrap()
            .components()[0];
        assert_eq!(decoded.to_bits(), zero.to_bits());
    }
    for endpoint in [-1., 1.] {
        assert_eq!(
            EncodedRgb::new(PROPHOTO, [endpoint; 3])
                .unwrap()
                .decode()
                .unwrap()
                .components(),
            [endpoint; 3]
        );
    }
}

#[test]
fn prophoto_full_16_bit_transfer_ramp_is_monotonic_and_round_trips() {
    let mut previous = -1.;
    for code in 0..=u16::MAX {
        let encoded = code as f64 / u16::MAX as f64;
        let linear = EncodedRgb::new(PROPHOTO, [encoded; 3])
            .unwrap()
            .decode()
            .unwrap();
        assert!(
            linear.components()[0] > previous,
            "non-monotonic at code {code}"
        );
        previous = linear.components()[0];
        scalar_close(
            linear.encode().unwrap().components()[0],
            encoded,
            encoded * 64. * f64::EPSILON + 8. * SUBNORMAL,
            "16-bit transfer ramp",
        );
    }
}

#[test]
fn prophoto_overflow_and_invalid_inputs_are_explicit() {
    for sign in [-1., 1.] {
        assert!(
            EncodedRgb::new(PROPHOTO, [1e170 * sign; 3])
                .unwrap()
                .decode()
                .is_ok()
        );
        assert!(
            EncodedRgb::new(PROPHOTO, [1e172 * sign; 3])
                .unwrap()
                .decode()
                .is_err()
        );
        assert!(
            LinearRgb::new(PROPHOTO, [f64::MAX * sign; 3])
                .unwrap()
                .encode()
                .is_ok()
        );
    }
    assert!(
        XyzD65::new([f64::MAX, -f64::MAX, f64::MAX])
            .unwrap()
            .to_linear(PROPHOTO)
            .is_err()
    );
}

#[test]
fn prophoto_matrices_match_independent_d50_solve_and_bradford() {
    let matrices = [
        (PROPHOTO.linear_to_xyz_matrix(), reference::TO_XYZ),
        (PROPHOTO.xyz_to_linear_matrix(), reference::FROM_XYZ),
    ]
    .into_iter()
    .chain(SPACES.into_iter().enumerate().flat_map(|(i, space)| {
        [
            (
                PROPHOTO.linear_to_linear_matrix(space),
                reference::TO_SPACES[i],
            ),
            (
                space.linear_to_linear_matrix(PROPHOTO),
                reference::FROM_SPACES[i],
            ),
        ]
    }));
    for (actual, expected) in matrices {
        for (actual, expected) in actual
            .into_iter()
            .flatten()
            .zip(expected.into_iter().flatten())
        {
            scalar_close(
                actual,
                expected,
                32. * f64::EPSILON * expected.abs().max(1.),
                "matrix coefficient",
            );
        }
    }
}

#[test]
fn prophoto_adapts_d50_to_d65_exactly_once() {
    // Reference white maps to D65, not D50 (no adaptation) or a double shift.
    let white = LinearRgb::new(PROPHOTO, [1.; 3])
        .unwrap()
        .to_xyz()
        .unwrap()
        .components();
    let d65 = WhitePoint::D65.xyz();
    for (actual, expected) in white.into_iter().zip(d65) {
        scalar_close(actual, expected, 4. * f64::EPSILON, "white");
    }
    // Undoing one adaptation recovers the published native D50 colorants.
    let native = BradfordAdaptation::new(WhitePoint::D65, WhitePoint::D50)
        .apply_colorants(PROPHOTO.linear_to_xyz_matrix())
        .unwrap();
    let rows = [
        [0.7977666449006423, 0.13518129740053308, 0.0313477341283922],
        [0.2880748288194013, 0.711835234241873, 0.00008993693872564],
        [0., 0., 0.8251046025104602],
    ];
    for (actual, expected) in native.into_iter().flatten().zip(rows.into_iter().flatten()) {
        scalar_close(actual, expected, 16. * f64::EPSILON, "native D50 colorant");
    }
    // Neutrals stay neutral in every other named space.
    for space in SPACES {
        let gray = LinearRgb::new(PROPHOTO, [0.18; 3])
            .unwrap()
            .convert(space)
            .unwrap()
            .components();
        for v in gray {
            scalar_close(v, 0.18, 16. * f64::EPSILON, "adapted neutral");
        }
    }
}

#[test]
fn prophoto_conversion_dense_grid_has_scale_aware_error_bound() {
    for (i, space) in SPACES.into_iter().enumerate() {
        let matrix = reference::TO_SPACES[i];
        let norm = matrix
            .map(|row| row.into_iter().map(f64::abs).sum::<f64>())
            .into_iter()
            .fold(0., f64::max);
        for scale in [1e-300, 1e-12, 1., 1e100] {
            for r in -2..=8 {
                for g in -2..=8 {
                    for b in -2..=8 {
                        let input = [r, g, b].map(|v| v as f64 / 8. * scale);
                        let expected =
                            matrix.map(|row| row.into_iter().zip(input).map(|(a, b)| a * b).sum());
                        let actual = LinearRgb::new(PROPHOTO, input)
                            .unwrap()
                            .convert(space)
                            .unwrap()
                            .components();
                        let magnitude = input.into_iter().map(f64::abs).fold(0., f64::max);
                        let budget = 64. * f64::EPSILON * norm * magnitude + 8. * SUBNORMAL;
                        for (actual, expected) in actual.into_iter().zip(expected) {
                            scalar_close(actual, expected, budget, "scaled primary conversion");
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn prophoto_containment_of_smaller_gamuts_is_nearly_but_not_exactly_complete() {
    // sRGB and Adobe RGB cubes are inside the ProPhoto RGB cube. After the
    // D50/D65 Bradford adaptation, Display P3 and Rec.2020 red (and their cyan
    // complements) fall slightly outside it: blue reaches about -0.0013 (P3)
    // and -0.0023 (Rec.2020). RGB-cube membership is not physical
    // realizability either: ProPhoto's green and blue primaries are imaginary.
    let outside = |space: RgbSpace, rgb: [f64; 3]| {
        matches!(space, RgbSpace::DisplayP3 | RgbSpace::Rec2020)
            && (rgb == [1., 0., 0.] || rgb == [0., 1., 1.])
    };
    for space in &SPACES[..4] {
        for corner in 0..8 {
            let rgb = [corner & 1, corner >> 1 & 1, corner >> 2 & 1].map(|v| v as f64);
            let v = LinearRgb::new(*space, rgb)
                .unwrap()
                .convert(PROPHOTO)
                .unwrap()
                .components();
            let inside = v.iter().all(|c| *c >= -1e-12 && *c <= 1. + 1e-12);
            assert_eq!(inside, !outside(*space, rgb), "{space:?} {rgb:?} -> {v:?}");
        }
    }
    for (space, blue) in [
        (RgbSpace::DisplayP3, -0.0012727345647388125),
        (RgbSpace::Rec2020, -0.0023420389707253983),
    ] {
        let v = LinearRgb::new(space, [1., 0., 0.])
            .unwrap()
            .convert(PROPHOTO)
            .unwrap()
            .components();
        scalar_close(v[2], blue, 1e-15, "red outside ProPhoto");
    }
    // ProPhoto primaries leave every bounded storage gamut.
    for primary in [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]] {
        for space in [RgbSpace::DisplayP3, RgbSpace::Rec2020] {
            let v = LinearRgb::new(PROPHOTO, primary)
                .unwrap()
                .convert(space)
                .unwrap();
            assert!(!v.is_in_gamut(), "{primary:?} in {space:?}: {v:?}");
        }
    }
}

#[test]
fn prophoto_reduce_chroma_keeps_lightness_and_hue_into_storage_gamuts() {
    // Saturated ProPhoto colors exercise the neutral-connected search: the
    // fixed-lightness neutral must be in gamut and the result must keep OkLab
    // lightness and hue. Very dark imaginary blues may reach the neutral.
    for destination in [RgbSpace::Srgb, RgbSpace::DisplayP3, RgbSpace::Rec2020] {
        for r in 0..=4 {
            for g in 0..=4 {
                for b in 0..=4 {
                    let rgb = [r, g, b].map(|v| v as f64 / 4.);
                    let source = LinearRgb::new(PROPHOTO, rgb).unwrap();
                    let mapped = source
                        .map_gamut(destination, GamutMapping::ReduceChroma)
                        .unwrap();
                    assert!(mapped.is_in_gamut(), "{rgb:?} -> {mapped:?}");
                    let [l0, c0, h0] = source.to_oklab().unwrap().to_oklch().unwrap().components();
                    let [l1, c1, h1] = mapped.to_oklab().unwrap().to_oklch().unwrap().components();
                    if l0 <= 0. || l0 >= 1. {
                        continue;
                    }
                    assert!((l0 - l1).abs() < 1e-6, "lightness {rgb:?}: {l0} -> {l1}");
                    assert!(c1 <= c0 + 1e-9, "chroma grew {rgb:?}");
                    if c1 > 1e-3 {
                        let dh = (h0 - h1 + 540.).rem_euclid(360.) - 180.;
                        assert!(dh.abs() < 1e-3, "hue {rgb:?}: {h0} -> {h1}");
                    }
                }
            }
        }
    }
}

#[test]
fn prophoto_gradient_keeps_extended_values_until_mapping() {
    let green = LinearRgba::new(LinearRgb::new(PROPHOTO, [0., 1., 0.]).unwrap(), 1.).unwrap();
    let blue = LinearRgba::new(LinearRgb::new(PROPHOTO, [0., 0., 1.]).unwrap(), 0.5).unwrap();
    for model in [ColorInterpolation::LinearRgb, ColorInterpolation::Oklab] {
        let gradient =
            LinearGradient::new([0., 0.], [100., 0.], [green, blue], PROPHOTO, model).unwrap();
        let mid = gradient.sample([50., 0.]).unwrap();
        let p3 = mid
            .map_gamut(RgbSpace::DisplayP3, GamutMapping::ReduceChroma)
            .unwrap();
        assert!(p3.premultiplied().iter().all(|v| v.is_finite()));
        let extended = mid.rgb().convert(RgbSpace::DisplayP3).unwrap();
        assert!(!extended.is_in_gamut(), "{model:?}: {extended:?}");
    }
}
