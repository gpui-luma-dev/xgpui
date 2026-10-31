#![cfg(feature = "icc")]
use xgpui_color::icc::{IccError, IccRgbProfile};
use xgpui_color::{ParametricCurve, RgbCurve, SampledCurve};
#[allow(clippy::type_complexity)]
mod reference {
    include!("reference/icc_curves.rs");
}

#[test]
fn replacement_curves_pcs_and_adaptation_match_independent_120_digit_oracle() {
    for sampled in [false, true] {
        let profile = IccRgbProfile::parse(if sampled {
            include_bytes!("reference/sampled-rgb.icc").as_slice()
        } else {
            include_bytes!("reference/parametric-rgb.icc").as_slice()
        })
        .unwrap();
        for &(is_sampled, rgb, decoded, pcs, xyz) in reference::CORE_CURVES {
            if is_sampled != sampled {
                continue;
            }
            for i in 0..3 {
                let a = profile.rgb_definition().curves()[i].decode(rgb[i]).unwrap();
                assert!(
                    (a - decoded[i]).abs()
                        <= 16. * f64::EPSILON * decoded[i].abs() + 4. * f64::from_bits(1),
                    "sampled={sampled} {rgb:?} curve {i}: {a:e} vs {:e}",
                    decoded[i]
                );
            }
            let magnitude: f64 = decoded.iter().map(|v| v.abs()).sum();
            for (actual, expected) in [
                (profile.decode_to_pcs(rgb).unwrap(), pcs),
                (
                    profile
                        .rgb_definition()
                        .decode_to_xyz_d65(rgb)
                        .unwrap()
                        .components(),
                    xyz,
                ),
            ] {
                for i in 0..3 {
                    assert!(
                        (actual[i] - expected[i]).abs()
                            <= 64. * f64::EPSILON * magnitude + 4. * f64::from_bits(1),
                        "sampled={sampled} {rgb:?} channel {i}: {:e} vs {:e}",
                        actual[i],
                        expected[i]
                    );
                    if magnitude == 0. {
                        assert_eq!(actual[i], 0.);
                    }
                }
            }
        }
    }
}

#[test]
fn curve_validation_endpoints_knots_discontinuities_and_cloning() {
    let sampled = SampledCurve::new(vec![0, 16384, 65535]).unwrap();
    let curve = RgbCurve::Sampled(sampled.clone());
    assert_eq!(curve.decode(0.).unwrap(), 0.);
    assert_eq!(curve.decode(1.).unwrap(), 1.);
    assert_eq!(curve.decode(0.5).unwrap(), 16384. / 65535.);
    assert_eq!(curve.decode(0.25).unwrap(), 8192. / 65535.);
    assert_eq!(sampled.samples(), [0, 16384, 65535]);
    for samples in [
        vec![],
        vec![0],
        vec![1, 0],
        vec![0; SampledCurve::MAX_SAMPLES + 1],
    ] {
        assert!(SampledCurve::new(samples).is_err());
    }
    let p = ParametricCurve::new([2., 1., 0., 0.1, 0.5, 0., 0.]).unwrap();
    let p = RgbCurve::Parametric(p);
    assert_eq!(p.decode(0.5).unwrap(), 0.25); // Exact branch selection, no smoothing.
    assert!(p.decode(f64::from_bits(0.5_f64.to_bits() - 1)).unwrap() < 0.051);
    for x in [f64::NAN, f64::INFINITY, -0.1, 1.1] {
        assert!(p.decode(x).is_err());
        assert!(curve.decode(x).is_err());
    }
    for parameters in [
        [0., 1., 0., 0., 0., 0., 0.],
        [2., 1., -1., 0., 0., 0., 0.],
        [f64::NAN, 1., 0., 0., 0., 0., 0.],
        [10000., 2., 0., 0., 0., 0., 0.],
        [1., 1., 0., f64::MAX, 0.5, 0., f64::MAX],
    ] {
        assert!(ParametricCurve::new(parameters).is_err());
    }
}

#[test]
fn adapter_rejects_limits_unsupported_metadata_and_corrupt_structure() {
    let original = include_bytes!("reference/parametric-rgb.icc");
    assert_eq!(
        IccRgbProfile::parse(&vec![0; IccRgbProfile::MAX_PROFILE_BYTES + 1]).unwrap_err(),
        IccError::Limit
    );
    for len in [0, 100, 131, original.len() - 1] {
        assert!(IccRgbProfile::parse(&original[..len]).is_err());
    }
    for (at, bytes) in [
        (12, &b"prtr"[..]),
        (16, &b"CMYK"[..]),
        (20, &b"Lab "[..]),
        (132, &b"cicp"[..]),
        (132, &b"A2B0"[..]),
    ] {
        let mut bad = original.to_vec();
        bad[at..at + 4].copy_from_slice(bytes);
        assert_eq!(
            IccRgbProfile::parse(&bad).unwrap_err(),
            IccError::Unsupported
        );
    }
    let mut bad = original.to_vec();
    bad[128..132].copy_from_slice(&33u32.to_be_bytes());
    assert_eq!(IccRgbProfile::parse(&bad).unwrap_err(), IccError::Limit);
    let mut bad = original.to_vec();
    bad[136..140].copy_from_slice(&0u32.to_be_bytes());
    assert_eq!(IccRgbProfile::parse(&bad).unwrap_err(), IccError::Malformed);
    let mut bad = original.to_vec();
    let first: Vec<u8> = bad[132..136].to_vec();
    bad[144..148].copy_from_slice(&first);
    assert_eq!(IccRgbProfile::parse(&bad).unwrap_err(), IccError::Malformed);
}

fn tag_offset(bytes: &[u8], signature: &[u8; 4]) -> usize {
    let count = u32::from_be_bytes(bytes[128..132].try_into().unwrap()) as usize;
    let at = (0..count)
        .map(|i| 132 + i * 12)
        .find(|at| &bytes[*at..*at + 4] == signature)
        .unwrap();
    u32::from_be_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize
}
#[test]
fn table_limits_nonmonotonic_data_and_parametric_scope_are_explicit() {
    let original = include_bytes!("reference/sampled-rgb.icc");
    let at = tag_offset(original, b"rTRC");
    let mut bad = original.to_vec();
    bad[at + 8..at + 12].copy_from_slice(&65537u32.to_be_bytes());
    assert_eq!(IccRgbProfile::parse(&bad).unwrap_err(), IccError::Limit);
    let mut bad = original.to_vec();
    bad[at + 14..at + 16].copy_from_slice(&65535u16.to_be_bytes());
    assert_eq!(
        IccRgbProfile::parse(&bad).unwrap_err(),
        IccError::InvalidColor
    );
    let original = include_bytes!("reference/parametric-rgb.icc");
    let at = tag_offset(original, b"rTRC");
    let mut bad = original.to_vec();
    bad[at + 8..at + 10].copy_from_slice(&1u16.to_be_bytes());
    assert_eq!(
        IccRgbProfile::parse(&bad).unwrap_err(),
        IccError::Unsupported
    );
    let mut bad = original.to_vec();
    bad[at + 12..at + 16].copy_from_slice(&(129i32 * 65536).to_be_bytes());
    assert_eq!(
        IccRgbProfile::parse(&bad).unwrap_err(),
        IccError::Unsupported
    );
    let mut bad = original.to_vec();
    bad[68..72].copy_from_slice(&65536u32.to_be_bytes());
    assert_eq!(
        IccRgbProfile::parse(&bad).unwrap_err(),
        IccError::Unsupported
    );
}
#[test]
fn pcs_colorants_are_not_double_adapted_by_chad_or_media_white() {
    let original = include_bytes!("reference/parametric-rgb.icc");
    let reference = IccRgbProfile::parse(original).unwrap();
    let mut changed = original.to_vec();
    let chad = tag_offset(original, b"chad");
    for i in 0..9 {
        changed[chad + 8 + i * 4..chad + 12 + i * 4]
            .copy_from_slice(&(if i % 4 == 0 { 65536i32 } else { 0 }).to_be_bytes());
    }
    let white = tag_offset(original, b"wtpt");
    for i in 0..3 {
        changed[white + 8 + i * 4..white + 12 + i * 4].copy_from_slice(&65536i32.to_be_bytes());
    }
    assert_eq!(
        IccRgbProfile::parse(&changed).unwrap().rgb_definition(),
        reference.rgb_definition()
    );
    assert_eq!(
        reference.clone().into_rgb_definition(),
        *reference.rgb_definition()
    );
}
