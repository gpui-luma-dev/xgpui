use anyhow::{Result, ensure};
use gpui::ImageTextureData;

pub(crate) fn payload_bytes(mut w: u32, mut h: u32) -> Result<usize> {
    ensure!(
        w > 0 && h > 0 && w <= 16384 && h <= 16384,
        "invalid image dimensions"
    );
    let mut bytes = 0usize;
    loop {
        bytes += w as usize * h as usize * 4;
        ensure!(
            bytes <= ImageTextureData::MAX_BYTES,
            "image mip chain exceeds 64 MiB"
        );
        if w == 1 && h == 1 {
            return Ok(bytes);
        }
        w = (w / 2).max(1);
        h = (h / 2).max(1);
    }
}
fn linear(v: u8) -> f32 {
    let v = v as f32 / 255.;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}
fn encoded(v: f32) -> u8 {
    let v = if v <= 0.0031308 {
        v * 12.92
    } else {
        1.055 * v.powf(1. / 2.4) - 0.055
    };
    (v.clamp(0., 1.) * 255.).round() as u8
}
/// Area filtering in linear light with associated alpha, converted back to the
/// compositor's straight encoded-sRGB contract. Odd edge pixels are included.
pub(crate) fn build(
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    cancelled: impl Fn() -> bool,
) -> Result<ImageTextureData> {
    payload_bytes(width, height)?;
    ensure!(
        rgba.len() == width as usize * height as usize * 4,
        "invalid RGBA byte count"
    );
    let decode: [f32; 256] = std::array::from_fn(|value| linear(value as u8));
    let mut levels = vec![rgba];
    let (mut w, mut h) = (width, height);
    while w > 1 || h > 1 {
        ensure!(!cancelled(), "image load cancelled");
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let mut out = vec![0; nw as usize * nh as usize * 4];
        let input = levels.last().unwrap();
        for y in 0..nh {
            ensure!(!cancelled(), "image load cancelled");
            for x in 0..nw {
                // Integrate fractional source-pixel overlap for odd extents.
                let (x0, x1) = (
                    x as f32 * w as f32 / nw as f32,
                    (x + 1) as f32 * w as f32 / nw as f32,
                );
                let (y0, y1) = (
                    y as f32 * h as f32 / nh as f32,
                    (y + 1) as f32 * h as f32 / nh as f32,
                );
                let mut sum = [0f32; 4];
                for sy in y0.floor() as u32..(y1.ceil() as u32).min(h) {
                    for sx in x0.floor() as u32..(x1.ceil() as u32).min(w) {
                        let weight = (x1.min((sx + 1) as f32) - x0.max(sx as f32))
                            * (y1.min((sy + 1) as f32) - y0.max(sy as f32));
                        let p = &input[((sy * w + sx) * 4) as usize..][..4];
                        let a = p[3] as f32 / 255.;
                        for c in 0..3 {
                            sum[c] += decode[p[c] as usize] * a * weight;
                        }
                        sum[3] += a * weight;
                    }
                }
                let p = &mut out[((y * nw + x) * 4) as usize..][..4];
                for c in 0..3 {
                    p[c] = if sum[3] > 0. {
                        encoded(sum[c] / sum[3])
                    } else {
                        0
                    };
                }
                p[3] = (sum[3] / ((x1 - x0) * (y1 - y0)) * 255.).round() as u8;
            }
        }
        levels.push(out);
        w = nw;
        h = nh;
    }
    ImageTextureData::new(width, height, levels)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filters_linear_light_and_excludes_transparent_color() {
        let data = build(2, 1, vec![0, 0, 0, 255, 255, 255, 255, 255], || false).unwrap();
        assert!((data.levels()[1][0] as i32 - 188).abs() <= 1);
        let data = build(2, 1, vec![255, 0, 0, 255, 0, 0, 255, 0], || false).unwrap();
        assert_eq!(data.levels()[1], [255, 0, 0, 128]);
    }
    #[test]
    fn odd_edges_and_cancellation() {
        let data = build(
            3,
            1,
            vec![0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255, 255],
            || false,
        )
        .unwrap();
        assert!(data.levels()[1][0] > 150);
        assert!(build(2, 1, vec![0; 8], || true).is_err());
        assert!(build(1, 1, vec![], || false).is_err());
        assert!(payload_bytes(16384, 16384).is_err());
    }
}
