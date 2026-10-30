//! Shared Rust calculations for task-focused browser demos; no browser/GPU API.
use std::sync::Arc;
use xgpui_color::{EncodedRgb, GamutMapping, LinearRgb, RgbSpace, XyzD65, icc::IccRgbProfile};
use xgpui_media::pipeline::{self, AlphaMode, ImageColor, ImagePrimaries, PixelColor};

pub const SPACES: [RgbSpace; 4] = [
    RgbSpace::Srgb,
    RgbSpace::DisplayP3,
    RgbSpace::AdobeRgb1998,
    RgbSpace::Rec2020,
];
pub const NAMES: [&str; 8] = [
    "sRGB",
    "Display P3",
    "Adobe RGB (1998)",
    "Rec.2020",
    "ICC sRGB v4",
    "ICC P3 v4",
    "ICC Adobe-compatible v4",
    "ICC sRGB2014 sampled",
];
pub const PATCHES: [(&str, [f32; 3]); 5] = [
    ("Green", [0., 1., 0.]),
    ("Cyan", [0., 1., 1.]),
    ("Mid gray", [0.18; 3]),
    ("Near black", [0.0001; 3]),
    ("White", [1.; 3]),
];
pub const LUMINANCE: [f64; 3] = [0.05, 0.18, 0.5];
pub const COLORS: [u32; 4] = [0x65d7ff, 0xff9a62, 0xc4a0ff, 0x91df86];

pub fn xy(xyz: [f64; 3]) -> Option<[f64; 2]> {
    let sum: f64 = xyz.into_iter().sum();
    (sum.is_finite() && sum > 0.).then(|| [xyz[0] / sum, xyz[1] / sum])
}
pub fn locus() -> Vec<[f64; 2]> {
    include_str!("../assets/colorimetry/CIE_cc_1931_2deg.csv")
        .lines()
        .map(|line| {
            let v: Vec<f64> = line
                .split(',')
                .map(|v| v.parse().expect("bundled CIE datum"))
                .collect();
            xy([v[1], v[2], v[3]]).expect("nonzero spectral coordinate")
        })
        .collect()
}
fn contains(points: &[[f64; 2]], p: [f64; 2]) -> bool {
    let mut inside = false;
    let mut previous = points[points.len() - 1];
    for &current in points {
        if (current[1] > p[1]) != (previous[1] > p[1])
            && p[0]
                < (previous[0] - current[0]) * (p[1] - current[1]) / (previous[1] - current[1])
                    + current[0]
        {
            inside = !inside;
        }
        previous = current;
    }
    inside
}
pub struct Cell {
    pub xy: [f64; 2],
    pub rgb: [f32; 3],
    pub outside: bool,
}
pub fn cells(
    space: RgbSpace,
    luminance: f64,
    mapping: GamutMapping,
    boundary: &[[f64; 2]],
) -> anyhow::Result<Vec<Cell>> {
    let mut cells = Vec::new();
    for row in 0..54 {
        for col in 0..48 {
            let p = [
                (col as f64 + 0.5) * 0.8 / 48.,
                (row as f64 + 0.5) * 0.9 / 54.,
            ];
            if !contains(boundary, p) {
                continue;
            }
            let xyz = XyzD65::new([
                p[0] * luminance / p[1],
                luminance,
                (1. - p[0] - p[1]) * luminance / p[1],
            ])?;
            let rgb = xyz.to_linear(space)?;
            let outside = rgb
                .components()
                .into_iter()
                .any(|v| !(-1e-12..=1. + 1e-12).contains(&v));
            let simulated = rgb.map_gamut(space, mapping)?;
            // Browser route is deliberately sRGB, even when simulating a larger gamut.
            let preview = simulated
                .map_gamut(RgbSpace::Srgb, GamutMapping::Clip)?
                .encode()?
                .components()
                .map(|v| v as f32);
            cells.push(Cell {
                xy: [col as f64 * 0.8 / 48., row as f64 * 0.9 / 54.],
                rgb: preview,
                outside,
            });
        }
    }
    Ok(cells)
}
fn primaries(space: RgbSpace) -> ImagePrimaries {
    match space {
        RgbSpace::Srgb => ImagePrimaries::Srgb,
        RgbSpace::DisplayP3 => ImagePrimaries::DisplayP3,
        RgbSpace::AdobeRgb1998 => ImagePrimaries::AdobeRgb1998,
        RgbSpace::Rec2020 => ImagePrimaries::Rec2020,
    }
}
fn interpretation(index: usize) -> anyhow::Result<PixelColor> {
    if index < 4 {
        return Ok(ImageColor {
            primaries: primaries(SPACES[index]),
            transfer: match index {
                2 => pipeline::TransferFunction::AdobeRgb1998,
                3 => pipeline::TransferFunction::Linear,
                _ => pipeline::TransferFunction::Srgb,
            },
            alpha: AlphaMode::Straight,
        }
        .into());
    }
    let bytes: &[u8] = match index {
        4 => include_bytes!(
            "../../../../extensions/crates/xgpui-color/tests/reference/real-icc/sRGB-v4.icc"
        ),
        5 => include_bytes!(
            "../../../../extensions/crates/xgpui-color/tests/reference/real-icc/DisplayP3-v4.icc"
        ),
        6 => include_bytes!(
            "../../../../extensions/crates/xgpui-color/tests/reference/real-icc/AdobeCompat-v4.icc"
        ),
        _ => include_bytes!(
            "../../../../extensions/crates/xgpui-color/tests/reference/real-icc/sRGB2014.icc"
        ),
    };
    Ok(PixelColor::Custom {
        rgb: Arc::new(IccRgbProfile::parse(bytes)?.into_rgb_definition()),
        alpha: AlphaMode::Straight,
    })
}
pub fn half(bits: u16) -> f64 {
    let sign = if bits & 0x8000 == 0 { 1. } else { -1. };
    let e = (bits >> 10) & 31;
    let m = f64::from(bits & 1023);
    if e == 31 {
        return f64::NAN;
    }
    sign * if e == 0 {
        m * 2f64.powi(-24)
    } else {
        (1. + m / 1024.) * 2f64.powi(i32::from(e) - 15)
    }
}
pub struct Sample {
    pub xyz: [f64; 3],
    pub stored: [f64; 4],
    pub bits: [u16; 4],
    pub preview: [f32; 3],
}
pub fn sample(
    index: usize,
    rgb: [f32; 3],
    alpha: f32,
    rec: bool,
    mapping: GamutMapping,
) -> anyhow::Result<Sample> {
    let color = interpretation(index)?;
    let rgba = [rgb[0], rgb[1], rgb[2], alpha];
    let source = color
        .decode(rgba.map(f64::from), ImagePrimaries::Rec2020)?
        .rgba();
    let xyz = LinearRgb::new(RgbSpace::Rec2020, [source[0], source[1], source[2]])?
        .to_xyz()?
        .components();
    let space = if rec {
        RgbSpace::Rec2020
    } else {
        RgbSpace::DisplayP3
    };
    let data = pipeline::pixels_rgba16f_data(1, 1, vec![rgba], color, primaries(space), mapping)?;
    let bytes = &data.levels()[0];
    let bits = std::array::from_fn(|i| u16::from_le_bytes([bytes[i * 2], bytes[i * 2 + 1]]));
    let stored = bits.map(half);
    let linear = if stored[3] == 0. {
        [0.; 3]
    } else {
        std::array::from_fn(|i| stored[i] / stored[3])
    };
    let mapped = LinearRgb::new(space, linear)?
        .map_gamut(RgbSpace::Srgb, GamutMapping::Clip)?
        .components();
    let composed = mapped.map(|v| v * stored[3] + 1. - stored[3]);
    let preview = LinearRgb::new(RgbSpace::Srgb, composed)?
        .encode()?
        .components()
        .map(|v| v as f32);
    Ok(Sample {
        xyz,
        stored,
        bits,
        preview,
    })
}
pub fn source_xy(space: RgbSpace, rgb: [f32; 3]) -> Option<[f64; 2]> {
    let linear = if space == RgbSpace::Rec2020 {
        LinearRgb::new(space, rgb.map(f64::from)).ok()?
    } else {
        EncodedRgb::new(space, rgb.map(f64::from))
            .ok()?
            .decode()
            .ok()?
    };
    xy(linear.to_xyz().ok()?.components())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn published_sampled_profile_keeps_exact_gray_and_alpha_semantics() {
        let p = sample(7, [0.18; 3], 0.5, true, GamutMapping::Clip).unwrap();
        let expected = [
            0.02586876064369192,
            0.027213980056848303,
            0.02964513297459798,
        ];
        for (a, e) in p.xyz.into_iter().zip(expected) {
            assert!((a - e).abs() < 128. * f64::EPSILON * 0.1);
        }
        assert_eq!(p.bits, [0x22f8, 0x22f7, 0x22f8, 0x3800]);
        let zero = sample(7, [0.18; 3], 0., true, GamutMapping::Clip).unwrap();
        assert_eq!(zero.xyz, [0.; 3]);
        assert_eq!(zero.bits, [0; 4]);
        assert_eq!(zero.preview, [1.; 3]);
        assert_ne!(
            sample(4, [0.18; 3], 0.5, true, GamutMapping::Clip)
                .unwrap()
                .xyz,
            p.xyz
        );
    }
    #[test]
    fn spectral_fill_mapping_and_metadata_change_the_result() {
        let boundary = locus();
        assert_eq!(boundary.len(), 471);
        let a = cells(RgbSpace::Srgb, 0.18, GamutMapping::Clip, &boundary).unwrap();
        let b = cells(RgbSpace::DisplayP3, 0.18, GamutMapping::Clip, &boundary).unwrap();
        assert_eq!(a.len(), b.len());
        assert!(a.iter().filter(|c| c.outside).count() > b.iter().filter(|c| c.outside).count());
        assert!(
            a.iter()
                .all(|c| c.rgb.into_iter().all(|v| (0. ..=1.).contains(&v)))
        );
        assert_ne!(
            source_xy(RgbSpace::Srgb, [0., 1., 0.]),
            source_xy(RgbSpace::DisplayP3, [0., 1., 0.])
        );
    }
}
