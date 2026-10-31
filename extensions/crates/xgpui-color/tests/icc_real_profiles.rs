#![cfg(feature = "icc")]
use xgpui_color::icc::{IccError, IccRgbProfile};

const ACCEPTED: &[&[u8]] = &[
    include_bytes!("reference/real-icc/sRGB-v4.icc"),
    include_bytes!("reference/real-icc/DisplayP3-v4.icc"),
    include_bytes!("reference/real-icc/AdobeCompat-v4.icc"),
    include_bytes!("reference/real-icc/sRGB2014.icc"),
];
#[test]
fn real_profiles_accept_matrix_trc_and_reject_other_models_and_overlaps() {
    for (i, bytes) in ACCEPTED.iter().enumerate() {
        IccRgbProfile::parse(bytes).unwrap_or_else(|e| panic!("profile {i}: {e}"));
    }
    for bytes in [
        include_bytes!("reference/real-icc/sGrey-v4.icc").as_slice(),
        include_bytes!("reference/real-icc/CGATS001Compat-v2-micro.icc"),
    ] {
        assert_eq!(
            IccRgbProfile::parse(bytes).unwrap_err(),
            IccError::Unsupported
        );
    }
    // Compact v2 intentionally packs other tags into desc's declared range.
    // Preserve strict overlap rejection; do not call these upstream files corrupt.
    for bytes in [
        include_bytes!("reference/real-icc/sRGB-v2-magic.icc").as_slice(),
        include_bytes!("reference/real-icc/DisplayP3-v2-magic.icc"),
        include_bytes!("reference/real-icc/AdobeCompat-v2.icc"),
    ] {
        assert_eq!(
            IccRgbProfile::parse(bytes).unwrap_err(),
            IccError::Malformed
        );
    }
}

#[allow(clippy::type_complexity)]
mod reference {
    include!("reference/real_icc.rs");
}

#[test]
fn real_profiles_match_exact_tag_oracles_without_named_substitution() {
    let profiles: Vec<_> = ACCEPTED
        .iter()
        .map(|bytes| IccRgbProfile::parse(bytes).unwrap())
        .collect();
    for &(index, rgb, decoded, pcs, xyz) in reference::CASES {
        let p = &profiles[index];
        for c in 0..3 {
            let actual = p.rgb_definition().curves()[c].decode(rgb[c]).unwrap();
            assert!(
                (actual - decoded[c]).abs()
                    <= 16. * f64::EPSILON * decoded[c].abs() + 4. * f64::from_bits(1),
                "profile {index} {rgb:?} curve {c}: {actual:e} vs {:e}",
                decoded[c]
            );
        }
        let energy: f64 = decoded.into_iter().map(f64::abs).sum();
        for (actual, expected) in [
            (p.decode_to_pcs(rgb).unwrap(), pcs),
            (
                p.rgb_definition()
                    .decode_to_xyz_d65(rgb)
                    .unwrap()
                    .components(),
                xyz,
            ),
        ] {
            for c in 0..3 {
                assert!(
                    (actual[c] - expected[c]).abs()
                        <= 64. * f64::EPSILON * energy + 4. * f64::from_bits(1),
                    "profile {index} {rgb:?} channel {c}: {:e} vs {:e}",
                    actual[c],
                    expected[c]
                );
            }
        }
    }
}

#[test]
fn srgb2014_preserves_non_neutral_matrix_and_bounds_informational_tags() {
    let bytes = include_bytes!("reference/real-icc/sRGB2014.icc");
    let profile = IccRgbProfile::parse(bytes).unwrap();
    let white = profile.decode_to_pcs([1.; 3]).unwrap();
    // Exact serialized column sums; do not normalize to the PCS white.
    assert_eq!(white, [63195. / 65536., 65534. / 65536., 54073. / 65536.]);
    assert_eq!(profile.pcs_white(), [63190. / 65536., 1., 54061. / 65536.]);
    let actual = profile
        .rgb_definition()
        .decode_to_xyz_d65([1.; 3])
        .unwrap()
        .components();
    assert!((actual[2] - xgpui_color::WhitePoint::D65.xyz()[2]).abs() > 1e-4);
    // Neutral analytic-space construction keeps its existing admission contract.
    assert!(
        xgpui_color::RgbDefinition::new_with_xyz_white(
            profile.colorants(),
            profile.rgb_definition().curves().clone(),
            profile.pcs_white()
        )
        .is_err()
    );
    let mut oversized = bytes.to_vec();
    let count = u32::from_be_bytes(bytes[128..132].try_into().unwrap()) as usize;
    let at = (0..count)
        .map(|i| 132 + i * 12)
        .find(|&i| &bytes[i..i + 4] == b"vued")
        .unwrap();
    let offset = oversized.len().next_multiple_of(4);
    oversized.resize(offset + 4100, 0);
    oversized[at + 4..at + 8].copy_from_slice(&(offset as u32).to_be_bytes());
    oversized[at + 8..at + 12].copy_from_slice(&4100u32.to_be_bytes());
    let length = oversized.len() as u32;
    oversized[..4].copy_from_slice(&length.to_be_bytes());
    assert_eq!(
        IccRgbProfile::parse(&oversized).unwrap_err(),
        IccError::Limit
    );
    let mut transformative = bytes.to_vec();
    transformative[at..at + 4].copy_from_slice(b"A2B0");
    assert_eq!(
        IccRgbProfile::parse(&transformative).unwrap_err(),
        IccError::Unsupported
    );
}
