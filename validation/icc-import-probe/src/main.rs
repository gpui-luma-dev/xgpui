use moxcms::{ColorProfile, Layout, RenderingIntent, TransformOptions, curve_from_gamma};
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
