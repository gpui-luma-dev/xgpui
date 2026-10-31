//! Optional bounded RGB matrix/TRC parsing. moxcms parses; this crate evaluates.
use crate::{ColorError, ParametricCurve, RgbCurve, RgbDefinition, SampledCurve, finite, mul};
use moxcms::{ColorProfile, DataColorSpace, ParsingOptions, ToneReprCurve};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IccError {
    Limit,
    Unsupported,
    Malformed,
    InvalidColor,
}
impl fmt::Display for IccError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Limit => "ICC profile exceeds bounded adapter limits",
            Self::Unsupported => "unsupported ICC profile or metadata",
            Self::Malformed => "malformed ICC profile",
            Self::InvalidColor => "invalid ICC color definition or pixel",
        })
    }
}
impl std::error::Error for IccError {}
impl From<ColorError> for IccError {
    fn from(_: ColorError) -> Self {
        Self::InvalidColor
    }
}
type Result<T> = std::result::Result<T, IccError>;

/// Validated SDR RGB matrix/TRC profile. No file decoding, transform engine,
/// monitor selection, rendering intents, or black-point compensation.
#[derive(Clone, Debug)]
pub struct IccRgbProfile {
    definition: RgbDefinition,
    colorants: [[f64; 3]; 3],
    pcs_white: [f64; 3],
}
impl IccRgbProfile {
    pub const MAX_PROFILE_BYTES: usize = 512 * 1024;
    pub const MAX_TAGS: usize = 32;
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        preflight(bytes)?;
        let profile = ColorProfile::new_from_slice_with_options(
            bytes,
            ParsingOptions {
                max_profile_size: Self::MAX_PROFILE_BYTES + 1,
                max_allowed_clut_size: 0,
                max_allowed_trc_size: SampledCurve::MAX_SAMPLES,
            },
        )
        .map_err(|_| IccError::Malformed)?;
        if profile.color_space != DataColorSpace::Rgb
            || profile.pcs != DataColorSpace::Xyz
            || profile.cicp.is_some()
        {
            return Err(IccError::Unsupported);
        }
        let convert = |curve: Option<ToneReprCurve>| -> Result<RgbCurve> {
            match curve.ok_or(IccError::Malformed)? {
                ToneReprCurve::Lut(values) => match values.len() {
                    0 => Ok(RgbCurve::Linear),
                    1 => {
                        let gamma = f64::from(values[0]) / 256.;
                        if gamma <= 0. {
                            return Err(IccError::InvalidColor);
                        }
                        Ok(RgbCurve::Gamma(gamma))
                    }
                    _ => Ok(RgbCurve::Sampled(SampledCurve::new(values)?)),
                },
                ToneReprCurve::Parametric(values) => {
                    // s15Fixed16 coefficients with |value|<128 are exact in f32.
                    // Reject larger ones rather than accept parser precision loss.
                    if values.iter().any(|v| !v.is_finite() || v.abs() >= 128.) {
                        return Err(IccError::Unsupported);
                    }
                    let p: Vec<f64> = values.into_iter().map(f64::from).collect();
                    let coefficients = match p.as_slice() {
                        [g] => [*g, 1., 0., 0., 0., 0., 0.],
                        [g, a, b, c, d] => [*g, *a, *b, *c, *d, 0., 0.],
                        [g, a, b, c, d, e, f] => [*g, *a, *b, *c, *d, *e, *f],
                        _ => return Err(IccError::Unsupported),
                    };
                    Ok(RgbCurve::Parametric(ParametricCurve::new(coefficients)?))
                }
            }
        };
        let curves = [
            convert(profile.red_trc)?,
            convert(profile.green_trc)?,
            convert(profile.blue_trc)?,
        ];
        let columns = [
            profile.red_colorant,
            profile.green_colorant,
            profile.blue_colorant,
        ]
        .map(|v| [v.x, v.y, v.z]);
        let colorants = std::array::from_fn(|i| std::array::from_fn(|j| columns[j][i]));
        let pcs_white = [
            profile.white_point.x,
            profile.white_point.y,
            profile.white_point.z,
        ];
        // Matrix tags already describe the PCS. chad describes their history;
        // applying it again would double-adapt. Adapt the encoded PCS white once.
        let definition = RgbDefinition::new_with_xyz_white(colorants, curves, pcs_white)?;
        Ok(Self {
            definition,
            colorants,
            pcs_white,
        })
    }
    pub fn rgb_definition(&self) -> &RgbDefinition {
        &self.definition
    }
    pub fn into_rgb_definition(self) -> RgbDefinition {
        self.definition
    }
    pub fn pcs_white(&self) -> [f64; 3] {
        self.pcs_white
    }
    pub fn colorants(&self) -> [[f64; 3]; 3] {
        self.colorants
    }
    /// Decode bounded encoded RGB into the profile's relative XYZ PCS.
    /// Alpha stays with the caller. No clipping of XYZ/working RGB.
    pub fn decode_to_pcs(&self, rgb: [f64; 3]) -> Result<[f64; 3]> {
        if rgb
            .iter()
            .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
        {
            return Err(IccError::InvalidColor);
        }
        let c = self.definition.curves();
        Ok(finite(mul(
            self.colorants,
            [
                c[0].decode(rgb[0])?,
                c[1].decode(rgb[1])?,
                c[2].decode(rgb[2])?,
            ],
        ))?)
    }
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<usize> {
    let data = bytes.get(offset..offset + 4).ok_or(IccError::Malformed)?;
    Ok(u32::from_be_bytes(data.try_into().map_err(|_| IccError::Malformed)?) as usize)
}
fn preflight(bytes: &[u8]) -> Result<()> {
    if bytes.len() > IccRgbProfile::MAX_PROFILE_BYTES {
        return Err(IccError::Limit);
    }
    if bytes.len() < 132 || u32_at(bytes, 0)? != bytes.len() || &bytes[36..40] != b"acsp" {
        return Err(IccError::Malformed);
    }
    if ![2, 4].contains(&bytes[8])
        || ![b"mntr", b"scnr"].contains(&bytes[12..16].try_into().map_err(|_| IccError::Malformed)?)
        || &bytes[16..20] != b"RGB "
        || &bytes[20..24] != b"XYZ "
    {
        return Err(IccError::Unsupported);
    }
    for (at, expected) in [(68, 0x0000f6d6usize), (72, 0x00010000), (76, 0x0000d32d)] {
        if u32_at(bytes, at)? != expected {
            return Err(IccError::Unsupported);
        }
    }
    let count = u32_at(bytes, 128)?;
    if count > IccRgbProfile::MAX_TAGS {
        return Err(IccError::Limit);
    }
    let end = 132 + count * 12;
    if end > bytes.len() {
        return Err(IccError::Malformed);
    }
    let mut signatures = Vec::with_capacity(count);
    let mut ranges = Vec::with_capacity(count);
    for index in 0..count {
        let at = 132 + index * 12;
        let sig = &bytes[at..at + 4];
        let offset = u32_at(bytes, at + 4)?;
        let size = u32_at(bytes, at + 8)?;
        let finish = offset.checked_add(size).ok_or(IccError::Malformed)?;
        if offset < end
            || offset % 4 != 0
            || size < 8
            || finish > bytes.len()
            || signatures.contains(&sig)
        {
            return Err(IccError::Malformed);
        }
        for &(a, b) in &ranges {
            if offset < b && a < finish && (a, b) != (offset, finish) {
                return Err(IccError::Malformed);
            }
        }
        signatures.push(sig);
        ranges.push((offset, finish));
        if ![
            b"rXYZ", b"gXYZ", b"bXYZ", b"rTRC", b"gTRC", b"bTRC", b"wtpt", b"chad", b"desc",
            b"cprt", b"dmnd", b"dmdd", b"chrm", b"lumi", b"meas", b"bkpt", b"tech", b"vued",
        ]
        .contains(&sig.try_into().map_err(|_| IccError::Malformed)?)
        {
            return Err(IccError::Unsupported);
        }
        // Descriptive/display-measurement metadata does not alter this adapter's
        // relative matrix/TRC decoding. In particular, do not apply black-point
        // compensation or absolute display luminance to supplied source pixels.
        if [
            b"desc", b"cprt", b"dmnd", b"dmdd", b"chrm", b"lumi", b"meas", b"bkpt", b"tech",
            b"vued",
        ]
        .contains(&sig.try_into().map_err(|_| IccError::Malformed)?)
            && size > 4096
        {
            return Err(IccError::Limit);
        }
        if [b"rTRC", b"gTRC", b"bTRC"].contains(&sig.try_into().map_err(|_| IccError::Malformed)?) {
            let kind = &bytes[offset..offset + 4];
            if kind == b"curv" {
                if size < 12 {
                    return Err(IccError::Malformed);
                }
                let entries = u32_at(bytes, offset + 8)?;
                if entries > SampledCurve::MAX_SAMPLES {
                    return Err(IccError::Limit);
                }
                if 12 + entries * 2 > size {
                    return Err(IccError::Malformed);
                }
            } else if kind == b"para" {
                if size < 12 {
                    return Err(IccError::Malformed);
                }
                let function = u16::from_be_bytes([bytes[offset + 8], bytes[offset + 9]]);
                if ![0, 3, 4].contains(&function) {
                    return Err(IccError::Unsupported);
                }
                let params = match function {
                    0 => 1,
                    3 => 5,
                    _ => 7,
                };
                if size < 12 + params * 4 {
                    return Err(IccError::Malformed);
                }
                for i in 0..params {
                    let start = offset + 12 + i * 4;
                    let coefficient = i32::from_be_bytes(
                        bytes[start..start + 4]
                            .try_into()
                            .map_err(|_| IccError::Malformed)?,
                    );
                    if f64::from(coefficient).abs() >= 128. * 65536. {
                        return Err(IccError::Unsupported);
                    }
                }
            } else {
                return Err(IccError::Unsupported);
            }
        }
    }
    Ok(())
}
