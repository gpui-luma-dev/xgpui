use xgpui_color::{
    ColorInterpolation, EncodedRgb, GamutMapping, LinearGradient, LinearRgb, LinearRgba, RgbSpace,
    WGSL_COLOR_MATH,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input = [0.2, 0.5, 0.8];
    let encoded = EncodedRgb::new(RgbSpace::Srgb, input)?;
    let p3 = encoded.decode()?.convert(RgbSpace::DisplayP3)?;
    let round_trip = p3.convert(RgbSpace::Srgb)?.encode()?.components();
    for (actual, expected) in round_trip.into_iter().zip(input) {
        assert!((actual - expected).abs() < 1e-10);
    }
    let perceptual = p3.to_oklab()?.to_oklch()?;
    assert!(
        perceptual
            .components()
            .iter()
            .all(|value| value.is_finite())
    );

    let red = LinearRgba::new(LinearRgb::new(RgbSpace::DisplayP3, [1., 0., 0.])?, 1.)?;
    let blue = LinearRgba::new(LinearRgb::new(RgbSpace::DisplayP3, [0., 0., 1.])?, 0.5)?;
    let gradient = LinearGradient::new(
        [0., 0.],
        [320., 0.],
        [red, blue],
        RgbSpace::DisplayP3,
        ColorInterpolation::Oklab,
    )?;
    let output = gradient
        .sample([160., 0.])?
        .map_gamut(RgbSpace::DisplayP3, GamutMapping::ReduceChroma)?
        .premultiplied();
    assert!(output.into_iter().all(|value| value.is_finite()));
    assert!(EncodedRgb::new(RgbSpace::Srgb, [f64::NAN, 0., 0.]).is_err());

    let adobe = EncodedRgb::new(RgbSpace::AdobeRgb1998, [0., 1., 1.])?.decode()?;
    let p3 = adobe.convert(RgbSpace::DisplayP3)?.components();
    assert!(p3[0] < 0.);
    assert!(
        adobe
            .map_gamut(RgbSpace::DisplayP3, GamutMapping::ReduceChroma)?
            .components()
            .into_iter()
            .all(|v| (0. ..=1.).contains(&v))
    );
    let gray = EncodedRgb::new(RgbSpace::AdobeRgb1998, [0.5; 3])?
        .decode()?
        .components();
    assert!((gray[0] - 0.21775552814439456).abs() < 1e-14);

    // Read the archived resource through the public API, with no source-tree path.
    assert!(WGSL_COLOR_MATH.contains("fn gamut_decode"));
    assert!(WGSL_COLOR_MATH.contains("fn gamut_interpolate"));
    println!("Consumer passed: conversions, gradients, gamut mapping and embedded WGSL.");
    Ok(())
}
