use xgpui_color::{EncodedRgb, GamutMapping, LinearRgb, RgbSpace, XyzD65};

#[allow(dead_code)]
#[path = "reference/adobe_rgb.rs"]
mod reference;

const ADOBE: RgbSpace = RgbSpace::AdobeRgb1998;
const SPACES: [RgbSpace; 4] = [
    RgbSpace::Srgb,
    RgbSpace::DisplayP3,
    RgbSpace::Rec2020,
    ADOBE,
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
fn adobe_transfer_matches_120_digit_oracle_across_binary64_range() {
    for (encode, values) in [
        (false, reference::DECODE_F64),
        (true, reference::ENCODE_F64),
    ] {
        for &(input, expected) in values {
            // Decode exponent 563/256 is exactly binary-representable. Encode's
            // 256/563 rounds by at most half an ulp (EPSILON/8 at this exponent).
            // Its relative effect grows with |ln(input)|. Add 16 eps for powf
            // evaluation/rounding, plus eight minimum subnormals for underflow.
            let exponent_budget = if encode && input > 0. {
                input.ln().abs() * f64::EPSILON / 8.
            } else {
                0.
            };
            let budget = expected.abs() * (16. * f64::EPSILON + exponent_budget) + 8. * SUBNORMAL;
            for sign in [-1., 1.] {
                let actual = if encode {
                    LinearRgb::new(ADOBE, [input * sign; 3])
                        .unwrap()
                        .encode()
                        .unwrap()
                        .components()
                } else {
                    EncodedRgb::new(ADOBE, [input * sign; 3])
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
fn adobe_matrices_match_independent_chromaticity_solve() {
    let matrices = [
        (ADOBE.linear_to_xyz_matrix(), reference::TO_XYZ),
        (ADOBE.xyz_to_linear_matrix(), reference::FROM_XYZ),
    ]
    .into_iter()
    .chain(SPACES.into_iter().enumerate().flat_map(|(i, space)| {
        [
            (
                ADOBE.linear_to_linear_matrix(space),
                reference::TO_SPACES[i],
            ),
            (
                space.linear_to_linear_matrix(ADOBE),
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
fn adobe_conversion_dense_grid_has_scale_aware_error_bound() {
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
                        let actual = LinearRgb::new(ADOBE, input)
                            .unwrap()
                            .convert(space)
                            .unwrap()
                            .components();
                        let magnitude = input.into_iter().map(f64::abs).fold(0., f64::max);
                        // Normwise bound includes coefficient construction and
                        // accumulation, including cancellation near a zero result.
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
fn adobe_full_16_bit_transfer_ramp_is_monotonic_and_round_trips() {
    let mut previous = -1.;
    for code in 0..=u16::MAX {
        let encoded = code as f64 / u16::MAX as f64;
        let linear = EncodedRgb::new(ADOBE, [encoded; 3])
            .unwrap()
            .decode()
            .unwrap();
        assert!(
            linear.components()[0] > previous,
            "non-monotonic at code {code}"
        );
        previous = linear.components()[0];
        let actual = linear.encode().unwrap().components()[0];
        scalar_close(
            actual,
            encoded,
            encoded * 64. * f64::EPSILON + 8. * SUBNORMAL,
            "16-bit transfer ramp",
        );
    }
}

#[test]
fn adobe_signed_zero_subnormals_invalid_inputs_and_overflow_are_explicit() {
    for zero in [0_f64, -0_f64] {
        let decoded = EncodedRgb::new(ADOBE, [zero; 3])
            .unwrap()
            .decode()
            .unwrap()
            .components()[0];
        let encoded = LinearRgb::new(ADOBE, [zero; 3])
            .unwrap()
            .encode()
            .unwrap()
            .components()[0];
        assert_eq!(decoded.to_bits(), zero.to_bits());
        assert_eq!(encoded.to_bits(), zero.to_bits());
    }
    for endpoint in [-1., 1.] {
        assert_eq!(
            EncodedRgb::new(ADOBE, [endpoint; 3])
                .unwrap()
                .decode()
                .unwrap()
                .components(),
            [endpoint; 3]
        );
        assert_eq!(
            LinearRgb::new(ADOBE, [endpoint; 3])
                .unwrap()
                .encode()
                .unwrap()
                .components(),
            [endpoint; 3]
        );
    }
    // Underflow is permitted to round to signed zero, never NaN or infinity.
    for sign in [-1., 1.] {
        let decoded = EncodedRgb::new(ADOBE, [SUBNORMAL * sign; 3])
            .unwrap()
            .decode()
            .unwrap()
            .components()[0];
        assert_eq!(decoded.to_bits(), (0_f64.copysign(sign)).to_bits());
        assert!(
            LinearRgb::new(ADOBE, [SUBNORMAL * sign; 3])
                .unwrap()
                .encode()
                .unwrap()
                .components()[0]
                .is_finite()
        );
        assert!(
            EncodedRgb::new(ADOBE, [1e140 * sign; 3])
                .unwrap()
                .decode()
                .is_ok()
        );
        assert!(
            EncodedRgb::new(ADOBE, [1e141 * sign; 3])
                .unwrap()
                .decode()
                .is_err()
        );
        assert!(
            EncodedRgb::new(ADOBE, [f64::MAX * sign; 3])
                .unwrap()
                .decode()
                .is_err()
        );
    }
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for channel in 0..3 {
            let mut input = [0.5; 3];
            input[channel] = bad;
            assert!(EncodedRgb::new(ADOBE, input).is_err());
            assert!(LinearRgb::new(ADOBE, input).is_err());
        }
    }
    assert!(
        XyzD65::new([f64::MAX, -f64::MAX, f64::MAX])
            .unwrap()
            .to_linear(ADOBE)
            .is_err()
    );
}

#[test]
fn adobe_gamut_boundary_uses_adjacent_floats_without_implicit_clipping() {
    let inside = [1_f64.next_down(), SUBNORMAL, 0.];
    let outside = [1_f64.next_up(), -SUBNORMAL, 0.];
    assert!(LinearRgb::new(ADOBE, inside).unwrap().is_in_gamut());
    let rgb = LinearRgb::new(ADOBE, outside).unwrap();
    assert!(!rgb.is_in_gamut());
    assert_eq!(rgb.convert(ADOBE).unwrap().components(), outside);
    assert_eq!(
        rgb.map_gamut(ADOBE, GamutMapping::Clip)
            .unwrap()
            .components(),
        [1., 0., 0.]
    );
}
