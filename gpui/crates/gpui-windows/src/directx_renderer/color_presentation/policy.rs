//! Portable policy math; executable with `rustc --test` without a Windows runtime.
pub(crate) fn white_scale(level: u32) -> Option<f32> {
    let scale = level as f32 / 1000.;
    // Keep bounded P3 SDR (maximum scRGB channel <1.225) finite in FP16.
    (level > 0 && scale <= 65504. / 1.225).then_some(scale)
}
pub(crate) fn overlap(a: [i32; 4], b: [i32; 4]) -> u64 {
    let w = (i64::from(a[2].min(b[2])) - i64::from(a[0].max(b[0]))).max(0) as u64;
    let h = (i64::from(a[3].min(b[3])) - i64::from(a[1].max(b[1]))).max(0) as u64;
    w * h // (2^32-1)^2 fits u64, even for extreme desktop coordinates.
}
pub(crate) fn same_device_name(a: &[u16; 32], b: &[u16; 32]) -> bool {
    let trim = |v: &[u16; 32]| {
        String::from_utf16_lossy(&v[..v.iter().position(|c| *c == 0).unwrap_or(v.len())])
    };
    trim(a).eq_ignore_ascii_case(&trim(b))
}
#[test]
fn sdr_white_is_relative_to_eighty_nits_and_stays_finite_in_fp16() {
    assert_eq!(white_scale(1000), Some(1.));
    assert_eq!(white_scale(2500), Some(2.5));
    assert_eq!(white_scale(0), None);
    assert_eq!(white_scale(u32::MAX), None);
}
#[test]
fn output_intersection_handles_negative_and_extreme_coordinates() {
    assert_eq!(overlap([-100, 0, 300, 200], [0, 0, 500, 500]), 60000);
    assert_eq!(overlap([-100, 0, 300, 200], [400, 0, 500, 500]), 0);
    let full = [i32::MIN, i32::MIN, i32::MAX, i32::MAX];
    assert_eq!(overlap(full, full), u64::from(u32::MAX).pow(2));
}
#[test]
fn names_compare_terminated_content_not_unused_storage() {
    let mut a = [123u16; 32];
    let mut b = [456u16; 32];
    for (i, c) in "\\\\.\\DISPLAY1\0".encode_utf16().enumerate() {
        a[i] = c;
    }
    for (i, c) in "\\\\.\\display1\0".encode_utf16().enumerate() {
        b[i] = c;
    }
    assert!(same_device_name(&a, &b));
    b[0] = 0;
    assert!(!same_device_name(&a, &b));
}
