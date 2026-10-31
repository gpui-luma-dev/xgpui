use moxcms::{ColorProfile, Layout, RenderingIntent, TransformOptions, curve_from_gamma};
mod curve_reference;
mod reference;

fn options() -> TransformOptions {
    TransformOptions {
        rendering_intent: RenderingIntent::RelativeColorimetric,
        prefer_fixed_point: false,
        allow_use_cicp_transfer: false,
        allow_extended_range_rgb_xyz: true,
        ..Default::default()
    }
}

fn check_profiles() {
    let source = ColorProfile::new_from_slice(include_bytes!("../fixtures/adobe-rgb.icc")).unwrap();
    let mut max_error = 0_f64;
    let mut max_budget_fraction = 0_f64;
    for rec2020 in [false, true] {
        let destination = ColorProfile::new_from_slice(if rec2020 {
            include_bytes!("../fixtures/linear-rec2020.icc").as_slice()
        } else {
            include_bytes!("../fixtures/linear-p3.icc").as_slice()
        })
        .unwrap();
        let transform = source
            .create_transform_f64(Layout::Rgb, &destination, Layout::Rgb, options())
            .unwrap();
        let mut saw_negative = false;
        for &(is_rec, rgb, expected) in reference::REFERENCES {
            if is_rec != rec2020 {
                continue;
            }
            let mut actual = [0.; 3];
            transform.transform(&rgb, &mut actual).unwrap();
            // This engine's floating transform uses f32 internally even with
            // f64 input/output. Scale the budget to decoded source energy,
            // including cross-channel matrix contributions near cancellation.
            let magnitude: f64 = rgb.iter().map(|v| v.powf(563. / 256.)).sum();
            let budget = 16. * f64::from(f32::EPSILON) * magnitude;
            for (a, e) in actual.into_iter().zip(expected) {
                let error = (a - e).abs();
                assert!(
                    a.is_finite() && error <= budget,
                    "rec2020={rec2020} {rgb:?}: {a:e} vs {e:e}, error={error:e}, budget={budget:e}"
                );
                if magnitude == 0. {
                    assert_eq!(a, 0.);
                }
                max_error = max_error.max(error);
                if budget > 0. {
                    max_budget_fraction = max_budget_fraction.max(error / budget);
                }
                saw_negative |= a < 0.;
            }
        }
        if !rec2020 {
            assert!(
                saw_negative,
                "P3 must retain Adobe colors outside bounded P3"
            );
        }
    }
    println!(
        "432 serialized-profile vectors vs Little CMS: max abs error {max_error:e}; max budget fraction {max_budget_fraction:.4}"
    );
}

fn check_curve_profiles(sampled: bool) -> usize {
    let source = ColorProfile::new_from_slice(if sampled {
        include_bytes!("../fixtures/sampled-rgb.icc").as_slice()
    } else {
        include_bytes!("../fixtures/parametric-rgb.icc").as_slice()
    })
    .unwrap();
    match source.red_trc.as_ref().unwrap() {
        moxcms::ToneReprCurve::Lut(values) if sampled => assert_eq!(values.len(), 257),
        moxcms::ToneReprCurve::Parametric(_) if !sampled => {}
        curve => panic!("unexpected source curve: {curve:?}"),
    }
    if sampled {
        assert_ne!(source.red_trc, source.green_trc);
    }
    let mut max_error = 0_f64;
    let mut max_lcms_error = 0_f64;
    let mut max_budget_fraction = 0_f64;
    let mut count = 0;
    let mut violations = 0;
    let mut first_failure = None;
    for rec2020 in [false, true] {
        let destination = ColorProfile::new_from_slice(if rec2020 {
            include_bytes!("../fixtures/linear-rec2020.icc").as_slice()
        } else {
            include_bytes!("../fixtures/linear-p3.icc").as_slice()
        })
        .unwrap();
        let transform = source
            .create_transform_f64(Layout::Rgb, &destination, Layout::Rgb, options())
            .unwrap();
        for &(is_sampled, is_rec, rgb, decoded, lcms, expected) in curve_reference::CURVE_REFERENCES
        {
            if is_sampled != sampled || is_rec != rec2020 {
                continue;
            }
            count += 1;
            let mut actual = [0.; 3];
            transform.transform(&rgb, &mut actual).unwrap();
            let magnitude: f64 = decoded.iter().map(|v| v.abs()).sum();
            let budget = 16. * f64::from(f32::EPSILON) * magnitude;
            let mut failed = false;
            for channel in 0..3 {
                let a = actual[channel];
                let e = expected[channel];
                assert!(a.is_finite() && e.is_finite() && lcms[channel].is_finite());
                let error = (a - e).abs();
                if error > budget {
                    failed = true;
                    first_failure.get_or_insert_with(|| format!("rec2020={rec2020} {rgb:?} channel {channel}: {a:e} vs {e:e}, error={error:e}, budget={budget:e}"));
                }
                max_error = max_error.max(error);
                max_lcms_error = max_lcms_error.max((lcms[channel] - e).abs());
                if budget > 0. {
                    max_budget_fraction = max_budget_fraction.max(error / budget);
                }
            }
            violations += usize::from(failed);
        }
    }
    println!(
        "{count} curve vectors sampled={sampled}: {violations} outside f32 budget; max abs error vs exact tags {max_error:e}; max budget fraction {max_budget_fraction:.4}; Little CMS max error vs exact tags {max_lcms_error:e}"
    );
    if let Some(failure) = first_failure {
        println!("First precision failure: {failure}");
    }
    violations
}

fn inverse_matrix(matrix: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let mut rows: [[f64; 6]; 3] = std::array::from_fn(|i| {
        std::array::from_fn(|j| {
            if j < 3 {
                matrix[i][j]
            } else {
                f64::from(i == j - 3)
            }
        })
    });
    for column in 0..3 {
        let pivot = (column..3)
            .max_by(|a, b| rows[*a][column].abs().total_cmp(&rows[*b][column].abs()))
            .unwrap();
        rows.swap(column, pivot);
        let scale = rows[column][column];
        assert!(scale.is_finite() && scale != 0.);
        rows[column] = rows[column].map(|v| v / scale);
        for i in 0..3 {
            if i != column {
                let factor = rows[i][column];
                let normalized = rows[column];
                for (value, pivot) in rows[i].iter_mut().zip(normalized) {
                    *value -= factor * pivot;
                }
            }
        }
    }
    std::array::from_fn(|i| std::array::from_fn(|j| rows[i][j + 3]))
}

fn check_replacement_curves() {
    use xgpui_color::icc::IccRgbProfile;
    let mut count = 0;
    let mut max_error = 0_f64;
    for sampled in [false, true] {
        let source = IccRgbProfile::parse(if sampled {
            include_bytes!("../fixtures/sampled-rgb.icc").as_slice()
        } else {
            include_bytes!("../fixtures/parametric-rgb.icc").as_slice()
        })
        .unwrap();
        for rec2020 in [false, true] {
            let destination = IccRgbProfile::parse(if rec2020 {
                include_bytes!("../fixtures/linear-rec2020.icc").as_slice()
            } else {
                include_bytes!("../fixtures/linear-p3.icc").as_slice()
            })
            .unwrap();
            let inverse = inverse_matrix(destination.colorants());
            for &(is_sampled, is_rec, rgb, decoded, _, expected) in
                curve_reference::CURVE_REFERENCES
            {
                if sampled != is_sampled || rec2020 != is_rec {
                    continue;
                }
                count += 1;
                let pcs = source.decode_to_pcs(rgb).unwrap();
                let actual =
                    inverse.map(|row| row.into_iter().zip(pcs).map(|(a, b)| a * b).sum::<f64>());
                let magnitude: f64 = decoded.iter().map(|v| v.abs()).sum();
                for i in 0..3 {
                    let error = (actual[i] - expected[i]).abs();
                    assert!(
                        error <= 64. * f64::EPSILON * magnitude + 4. * f64::from_bits(1),
                        "replacement sampled={sampled} rec2020={rec2020} {rgb:?} channel {i}: {} vs {}, error={error:e}",
                        actual[i],
                        expected[i]
                    );
                    max_error = max_error.max(error);
                    if magnitude == 0. {
                        assert_eq!(actual[i], 0.);
                    }
                }
            }
        }
    }
    println!(
        "Replacement: {count} curve vectors PASS at f64 precision; max absolute error {max_error:e}"
    );
}

fn check_synthetic_destination() {
    let source = ColorProfile::new_adobe_rgb();
    let mut destination = ColorProfile::new_display_p3();
    let linear = curve_from_gamma(1.);
    destination.red_trc = Some(linear.clone());
    destination.green_trc = Some(linear.clone());
    destination.blue_trc = Some(linear);
    // We changed the transfer semantics. Keeping the factory's sRGB CICP tag
    // would make this synthetic profile internally inconsistent. Do not strip
    // metadata from real imported profiles as a workaround.
    destination.cicp = None;
    let transform = source
        .create_transform_f64(Layout::Rgb, &destination, Layout::Rgb, options())
        .unwrap();
    let mut out = [0.; 9];
    transform
        .transform(&[0., 1., 0., 0., 1., 1., 1., 1., 1.], &mut out)
        .unwrap();
    assert!(
        out[0] < 0. && out[2] > 0.,
        "Adobe green has negative P3 red and positive blue"
    );
    assert!(out[3] < 0., "Adobe cyan has negative P3 red");
    assert!(
        out[6..]
            .iter()
            .all(|v| (v - 1.).abs() < 4. * f64::from(f32::EPSILON))
    );
    println!("Consistent synthetic linear destination retains Adobe green/cyan: {out:?}");
}

fn main() {
    check_synthetic_destination();
    check_profiles();
    check_replacement_curves();
    let violations = check_curve_profiles(false) + check_curve_profiles(true);
    if std::env::args().any(|arg| arg == "--strict-curves") {
        assert_eq!(
            violations, 0,
            "curve precision acceptance remains unresolved"
        );
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn consistent_synthetic_destination_retains_extended_colors() {
        super::check_synthetic_destination();
    }
    #[test]
    fn serialized_profiles_match_independent_engine() {
        super::check_profiles();
    }
    #[test]
    fn curve_fixture_construction_and_finite_results() {
        // Diagnostic execution is not numerical acceptance. The two ignored
        // acceptance gates below deliberately retain their original budgets.
        super::check_curve_profiles(false);
        super::check_curve_profiles(true);
    }
    #[test]
    #[ignore = "known parametric near-black precision gap; run --ignored to reproduce"]
    fn parametric_curve_precision_acceptance() {
        assert_eq!(super::check_curve_profiles(false), 0);
    }
    #[test]
    #[ignore = "known sampled-curve interpolation precision gap; run --ignored to reproduce"]
    fn sampled_curve_precision_acceptance() {
        assert_eq!(super::check_curve_profiles(true), 0);
    }
    #[test]
    fn replacement_passes_both_curve_acceptance_sets() {
        super::check_replacement_curves();
    }
    #[test]
    fn malformed_profile_is_rejected() {
        for bytes in [
            &[][..],
            &b"not an ICC profile"[..],
            &include_bytes!("../fixtures/adobe-rgb.icc")[..100],
        ] {
            assert!(moxcms::ColorProfile::new_from_slice(bytes).is_err());
        }
    }
}
