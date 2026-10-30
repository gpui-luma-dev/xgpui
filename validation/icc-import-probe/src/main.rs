use moxcms::{ColorProfile, Layout, RenderingIntent, TransformOptions, curve_from_gamma};
fn main() {
    let source = ColorProfile::new_adobe_rgb();
    let mut destination = ColorProfile::new_display_p3();
    let linear = curve_from_gamma(1.);
    destination.red_trc = Some(linear.clone());
    destination.green_trc = Some(linear.clone());
    destination.blue_trc = Some(linear);
    let transform = source
        .create_transform_f64(
            Layout::Rgb,
            &destination,
            Layout::Rgb,
            TransformOptions {
                rendering_intent: RenderingIntent::RelativeColorimetric,
                prefer_fixed_point: false,
                allow_use_cicp_transfer: false,
                allow_extended_range_rgb_xyz: std::env::args().any(|arg| arg == "--extended"),
                ..Default::default()
            },
        )
        .unwrap();
    let mut out = [0.; 9];
    transform
        .transform(&[0., 1., 0., 0., 1., 1., 1., 1., 1.], &mut out)
        .unwrap();
    println!("Adobe green/cyan/white → linear P3: {out:?}");
    assert!(
        out[0] < 0. && out[2] < 0.,
        "Adobe green must retain negative P3 channels"
    );
    assert!(out[3] < 0., "Adobe cyan must retain negative P3 red");
    assert!(out[6..].iter().all(|x| (x - 1.).abs() < 0.0001));
}
