use anyhow::{Result, ensure};
use xgpui_color::{EncodedRgb, GamutMapping, LinearRgb, RgbSpace};

/// D65 SDR primaries. Storage format is specified independently.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ImagePrimaries {
    Srgb,
    DisplayP3,
    AdobeRgb1998,
}
impl ImagePrimaries {
    fn space(self) -> RgbSpace {
        match self {
            Self::Srgb => RgbSpace::Srgb,
            Self::DisplayP3 => RgbSpace::DisplayP3,
            Self::AdobeRgb1998 => RgbSpace::AdobeRgb1998,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TransferFunction {
    Linear,
    /// Shared by encoded sRGB and encoded Display P3.
    Srgb,
    /// Adobe RGB (1998): decode with exponent 563/256, encode with 256/563.
    AdobeRgb1998,
}
impl TransferFunction {
    // Transfer curves are independent of primaries. Re-tag the numeric channels
    // for curve evaluation only; no primary conversion occurs here.
    fn decode(self, rgb: [f64; 3]) -> Result<[f64; 3]> {
        let space = match self {
            Self::Linear => return Ok(rgb),
            Self::Srgb => RgbSpace::Srgb,
            Self::AdobeRgb1998 => RgbSpace::AdobeRgb1998,
        };
        Ok(EncodedRgb::new(space, rgb)?.decode()?.components())
    }
    fn encode(self, rgb: [f64; 3]) -> Result<[f64; 3]> {
        let space = match self {
            Self::Linear => return Ok(rgb),
            Self::Srgb => RgbSpace::Srgb,
            Self::AdobeRgb1998 => RgbSpace::AdobeRgb1998,
        };
        Ok(LinearRgb::new(space, rgb)?.encode()?.components())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AlphaMode {
    Straight,
    /// RGB was multiplied by alpha in the declared transfer domain.
    Premultiplied,
}
/// Explicit pixel interpretation; neither an ICC profile nor an HDR descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ImageColor {
    pub primaries: ImagePrimaries,
    pub transfer: TransferFunction,
    pub alpha: AlphaMode,
}
impl ImageColor {
    pub const SRGB: Self = Self {
        primaries: ImagePrimaries::Srgb,
        transfer: TransferFunction::Srgb,
        alpha: AlphaMode::Straight,
    };
    /// Explicit encoded Adobe RGB (1998) supplied pixels; no ICC inference.
    pub const ADOBE_RGB_1998: Self = Self {
        primaries: ImagePrimaries::AdobeRgb1998,
        transfer: TransferFunction::AdobeRgb1998,
        alpha: AlphaMode::Straight,
    };
    pub const LINEAR_P3: Self = Self {
        primaries: ImagePrimaries::DisplayP3,
        transfer: TransferFunction::Linear,
        alpha: AlphaMode::Straight,
    };
    /// CPU reference: unassociate in the source transfer domain, decode, then
    /// convert primaries. Input is bounded SDR (RGB/alpha 0..1). Transparent RGB
    /// canonicalizes to zero; associated RGB must not exceed alpha.
    pub fn decode(self, rgba: [f64; 4], working: ImagePrimaries) -> Result<WorkingPixel> {
        ensure!(
            rgba.iter()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
            "invalid bounded SDR pixel"
        );
        let [r, g, b, alpha] = rgba;
        let mut rgb = [r, g, b];
        if self.alpha == AlphaMode::Premultiplied {
            ensure!(
                rgb.iter().all(|v| *v <= alpha),
                "premultiplied RGB exceeds alpha"
            );
            if alpha > 0. {
                rgb = rgb.map(|v| v / alpha);
            }
        }
        if alpha == 0. {
            rgb = [0.; 3];
        }
        let linear = LinearRgb::new(self.primaries.space(), self.transfer.decode(rgb)?)?;
        let rgb = linear.convert(working.space())?.components();
        WorkingPixel::new(working, [rgb[0], rgb[1], rgb[2], alpha])
    }
}

/// Finite straight-alpha linear RGB. Extended RGB is retained until `output`;
/// alpha stays bounded. Zero alpha has canonical zero RGB, even after offsets.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorkingPixel {
    primaries: ImagePrimaries,
    rgba: [f64; 4],
}
impl WorkingPixel {
    pub fn new(primaries: ImagePrimaries, mut rgba: [f64; 4]) -> Result<Self> {
        ensure!(
            rgba.iter().all(|v| v.is_finite()) && (0.0..=1.0).contains(&rgba[3]),
            "invalid working pixel"
        );
        if rgba[3] == 0. {
            rgba = [0.; 4];
        }
        Ok(Self { primaries, rgba })
    }
    pub fn primaries(self) -> ImagePrimaries {
        self.primaries
    }
    pub fn rgba(self) -> [f64; 4] {
        self.rgba
    }
    /// Convert primaries and explicitly map to bounded destination SDR, then
    /// encode and associate in that order. This is CPU reference output, not an
    /// OS swapchain format. Native wide-SDR adapters retain their #25 contracts.
    pub fn output(self, destination: ImageColor, mapping: GamutMapping) -> Result<[f64; 4]> {
        let [r, g, b, a] = self.rgba;
        let linear = LinearRgb::new(self.primaries.space(), [r, g, b])?
            .map_gamut(destination.primaries.space(), mapping)?;
        let mut rgb = destination.transfer.encode(linear.components())?;
        // Absorb transfer rounding at the endpoints after explicit mapping.
        rgb = rgb.map(|v| v.clamp(0., 1.));
        if destination.alpha == AlphaMode::Premultiplied {
            rgb = rgb.map(|v| v * a);
        }
        Ok([rgb[0], rgb[1], rgb[2], a])
    }
}

/// CPU reference affine RGB transform in one declared linear working space.
/// Rows multiply [R,G,B,1]. Alpha is never a matrix input or output. Builders
/// compose in call order and never clip intermediate RGB. Invalid parameters
/// and nonfinite composition/results return errors rather than silently clamp.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorMatrix {
    primaries: ImagePrimaries,
    rows: [[f64; 4]; 3],
}
impl ColorMatrix {
    pub fn identity(primaries: ImagePrimaries) -> Self {
        Self {
            primaries,
            rows: [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]],
        }
    }
    pub fn new(primaries: ImagePrimaries, rows: [[f64; 4]; 3]) -> Result<Self> {
        ensure!(
            rows.iter().flatten().all(|v| v.is_finite()),
            "nonfinite color matrix"
        );
        Ok(Self { primaries, rows })
    }
    pub fn primaries(self) -> ImagePrimaries {
        self.primaries
    }
    pub fn rows(self) -> [[f64; 4]; 3] {
        self.rows
    }
    pub fn then(self, next: Self) -> Result<Self> {
        ensure!(
            self.primaries == next.primaries,
            "matrix working spaces differ"
        );
        let rows = std::array::from_fn(|i| {
            std::array::from_fn(|j| {
                (0..3)
                    .map(|k| next.rows[i][k] * self.rows[k][j])
                    .sum::<f64>()
                    + if j == 3 { next.rows[i][3] } else { 0. }
            })
        });
        Self::new(self.primaries, rows)
    }
    /// Exposure in stops: +1 EV doubles linear RGB; -1 EV halves it.
    pub fn exposure(self, ev: f64) -> Result<Self> {
        let gain = ev.exp2();
        ensure!(
            ev.is_finite() && gain.is_finite() && gain > 0.,
            "invalid exposure EV"
        );
        self.then(Self::new(
            self.primaries,
            [[gain, 0., 0., 0.], [0., gain, 0., 0.], [0., 0., gain, 0.]],
        )?)
    }
    /// Linear contrast around an explicit 0..1 pivot. Factor must be >=0.
    pub fn contrast(self, factor: f64, pivot: f64) -> Result<Self> {
        ensure!(
            factor.is_finite() && factor >= 0. && pivot.is_finite() && (0.0..=1.0).contains(&pivot),
            "invalid contrast factor or pivot"
        );
        let offset = pivot * (1. - factor);
        self.then(Self::new(
            self.primaries,
            [
                [factor, 0., 0., offset],
                [0., factor, 0., offset],
                [0., 0., factor, offset],
            ],
        )?)
    }
    /// Linear luminance-axis saturation. 0 is grayscale, 1 is identity.
    /// Coefficients are the Y row of the working primaries' D65 XYZ matrix.
    pub fn saturation(self, factor: f64) -> Result<Self> {
        ensure!(
            factor.is_finite() && factor >= 0.,
            "invalid saturation factor"
        );
        let y = self.primaries.space().linear_to_xyz_matrix()[1];
        let rows = std::array::from_fn(|i| {
            std::array::from_fn(|j| {
                if j == 3 {
                    0.
                } else {
                    (1. - factor) * y[j] + if i == j { factor } else { 0. }
                }
            })
        });
        self.then(Self::new(self.primaries, rows)?)
    }
    pub fn apply(self, pixel: WorkingPixel) -> Result<WorkingPixel> {
        ensure!(
            self.primaries == pixel.primaries,
            "pixel and matrix working spaces differ"
        );
        let [r, g, b, a] = pixel.rgba;
        let rgb = self
            .rows
            .map(|row| row[0] * r + row[1] * g + row[2] * b + row[3]);
        WorkingPixel::new(self.primaries, [rgb[0], rgb[1], rgb[2], a])
    }
}

/// CPU reference darkening after the matrix, before output mapping. Radius is
/// the unaffected inner radius; feather is the transition width in radii.
/// Radius 1 reaches the shortest image edge at roundness 1. Roundness 0 follows
/// the image's ellipse; 1 is a circle in image-pixel coordinates. Intensity and
/// roundness are 0..1, radius >=0 and feather >0. No arbitrary alpha transform.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vignette {
    intensity: f64,
    radius: f64,
    feather: f64,
    roundness: f64,
}
impl Vignette {
    /// Intensity, inner radius, feather and roundness, in the CPU reference units.
    pub fn parameters(self) -> [f64; 4] {
        [self.intensity, self.radius, self.feather, self.roundness]
    }

    pub fn new(intensity: f64, radius: f64, feather: f64, roundness: f64) -> Result<Self> {
        ensure!(
            [intensity, radius, feather, roundness]
                .iter()
                .all(|v| v.is_finite())
                && (0.0..=1.0).contains(&intensity)
                && radius >= 0.
                && feather > 0.
                && (radius + feather).is_finite()
                && (0.0..=1.0).contains(&roundness),
            "invalid vignette parameters"
        );
        Ok(Self {
            intensity,
            radius,
            feather,
            roundness,
        })
    }
    /// UV addresses the full source image, never a tile or destination widget.
    pub fn factor(self, uv: [f64; 2], image_size: [u32; 2]) -> Result<f64> {
        ensure!(
            uv.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v))
                && image_size.iter().all(|v| *v > 0),
            "invalid vignette coordinates"
        );
        let x = 2. * uv[0] - 1.;
        let y = 2. * uv[1] - 1.;
        let [w, h] = image_size.map(f64::from);
        let shortest = w.min(h);
        let ellipse = x.hypot(y);
        let circle = (x * w / shortest).hypot(y * h / shortest);
        let distance = ellipse + self.roundness * (circle - ellipse);
        let t = ((distance - self.radius) / self.feather).clamp(0., 1.);
        Ok(1. - self.intensity * t * t * (3. - 2. * t))
    }
    pub fn apply(
        self,
        pixel: WorkingPixel,
        uv: [f64; 2],
        image_size: [u32; 2],
    ) -> Result<WorkingPixel> {
        let factor = self.factor(uv, image_size)?;
        let [r, g, b, a] = pixel.rgba;
        WorkingPixel::new(pixel.primaries, [r * factor, g * factor, b * factor, a])
    }
}
