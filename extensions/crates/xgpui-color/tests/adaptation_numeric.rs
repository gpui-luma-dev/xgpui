use xgpui_color::{BradfordAdaptation, RgbCurve, RgbDefinition, WhitePoint};
#[path = "reference/bradford.rs"]
mod reference;

#[test]
fn bradford_matches_independent_decimal_vectors_with_scale_aware_bounds() {
    for &(source_d50, input, expected) in reference::VECTORS {
        let (source, dest) = if source_d50 {
            (WhitePoint::D50, WhitePoint::D65)
        } else {
            (WhitePoint::D65, WhitePoint::D50)
        };
        let transform = BradfordAdaptation::new(source, dest);
        let actual = transform.apply(input).unwrap();
        for row in 0..3 {
            // Include input L1 magnitude: cancellation while constructing CAT
            // off-diagonals can dominate their relative error. For these two
            // well-conditioned CATs, 32 eps covers construction plus the dot
            // product without a fixed absolute floor that hides dark errors.
            let magnitude: f64 = transform.matrix()[row]
                .iter()
                .zip(input)
                .map(|(a, b)| (a * b).abs())
                .sum();
            let budget =
                32. * f64::EPSILON * (magnitude + input.iter().map(|v| v.abs()).sum::<f64>())
                    + f64::from_bits(1) * 4.;
            assert!(
                (actual[row] - expected[row]).abs() <= budget,
                "{source_d50} {input:?} row {row}: {} vs {}, budget {budget}",
                actual[row],
                expected[row]
            );
        }
    }
}

#[test]
fn independent_css_matrix_columns_and_white_reference() {
    // Published CSS Color 4 D50→D65 Bradford matrix, independent of generator.
    let expected = [
        [0.955473421488075, -0.02309845494876471, 0.06325924320057072],
        [
            -0.0283697093338637,
            1.0099953980813041,
            0.021041441191917323,
        ],
        [
            0.012314014864481998,
            -0.020507649298898964,
            1.330365926242124,
        ],
    ];
    let transform = BradfordAdaptation::new(WhitePoint::D50, WhitePoint::D65);
    for (a, b) in transform
        .matrix()
        .into_iter()
        .flatten()
        .zip(expected.into_iter().flatten())
    {
        assert!((a - b).abs() <= 8. * f64::EPSILON);
    }
    for (a, b) in transform
        .apply(WhitePoint::D50.xyz())
        .unwrap()
        .into_iter()
        .zip([3127. / 3290., 1., 3583. / 3290.])
    {
        assert!((a - b).abs() <= 16. * f64::EPSILON * b);
    }
}

#[test]
fn identity_is_exact_and_invalid_or_overflowing_xyz_is_rejected() {
    for white in [WhitePoint::D50, WhitePoint::D65] {
        let identity = BradfordAdaptation::new(white, white);
        let values = [-0., f64::from_bits(1), f64::MAX];
        assert_eq!(
            identity.apply(values).unwrap().map(f64::to_bits),
            values.map(f64::to_bits)
        );
        assert!(identity.apply([f64::NAN, 0., 0.]).is_err());
    }
    let transform = BradfordAdaptation::new(WhitePoint::D50, WhitePoint::D65);
    assert!(transform.apply([0., 0., f64::MAX]).is_err());
    assert!(transform.apply([f64::INFINITY, 0., 0.]).is_err());
    assert!(transform.apply_colorants([[f64::NAN; 3]; 3]).is_err());
    assert!(
        transform
            .apply([f64::from_bits(1); 3])
            .unwrap()
            .iter()
            .all(|v| v.is_finite())
    );
}

#[test]
fn d50_colorants_adapt_once_and_validate_source_white() {
    // Synthetic diagonal D50 colorants: the adapted columns equal each
    // published CAT column times the corresponding D50 white component.
    let w = WhitePoint::D50.xyz();
    let source = [[w[0], 0., 0.], [0., w[1], 0.], [0., 0., w[2]]];
    let definition =
        RgbDefinition::new_with_white_point(source, [RgbCurve::Linear; 3], WhitePoint::D50)
            .unwrap();
    let green = definition
        .decode_to_xyz_d65([0., 1., 0.])
        .unwrap()
        .components();
    for (a, b) in green.into_iter().zip([
        -0.02309845494876471,
        1.0099953980813041,
        -0.020507649298898964,
    ]) {
        assert!((a - b).abs() <= 8. * f64::EPSILON);
    }
    assert!(RgbDefinition::new_d65(source, [RgbCurve::Linear; 3]).is_err());
    assert!(
        RgbDefinition::new_with_white_point(
            definition.linear_to_xyz_d65_matrix(),
            [RgbCurve::Linear; 3],
            WhitePoint::D50
        )
        .is_err()
    );
    let unchanged = RgbDefinition::new_with_white_point(
        definition.linear_to_xyz_d65_matrix(),
        [RgbCurve::Linear; 3],
        WhitePoint::D65,
    )
    .unwrap();
    assert_eq!(unchanged, definition);
    // Quantized metadata has an admission bound distinct from numerical error.
    let quantized = source.map(|r| r.map(|v| (v * 65536.).round() / 65536.));
    assert!(
        RgbDefinition::new_with_white_point(quantized, [RgbCurve::Linear; 3], WhitePoint::D50)
            .is_ok()
    );
    let mut bad = source;
    bad[1][1] += 4. / 65536.;
    assert!(
        RgbDefinition::new_with_white_point(bad, [RgbCurve::Linear; 3], WhitePoint::D50).is_err()
    );
    assert!(
        RgbDefinition::new_with_white_point(source, [RgbCurve::Gamma(0.); 3], WhitePoint::D50)
            .is_err()
    );
}
