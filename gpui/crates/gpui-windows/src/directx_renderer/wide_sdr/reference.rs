//! Independent CPU oracle for WARP pixel tests; also runnable with rustc --test
//! on non-Windows hosts. Production shaders never call these functions.
pub(crate) fn decode(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}
pub(crate) fn p3(c: [f32; 4]) -> [f32; 4] {
    [
        1.224_940_2 * c[0] - 0.22494018 * c[1],
        -0.04205695 * c[0] + 1.042_056_9 * c[1],
        -0.01963755 * c[0] - 0.07863605 * c[1] + 1.098_273_6 * c[2],
        c[3],
    ]
}
pub(crate) fn ui(c: [f32; 4]) -> [f32; 4] {
    if c[3] == 0. {
        return [0.; 4];
    }
    [
        decode(c[0] / c[3]) * c[3],
        decode(c[1] / c[3]) * c[3],
        decode(c[2] / c[3]) * c[3],
        c[3],
    ]
}
pub(crate) fn over(src: [f32; 4], dst: [f32; 4]) -> [f32; 4] {
    std::array::from_fn(|i| src[i] + dst[i] * (1. - src[3]))
}
pub(crate) fn half(v: u16) -> f32 {
    let sign = if v & 0x8000 == 0 { 1. } else { -1. };
    let e = (v >> 10) & 31;
    let m = (v & 1023) as f32;
    if e == 0 {
        sign * m * 2f32.powi(-24)
    } else if e == 31 {
        if m == 0. {
            sign * f32::INFINITY
        } else {
            f32::NAN
        }
    } else {
        sign * (1. + m / 1024.) * 2f32.powi(e as i32 - 15)
    }
}
#[test]
fn p3_primaries_preserve_extended_channels_and_white() {
    let red = p3([1., 0., 0., 1.]);
    assert!(red[0] > 1.22 && red[1] < -0.04 && red[2] < -0.019);
    let white = p3([1.; 4]);
    assert!(white.iter().all(|v| (v - 1.).abs() < 0.00001));
}
#[test]
fn srgb_transfer_and_low_alpha() {
    assert!((decode(0.5) - 0.21404114).abs() < 1e-6);
    assert!((decode(0.04045) - 0.003130805).abs() < 1e-7);
    assert_eq!(ui([0.; 4]), [0.; 4]);
    assert!((ui([0.00001, 0., 0., 0.00001])[0] - 0.00001).abs() < 1e-9);
}
#[test]
fn associated_alpha_and_order() {
    let red = p3([0.5, 0., 0., 0.5]);
    let blue = p3([0., 0., 0.5, 0.5]);
    assert!((over(red, blue)[3] - 0.75).abs() < 1e-6);
    assert_ne!(over(red, blue), over(blue, red));
    assert_eq!(over([0.; 4], red), red);
}
#[test]
fn half_decode_preserves_sign_and_small_alpha() {
    assert_eq!(half(0x3c00), 1.);
    assert_eq!(half(0xbc00), -1.);
    assert_eq!(half(0), 0.);
    assert!(half(1) > 0. && half(1) < 1e-6);
    assert!(half(0x7c00).is_infinite());
}
