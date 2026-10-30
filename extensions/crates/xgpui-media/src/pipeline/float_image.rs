//! Supplied SDR pixels, normalized to explicitly selected bounded linear storage.
use super::{AlphaMode, GamutMapping, ImageColor, ImagePrimaries};
use anyhow::{Result, ensure};
use gpui::ImageTextureData;
use half::f16;

/// Full RGBA16F mip payload size, checked before conversion/allocation.
pub fn rgba16f_payload_bytes(width: u32, height: u32) -> Result<usize> {
    let bytes = crate::mipmaps::payload_bytes(width, height)? * 2;
    ensure!(
        bytes <= ImageTextureData::MAX_BYTES,
        "RGBA16F mip chain exceeds 64 MiB"
    );
    Ok(bytes)
}

/// Convert explicitly described supplied pixels; no file codec or ICC inference.
/// For large images use MediaLoader::load_rgba16f to keep work off the UI thread.
/// Clips to bounded linear P3 before storage. Use `rgba16f_data_with_mapping`
/// to select a different policy. Adobe RGB colors outside P3 are not retained.
/// Mips average associated linear light, including fractional odd edge pixels.
pub fn rgba16f_data(
    width: u32,
    height: u32,
    pixels: Vec<[f32; 4]>,
    color: ImageColor,
) -> Result<ImageTextureData> {
    rgba16f_data_with_mapping(width, height, pixels, color, GamutMapping::Clip)
}
/// Convert explicitly described pixels to bounded associated linear P3 using
/// the selected mapping before generating mipmaps. This accepts Adobe RGB input
/// but does not preserve its out-of-P3 gamut or infer an embedded ICC profile.
pub fn rgba16f_data_with_mapping(
    width: u32,
    height: u32,
    pixels: Vec<[f32; 4]>,
    color: ImageColor,
    mapping: GamutMapping,
) -> Result<ImageTextureData> {
    build(width, height, pixels, color, mapping, || false)
}
pub(crate) fn build(
    width: u32,
    height: u32,
    pixels: Vec<[f32; 4]>,
    color: ImageColor,
    mapping: GamutMapping,
    cancelled: impl Fn() -> bool,
) -> Result<ImageTextureData> {
    build_in(
        width,
        height,
        pixels,
        color,
        mapping,
        ImagePrimaries::DisplayP3,
        cancelled,
    )
}

/// Preserve supplied Adobe RGB/P3 colors in bounded linear Rec.2020 SDR storage.
/// Mapping is explicit for sources outside Rec.2020; mips average associated light.
/// Native wide placement is unavailable until the compositor supports this format.
pub fn rec2020_rgba16f_data(
    width: u32,
    height: u32,
    pixels: Vec<[f32; 4]>,
    color: ImageColor,
    mapping: GamutMapping,
) -> Result<ImageTextureData> {
    build_in(
        width,
        height,
        pixels,
        color,
        mapping,
        ImagePrimaries::Rec2020,
        || false,
    )
}

pub(crate) fn build_in(
    width: u32,
    height: u32,
    pixels: Vec<[f32; 4]>,
    color: ImageColor,
    mapping: GamutMapping,
    working: ImagePrimaries,
    cancelled: impl Fn() -> bool,
) -> Result<ImageTextureData> {
    ensure!(
        matches!(working, ImagePrimaries::DisplayP3 | ImagePrimaries::Rec2020),
        "unsupported image storage primaries"
    );
    rgba16f_payload_bytes(width, height)?;
    ensure!(
        pixels.len() == width as usize * height as usize,
        "invalid supplied pixel count"
    );
    let canonical = ImageColor {
        alpha: AlphaMode::Premultiplied,
        primaries: working,
        ..ImageColor::LINEAR_P3
    };
    let mut input = Vec::with_capacity(pixels.len());
    for row in pixels.chunks(width as usize) {
        ensure!(!cancelled(), "image load cancelled");
        for p in row {
            input.push(
                color
                    .decode(p.map(f64::from), working)?
                    .output(canonical, mapping)?
                    .map(|v| v as f32),
            );
        }
    }
    drop(pixels);
    let mut levels = Vec::new();
    let (mut w, mut h) = (width, height);
    loop {
        ensure!(!cancelled(), "image load cancelled");
        let mut bytes = Vec::with_capacity(input.len() * 8);
        for p in &input {
            for c in p {
                bytes.extend_from_slice(
                    &f16::from_f32(if *c == 0. { 0. } else { *c }).to_le_bytes(),
                );
            }
        }
        levels.push(bytes);
        if w == 1 && h == 1 {
            break;
        }
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let mut next = vec![[0.; 4]; nw as usize * nh as usize];
        for y in 0..nh {
            ensure!(!cancelled(), "image load cancelled");
            for x in 0..nw {
                let (x0, x1) = (
                    x as f64 * w as f64 / nw as f64,
                    (x + 1) as f64 * w as f64 / nw as f64,
                );
                let (y0, y1) = (
                    y as f64 * h as f64 / nh as f64,
                    (y + 1) as f64 * h as f64 / nh as f64,
                );
                let mut sum = [0.; 4];
                for sy in y0.floor() as u32..(y1.ceil() as u32).min(h) {
                    for sx in x0.floor() as u32..(x1.ceil() as u32).min(w) {
                        let weight = (x1.min((sx + 1) as f64) - x0.max(sx as f64))
                            * (y1.min((sy + 1) as f64) - y0.max(sy as f64));
                        let p = input[(sy * w + sx) as usize];
                        for c in 0..4 {
                            sum[c] += p[c] as f64 * weight;
                        }
                    }
                }
                next[(y * nw + x) as usize] = sum.map(|v| (v / ((x1 - x0) * (y1 - y0))) as f32);
            }
        }
        input = next;
        w = nw;
        h = nh;
    }
    match working {
        ImagePrimaries::Rec2020 => {
            ImageTextureData::new_linear_rec2020_rgba16f(width, height, levels)
        }
        _ => ImageTextureData::new_linear_p3_rgba16f(width, height, levels),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    include!("adobe_fp16_reference.rs");
    fn pixel(bytes: &[u8]) -> [f32; 4] {
        std::array::from_fn(|i| f16::from_le_bytes([bytes[2 * i], bytes[2 * i + 1]]).to_f32())
    }
    #[test]
    fn precision_alpha_odd_edges_and_metadata() {
        let data = rgba16f_data(
            3,
            1,
            vec![[0.1, 0., 0., 1.], [0.1001, 0., 0., 1.], [0., 1., 0., 0.]],
            ImageColor::LINEAR_P3,
        )
        .unwrap();
        assert_ne!(
            pixel(&data.levels()[0])[0],
            pixel(&data.levels()[0][8..])[0]
        );
        let mip = pixel(&data.levels()[1]);
        assert!((mip[0] - (0.2001 / 3.)).abs() < 0.0001);
        assert!((mip[3] - 2. / 3.).abs() < 0.001);
        let encoded = rgba16f_data(1, 1, vec![[0.5, 0.5, 0.5, 0.5]], ImageColor::SRGB).unwrap();
        assert!((pixel(&encoded.levels()[0])[0] - 0.10702).abs() < 0.0002);
        assert_eq!(data.byte_len(), 32);
        assert_eq!(data.staging_byte_len(), 512);
    }
    #[test]
    fn adobe_input_maps_before_association_and_odd_mips() {
        let pixels = vec![[0., 1., 0., 0.5], [0., 1., 1., 1.], [1., 0., 0., 0.]];
        let data = rgba16f_data_with_mapping(
            3,
            1,
            pixels.clone(),
            ImageColor::ADOBE_RGB_1998,
            GamutMapping::Clip,
        )
        .unwrap();
        let first = pixel(&data.levels()[0]);
        let second = pixel(&data.levels()[0][8..]);
        // Independent A98->XYZ->P3 reference; negative red is clipped before
        // association. Cyan/green expose the part of Adobe RGB outside P3.
        let expected = [
            [0., 0.4767913506852906, 0.013252388163164934, 0.5],
            [0., 0.9535827013705814, 0.9761124052091603, 1.],
        ];
        for (actual, expected) in [first, second].into_iter().zip(expected) {
            for (a, e) in actual.into_iter().zip(expected) {
                let actual = f16::from_f32(a).to_bits();
                let reference = f16::from_f64(e).to_bits();
                assert!(
                    actual.abs_diff(reference) <= 1,
                    "FP16 result {a} differs by more than one representable step from {e}"
                );
                if e == 0. {
                    assert_eq!(actual, 0);
                }
            }
        }
        assert_eq!(pixel(&data.levels()[0][16..]), [0.; 4]);
        let mip = pixel(&data.levels()[1]);
        for i in 0..4 {
            let reference = f16::from_f64((expected[0][i] + expected[1][i]) / 3.).to_bits();
            assert!(f16::from_f32(mip[i]).to_bits().abs_diff(reference) <= 1);
        }
        let legacy = rgba16f_data(3, 1, pixels.clone(), ImageColor::ADOBE_RGB_1998).unwrap();
        assert_eq!(legacy.levels(), data.levels());
        let reduced = rgba16f_data_with_mapping(
            3,
            1,
            pixels,
            ImageColor::ADOBE_RGB_1998,
            GamutMapping::ReduceChroma,
        )
        .unwrap();
        assert_ne!(reduced.levels()[0], data.levels()[0]);
        assert_eq!(pixel(&reduced.levels()[0][16..]), [0.; 4]);
        for level in reduced.levels() {
            for bytes in level.chunks_exact(8) {
                let rgba = pixel(bytes);
                assert!(rgba.iter().all(|v| (0. ..=1.).contains(v)));
                assert!(rgba[..3].iter().all(|v| *v <= rgba[3]));
            }
        }
    }

    #[test]
    fn adobe_near_black_and_midtones_match_independent_fp16_bit_patterns() {
        // Decimal(120 digits) powers of the exact f32 inputs, then IEEE binary16
        // round-to-nearest-even via Python struct.pack("e"). These include
        // intentional FP16 underflow and the first subnormal storage values.
        for &(encoded, alpha, bits) in ADOBE_FP16_GRAY_REFERENCES {
            let data = rgba16f_data(
                1,
                1,
                vec![[encoded, encoded, encoded, alpha]],
                ImageColor::ADOBE_RGB_1998,
            )
            .unwrap();
            let stored = &data.levels()[0];
            for channel in 0..3 {
                let actual = u16::from_le_bytes([stored[channel * 2], stored[channel * 2 + 1]]);
                assert_eq!(
                    actual, bits,
                    "encoded={encoded}, alpha={alpha}, channel={channel}"
                );
            }
            assert_eq!(
                u16::from_le_bytes([stored[6], stored[7]]),
                f16::from_f32(alpha).to_bits()
            );
        }
    }

    #[test]
    fn rejects_invalid_pixels_and_budget() {
        for v in [f32::NAN, f32::INFINITY, -0.1, 1.1] {
            assert!(rgba16f_data(1, 1, vec![[v, 0., 0., 1.]], ImageColor::LINEAR_P3).is_err());
        }
        assert!(rgba16f_payload_bytes(4096, 4096).is_err());
        assert!(rgba16f_data(1, 1, vec![], ImageColor::LINEAR_P3).is_err());
        let premul = ImageColor {
            alpha: AlphaMode::Premultiplied,
            ..ImageColor::LINEAR_P3
        };
        assert!(rgba16f_data(1, 1, vec![[0.6, 0., 0., 0.5]], premul).is_err());
        assert!(ImageTextureData::new_linear_p3_rgba16f(1, 1, vec![vec![255; 8]]).is_err());
        assert!(build(1, 1, vec![[0.; 4]], premul, GamutMapping::Clip, || true).is_err());
    }
}

#[cfg(test)]
mod rec2020_tests {
    use super::*;
    include!("rec2020_reference.rs");

    #[test]
    fn source_gamuts_and_alpha_match_independent_rec2020_fp16_oracles() {
        for &(adobe, rgba, expected) in REC2020_REFERENCES {
            let color = if adobe {
                ImageColor::ADOBE_RGB_1998
            } else {
                ImageColor::LINEAR_P3
            };
            let data = rec2020_rgba16f_data(1, 1, vec![rgba], color, GamutMapping::Clip).unwrap();
            assert_eq!(
                data.format(),
                gpui::ImageTextureFormat::LinearRec2020Rgba16Float
            );
            for (i, e) in expected.into_iter().enumerate() {
                let actual =
                    u16::from_le_bytes([data.levels()[0][2 * i], data.levels()[0][2 * i + 1]]);
                // At most one binary16 step covers f64 -> f32 -> f16 double rounding.
                assert!(
                    actual.abs_diff(e) <= 1,
                    "adobe={adobe} {rgba:?}, channel={i}: {actual:x} != {e:x}"
                );
                if e == 0 {
                    assert_eq!(actual, 0);
                }
            }
        }
    }

    #[test]
    fn retains_out_of_p3_green_and_mips_in_rec2020() {
        let rgba = vec![[0., 1., 0., 0.5], [0., 1., 1., 1.], [1., 0., 0., 0.]];
        let data = rec2020_rgba16f_data(3, 1, rgba, ImageColor::ADOBE_RGB_1998, GamutMapping::Clip)
            .unwrap();
        // Source green remains outside P3 after storage instead of clipping red.
        let pixel: [f64; 4] = std::array::from_fn(|i| {
            f64::from(
                f16::from_le_bytes([data.levels()[0][2 * i], data.levels()[0][2 * i + 1]]).to_f32(),
            )
        });
        let color = ImageColor {
            alpha: AlphaMode::Premultiplied,
            ..ImageColor::LINEAR_REC2020
        };
        assert!(
            color
                .decode(pixel, ImagePrimaries::DisplayP3)
                .unwrap()
                .rgba()[0]
                < -0.05
        );
        assert_eq!(&data.levels()[0][16..], &[0; 8]);
        // 3 -> 1 averages all pixels; use independently generated source oracles.
        let mut sum = [0.; 4];
        for rgba in [[0., 1., 0., 0.5], [0., 1., 1., 1.]] {
            let &(_, _, bits) = REC2020_REFERENCES
                .iter()
                .find(|(a, p, _)| *a && *p == rgba)
                .unwrap_or_else(|| panic!("missing reference"));
            for i in 0..3 {
                sum[i] += f16::from_bits(bits[i]).to_f64() / 3.;
            }
            sum[3] += f64::from(rgba[3]) / 3.;
        }
        for (i, value) in sum.into_iter().enumerate() {
            let actual = u16::from_le_bytes([data.levels()[1][2 * i], data.levels()[1][2 * i + 1]]);
            assert!(actual.abs_diff(f16::from_f64(value).to_bits()) <= 1);
        }
    }

    #[test]
    fn validates_rec2020_storage_and_input_contracts() {
        for channel in [0x8000u16, 0xbc00, 0x3c01, 0x7c00, 0x7e00] {
            let mut bytes = vec![0; 8];
            bytes[0..2].copy_from_slice(&channel.to_le_bytes());
            bytes[6..8].copy_from_slice(&0x3c00u16.to_le_bytes());
            assert!(ImageTextureData::new_linear_rec2020_rgba16f(1, 1, vec![bytes]).is_err());
        }
        for value in [f32::NAN, f32::INFINITY, -0.1, 1.1] {
            assert!(
                rec2020_rgba16f_data(
                    1,
                    1,
                    vec![[value, 0., 0., 1.]],
                    ImageColor::LINEAR_REC2020,
                    GamutMapping::Clip
                )
                .is_err()
            );
        }
        assert!(
            build_in(
                1,
                1,
                vec![[0.; 4]],
                ImageColor::LINEAR_REC2020,
                GamutMapping::Clip,
                ImagePrimaries::Rec2020,
                || true
            )
            .is_err()
        );
        assert!(ImageTextureData::new_linear_rec2020_rgba16f(3, 1, vec![vec![0; 24]]).is_err());
    }
}
