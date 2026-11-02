use gpui::{LinearImagePaint, content::*};
fn image(color: ColorEncoding) -> ImageDescriptor {
    ImageDescriptor::new(
        ImageExtent::new(8, 4).unwrap(),
        if color == ColorEncoding::Srgb {
            PixelFormat::Rgba8
        } else {
            PixelFormat::Rgba16Float
        },
        ImageInterpretation {
            color,
            alpha: AlphaMode::Straight,
        },
        1,
    )
    .unwrap()
}
#[test]
fn checked_transforms_preserve_domain_and_reject_nonfinite_or_overflowing_rows() {
    let rows = [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]];
    assert_eq!(ImageColorTransform::default().shader_flags(), [0., 0.]);
    let mut invalid = rows;
    invalid[0][0] = f32::NAN;
    assert!(ImageColorTransform::encoded_srgb(invalid).is_err());
    invalid[0][0] = f32::MAX;
    assert!(ImageColorTransform::encoded_srgb(invalid).is_err());
    let encoded = ImageColorTransform::encoded_srgb([[2., 0., 0., 0.], rows[1], rows[2]]).unwrap();
    assert!(encoded.validate(&image(ColorEncoding::Srgb), false).is_ok());
    assert!(
        encoded
            .validate(&image(ColorEncoding::LinearP3), false)
            .is_err()
    );
    let linear = ImageColorTransform::linear(
        LinearImagePaint::new(rows, [0.5, 0.2, 0.8, 1.])
            .unwrap()
            .with_p3(true),
    );
    assert_eq!(linear.shader_flags(), [1., 2.]);
    assert!(linear.validate(&image(ColorEncoding::Srgb), false).is_ok());
    assert!(
        linear
            .validate(&image(ColorEncoding::LinearRec2020), true)
            .is_ok()
    );
    let large = ImageColorTransform::linear(
        LinearImagePaint::new([[5., 0., 0., 0.], rows[1], rows[2]], [0., 0., 1., 1.]).unwrap(),
    );
    assert!(large.validate(&image(ColorEncoding::Srgb), false).is_ok());
    assert!(
        large
            .validate(&image(ColorEncoding::LinearP3), true)
            .is_err()
    );
}
