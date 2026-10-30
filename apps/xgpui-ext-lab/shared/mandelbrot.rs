//! Shared native/browser f64 escape-time and palette math. No platform dependencies.
pub(crate) fn iterations(zoom: f64) -> u32 {
    (256. + zoom.log2().max(0.) * 48.).min(2048.) as u32
}

pub(crate) fn escape(cx: f64, cy: f64, limit: u32) -> u32 {
    // Analytic interior tests avoid the most expensive common pixels.
    let q = (cx - 0.25) * (cx - 0.25) + cy * cy;
    if q * (q + cx - 0.25) <= 0.25 * cy * cy || (cx + 1.) * (cx + 1.) + cy * cy <= 0.0625 {
        return limit;
    }
    let (mut x, mut y) = (0., 0.);
    for i in 0..limit {
        if x * x + y * y > 4. {
            return i;
        }
        (x, y) = (x * x - y * y + cx, 2. * x * y + cy);
    }
    limit
}
pub(crate) fn bgra(real: f64, imaginary: f64, limit: u32) -> [u8; 4] {
    let n = escape(real, imaginary, limit);
    let rgb = if n == limit {
        [5, 8, 16]
    } else {
        let t = n as f64 * 0.11;
        [
            ((t.sin() * 0.5 + 0.5) * 255.) as u8,
            (((t + 2.).sin() * 0.5 + 0.5) * 255.) as u8,
            (((t + 4.).sin() * 0.5 + 0.5) * 255.) as u8,
        ]
    };
    [rgb[2], rgb[1], rgb[0], 255]
}
