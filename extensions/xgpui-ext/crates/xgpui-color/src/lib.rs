//! CPU-only, unclipped color conversions. All RGB spaces and XYZ use D65.
//! Matrices are row-major and multiply column vectors. Alpha is not involved.
#![doc = include_str!("../README.md")]

use std::fmt;
mod gradient;
mod mapping;
pub use gradient::{ColorInterpolation, HueInterpolation, LinearGradient, LinearRgba};
pub use mapping::GamutMapping;

/// Invalid input or a conversion whose result cannot be represented as finite f64.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColorError;
impl fmt::Display for ColorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid color input or non-finite conversion result")
    }
}
impl std::error::Error for ColorError {}
type Result<T> = std::result::Result<T, ColorError>;
fn finite(v: [f64; 3]) -> Result<[f64; 3]> {
    if v.iter().all(|v| v.is_finite()) {
        Ok(v)
    } else {
        Err(ColorError)
    }
}

/// SDR RGB primaries and transfer function. Display P3 uses the sRGB transfer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RgbSpace {
    Srgb,
    DisplayP3,
    Rec2020,
}
impl RgbSpace {
    /// Linear RGB to relative XYZ/D65 (Y = 1 at reference white).
    /// Constants follow CSS Color 4 sample conversions (rational primary matrices).
    pub fn linear_to_xyz_matrix(self) -> [[f64; 3]; 3] {
        match self {
            Self::Srgb => [
                [506752. / 1228815., 87881. / 245763., 12673. / 70218.],
                [87098. / 409605., 175762. / 245763., 12673. / 175545.],
                [7918. / 409605., 87881. / 737289., 1001167. / 1053270.],
            ],
            Self::DisplayP3 => [
                [608311. / 1250200., 189793. / 714400., 198249. / 1000160.],
                [35783. / 156275., 247089. / 357200., 198249. / 2500400.],
                [0., 32229. / 714400., 5220557. / 5000800.],
            ],
            Self::Rec2020 => [
                [
                    63426534. / 99577255.,
                    20160776. / 139408157.,
                    47086771. / 278816314.,
                ],
                [
                    26158966. / 99577255.,
                    472592308. / 697040785.,
                    8267143. / 139408157.,
                ],
                [0., 19567812. / 697040785., 295819943. / 278816314.],
            ],
        }
    }
    /// Relative XYZ/D65 to linear RGB, without clipping or adaptation.
    pub fn xyz_to_linear_matrix(self) -> [[f64; 3]; 3] {
        inverse(self.linear_to_xyz_matrix())
    }
    /// Linear RGB primary conversion only; does not decode encoded RGB.
    pub fn linear_to_linear_matrix(self, destination: Self) -> [[f64; 3]; 3] {
        if self == destination {
            return [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
        }
        let a = destination.xyz_to_linear_matrix();
        let b = self.linear_to_xyz_matrix();
        std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| a[i][k] * b[k][j]).sum()))
    }
    fn transfer(self, value: f64, decode: bool) -> f64 {
        let x = value.abs();
        let y = match (self, decode) {
            (Self::Rec2020, true) => {
                let a = 1.09929682680944;
                if x < 0.018053968510807 * 4.5 {
                    x / 4.5
                } else {
                    ((x + a - 1.) / a).powf(1. / 0.45)
                }
            }
            (Self::Rec2020, false) => {
                if x < 0.018053968510807 {
                    4.5 * x
                } else {
                    1.09929682680944 * x.powf(0.45) - 0.09929682680944
                }
            }
            (_, true) => {
                if x <= 0.04045 {
                    x / 12.92
                } else {
                    ((x + 0.055) / 1.055).powf(2.4)
                }
            }
            (_, false) => {
                if x <= 0.0031308 {
                    12.92 * x
                } else {
                    1.055 * x.powf(1. / 2.4) - 0.055
                }
            }
        };
        y.copysign(value)
    }
}
fn mul(m: [[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    m.map(|r| r[0] * v[0] + r[1] * v[1] + r[2] * v[2])
}
fn inverse(m: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let [[a, b, c], [d, e, f], [g, h, i]] = m;
    let det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
    [
        [e * i - f * h, c * h - b * i, b * f - c * e],
        [f * g - d * i, a * i - c * g, c * d - a * f],
        [d * h - e * g, b * g - a * h, a * e - b * d],
    ]
    .map(|r| r.map(|x| x / det))
}

macro_rules! rgb {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Copy, Debug, PartialEq)]
        pub struct $name {
            components: [f64; 3],
            space: RgbSpace,
        }
        impl $name {
            pub fn new(space: RgbSpace, components: [f64; 3]) -> Result<Self> {
                Ok(Self {
                    space,
                    components: finite(components)?,
                })
            }
            pub fn components(self) -> [f64; 3] {
                self.components
            }
            pub fn space(self) -> RgbSpace {
                self.space
            }
        }
    };
}
rgb!(
    EncodedRgb,
    "Finite encoded RGB, possibly outside [0,1]; no implicit gamut mapping."
);
rgb!(
    LinearRgb,
    "Finite linear RGB, possibly outside [0,1]; no implicit gamut mapping."
);
impl EncodedRgb {
    /// Sign-preserving extended transfer decoding, including negative channels.
    pub fn decode(self) -> Result<LinearRgb> {
        LinearRgb::new(
            self.space,
            self.components.map(|v| self.space.transfer(v, true)),
        )
    }
}
impl LinearRgb {
    pub fn encode(self) -> Result<EncodedRgb> {
        EncodedRgb::new(
            self.space,
            self.components.map(|v| self.space.transfer(v, false)),
        )
    }
    pub fn convert(self, destination: RgbSpace) -> Result<Self> {
        Self::new(
            destination,
            mul(
                self.space.linear_to_linear_matrix(destination),
                self.components,
            ),
        )
    }
    pub fn to_xyz(self) -> Result<XyzD65> {
        XyzD65::new(mul(self.space.linear_to_xyz_matrix(), self.components))
    }
    pub fn to_oklab(self) -> Result<Oklab> {
        let [r, g, b] = self.convert(RgbSpace::Srgb)?.components;
        let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
        let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
        let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
        Oklab::new([
            0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
            1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
            0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
        ])
    }
}
/// Relative XYZ with D65 white; components are unclipped.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct XyzD65([f64; 3]);
impl XyzD65 {
    pub fn new(v: [f64; 3]) -> Result<Self> {
        Ok(Self(finite(v)?))
    }
    pub fn components(self) -> [f64; 3] {
        self.0
    }
    pub fn to_linear(self, space: RgbSpace) -> Result<LinearRgb> {
        LinearRgb::new(space, mul(space.xyz_to_linear_matrix(), self.0))
    }
}
/// OkLab [L, a, b], with nominal SDR L in 0..1; extended coordinates are retained.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Oklab([f64; 3]);
impl Oklab {
    pub fn new(v: [f64; 3]) -> Result<Self> {
        Ok(Self(finite(v)?))
    }
    pub fn components(self) -> [f64; 3] {
        self.0
    }
    pub fn to_linear(self, space: RgbSpace) -> Result<LinearRgb> {
        let [l, a, b] = self.0;
        let x = (l + 0.3963377774 * a + 0.2158037573 * b).powi(3);
        let y = (l - 0.1055613458 * a - 0.0638541728 * b).powi(3);
        let z = (l - 0.0894841775 * a - 1.2914855480 * b).powi(3);
        LinearRgb::new(
            RgbSpace::Srgb,
            [
                4.0767416621 * x - 3.3077115913 * y + 0.2309699292 * z,
                -1.2684380046 * x + 2.6097574011 * y - 0.3413193965 * z,
                -0.0041960863 * x - 0.7034186147 * y + 1.7076147010 * z,
            ],
        )?
        .convert(space)
    }
    pub fn to_oklch(self) -> Result<Oklch> {
        let [l, a, b] = self.0;
        Oklch::new(l, a.hypot(b), b.atan2(a).to_degrees())
    }
}
/// OkLCh: lightness, nonnegative chroma, hue in degrees normalized to [0,360).
/// Exact zero chroma has canonical hue zero. Near-neutral hues are retained.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Oklch([f64; 3]);
impl Oklch {
    pub fn new(l: f64, c: f64, h: f64) -> Result<Self> {
        finite([l, c, h])?;
        if c < 0. {
            return Err(ColorError);
        }
        let hue = h.rem_euclid(360.);
        // A tiny negative angle can round up to exactly 360 in floating point.
        Ok(Self([l, c, if c == 0. || hue >= 360. { 0. } else { hue }]))
    }
    pub fn components(self) -> [f64; 3] {
        self.0
    }
    pub fn to_oklab(self) -> Result<Oklab> {
        let [l, c, h] = self.0;
        Oklab::new([l, c * h.to_radians().cos(), c * h.to_radians().sin()])
    }
}

#[cfg(test)]
mod tests;

/// Resource-free WGSL helpers; see the crate guide for IDs and input contracts.
pub const WGSL_COLOR_MATH: &str = include_str!("math.wgsl");
