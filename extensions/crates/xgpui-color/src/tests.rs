use super::*;
const SPACES: [RgbSpace; 4] = [
    RgbSpace::Srgb,
    RgbSpace::DisplayP3,
    RgbSpace::Rec2020,
    RgbSpace::AdobeRgb1998,
];
fn close(a: [f64; 3], b: [f64; 3], eps: f64) {
    for (a, b) in a.into_iter().zip(b) {
        assert!((a - b).abs() <= eps, "{a} != {b} (tolerance {eps})");
    }
}
#[test]
fn reference_white_and_primary_vectors() {
    for space in SPACES {
        close(
            LinearRgb::new(space, [1.; 3])
                .unwrap()
                .to_xyz()
                .unwrap()
                .components(),
            [0.9504559270516716, 1., 1.0890577507598784],
            1e-14,
        );
    }
    close(
        LinearRgb::new(RgbSpace::Srgb, [1., 0., 0.])
            .unwrap()
            .to_xyz()
            .unwrap()
            .components(),
        [0.4123907992659595, 0.2126390058715104, 0.01933081871559185],
        1e-14,
    );
    close(
        LinearRgb::new(RgbSpace::DisplayP3, [1., 0., 0.])
            .unwrap()
            .convert(RgbSpace::Srgb)
            .unwrap()
            .components(),
        [1.2249401762805598, -0.0420569547096882, -0.0196375545903344],
        1e-13,
    );
    close(
        LinearRgb::new(RgbSpace::Rec2020, [0., 1., 0.])
            .unwrap()
            .to_xyz()
            .unwrap()
            .components(),
        [
            0.14461690358620832,
            0.6779980715188708,
            0.028072693049087428,
        ],
        1e-14,
    );
}
#[test]
fn primary_and_xyz_round_trips_preserve_extended_values() {
    for src in SPACES {
        for dst in SPACES {
            for v in [[0.; 3], [1.; 3], [0.2, 0.4, 0.7], [-0.2, 1.3, 0.1]] {
                let rgb = LinearRgb::new(src, v).unwrap();
                close(
                    rgb.convert(dst).unwrap().convert(src).unwrap().components(),
                    v,
                    2e-14,
                );
                close(
                    rgb.to_xyz().unwrap().to_linear(src).unwrap().components(),
                    v,
                    2e-14,
                );
            }
        }
    }
}
#[test]
fn transfers_have_reference_values_and_sign_symmetry() {
    close(
        EncodedRgb::new(RgbSpace::Srgb, [0.04045, 0.5, 1.])
            .unwrap()
            .decode()
            .unwrap()
            .components(),
        [0.0031308049535603713, 0.21404114048223255, 1.],
        1e-14,
    );
    for space in SPACES {
        for x in [
            0.,
            0.0031308,
            0.018053968510807,
            0.04045,
            0.0812428582986315,
            0.5,
            1.,
            1.5,
        ] {
            let pos = EncodedRgb::new(space, [x; 3]).unwrap().decode().unwrap();
            let neg = EncodedRgb::new(space, [-x; 3]).unwrap().decode().unwrap();
            close(neg.components(), pos.components().map(|c| -c), 1e-14);
            // sRGB's published rounded branch constants introduce a tiny discontinuity.
            close(pos.encode().unwrap().components(), [x; 3], 4e-8);
            close(
                LinearRgb::new(space, [x; 3])
                    .unwrap()
                    .encode()
                    .unwrap()
                    .decode()
                    .unwrap()
                    .components(),
                [x; 3],
                4e-8,
            );
        }
    }
}
#[test]
fn oklab_reference_and_extended_round_trips() {
    close(
        LinearRgb::new(RgbSpace::Srgb, [1., 0., 0.])
            .unwrap()
            .to_oklab()
            .unwrap()
            .components(),
        [0.6279553606145516, 0.22486306106597398, 0.1258462985307351],
        1e-12,
    );
    for space in SPACES {
        for v in [[0.; 3], [1.; 3], [0.2, 0.4, 0.7], [-0.2, 1.3, 0.1]] {
            let lab = LinearRgb::new(space, v).unwrap().to_oklab().unwrap();
            close(lab.to_linear(space).unwrap().components(), v, 5e-7);
            close(
                lab.to_oklch().unwrap().to_oklab().unwrap().components(),
                lab.components(),
                1e-14,
            );
        }
    }
    assert_eq!(
        Oklch::new(0.5, 0., 271.).unwrap().components(),
        [0.5, 0., 0.]
    );
    assert_eq!(
        Oklch::new(0.5, 0.2, -30.).unwrap().components(),
        [0.5, 0.2, 330.]
    );
}
#[test]
fn rejects_invalid_inputs_and_arithmetic_overflow() {
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for i in 0..3 {
            let mut v = [0.; 3];
            v[i] = bad;
            assert!(EncodedRgb::new(RgbSpace::Srgb, v).is_err());
            assert!(LinearRgb::new(RgbSpace::Srgb, v).is_err());
            assert!(XyzD65::new(v).is_err());
            assert!(Oklab::new(v).is_err());
            assert!(Oklch::new(v[0], v[1], v[2]).is_err());
        }
    }
    assert!(Oklch::new(0.5, -0.1, 0.).is_err());
    assert!(
        EncodedRgb::new(RgbSpace::Srgb, [f64::MAX; 3])
            .unwrap()
            .decode()
            .is_err()
    );
    assert!(
        Oklab::new([f64::MAX; 3])
            .unwrap()
            .to_linear(RgbSpace::Srgb)
            .is_err()
    );
}

#[test]
fn tiny_negative_hue_stays_in_half_open_range() {
    let hue = Oklch::new(0.5, 0.2, -f64::EPSILON).unwrap().components()[2];
    assert!((0.0..360.0).contains(&hue));
}

#[test]
fn adobe_rgb_reference_primaries_and_transfer() {
    // Reference XYZ columns from CSS Color 4's rational A98 matrix.
    for (rgb, xyz) in [
        (
            [1., 0., 0.],
            [0.5766690429101305, 0.29734497525053605, 0.02703136138641234],
        ),
        (
            [0., 1., 0.],
            [0.1855582379065463, 0.6273635662554661, 0.07068885253582723],
        ),
        (
            [0., 0., 1.],
            [0.1882286462349947, 0.07529145849399788, 0.9913375368376388],
        ),
    ] {
        close(
            LinearRgb::new(RgbSpace::AdobeRgb1998, rgb)
                .unwrap()
                .to_xyz()
                .unwrap()
                .components(),
            xyz,
            1e-14,
        );
    }
    // High precision power-law reference values, not encode/decode round trips.
    for (encoded, linear) in [
        (0., 0.),
        (0.003, 0.0000028290313452536574),
        (0.18, 0.023_024_029_316_635_19),
        (0.5, 0.21775552814439456),
        (1., 1.),
        (1.25, 1.6335270018684131),
    ] {
        for sign in [-1., 1.] {
            close(
                EncodedRgb::new(RgbSpace::AdobeRgb1998, [encoded * sign; 3])
                    .unwrap()
                    .decode()
                    .unwrap()
                    .components(),
                [linear * sign; 3],
                1e-14,
            );
        }
    }
    close(
        LinearRgb::new(RgbSpace::AdobeRgb1998, [0.18, 0.5, 1.25])
            .unwrap()
            .encode()
            .unwrap()
            .components(),
        [0.4585294656798946, 0.7296583817678015, 1.106791089860401],
        1e-14,
    );
}

#[test]
fn adobe_and_p3_cross_gamut_colors_remain_extended_until_mapping() {
    for value in [[0., 1., 0.], [0., 1., 1.]] {
        let adobe = LinearRgb::new(RgbSpace::AdobeRgb1998, value).unwrap();
        let p3 = adobe.convert(RgbSpace::DisplayP3).unwrap().components();
        assert!(p3.iter().any(|v| !(0. ..=1.).contains(v)), "{p3:?}");
        close(
            adobe
                .convert(RgbSpace::DisplayP3)
                .unwrap()
                .convert(RgbSpace::AdobeRgb1998)
                .unwrap()
                .components(),
            value,
            1e-14,
        );
    }
    let p3_red = LinearRgb::new(RgbSpace::DisplayP3, [1., 0., 0.]).unwrap();
    let adobe = p3_red.convert(RgbSpace::AdobeRgb1998).unwrap().components();
    assert!(adobe.iter().any(|v| !(0. ..=1.).contains(v)), "{adobe:?}");
}
