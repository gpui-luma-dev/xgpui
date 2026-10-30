//! Explicit custom RGB metadata. No profile parser or file-format knowledge.
use crate::{
    BradfordAdaptation, ColorError, Result, RgbSpace, WhitePoint, XyzD65, finite, inverse, mul,
};

/// Independent channel decoding curve. Named curves preserve signed extensions;
/// sampled and parametric curves explicitly use bounded input/output 0..1.
#[derive(Clone, Debug, PartialEq)]
pub enum RgbCurve {
    Linear,
    Srgb,
    Gamma(f64),
    Sampled(SampledCurve),
    Parametric(ParametricCurve),
}

/// Owned monotone normalized u16 samples, interpolated continuously in f64.
#[derive(Clone, PartialEq)]
pub struct SampledCurve(std::sync::Arc<[u16]>);
impl std::fmt::Debug for SampledCurve {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SampledCurve")
            .field("sample_count", &self.0.len())
            .field("first", &self.0.first())
            .field("last", &self.0.last())
            .finish()
    }
}
impl SampledCurve {
    pub const MAX_SAMPLES: usize = 65536;
    pub fn new(samples: Vec<u16>) -> Result<Self> {
        if !(2..=Self::MAX_SAMPLES).contains(&samples.len())
            || samples.windows(2).any(|p| p[0] > p[1])
        {
            return Err(ColorError);
        }
        Ok(Self(samples.into()))
    }
    pub fn samples(&self) -> &[u16] {
        &self.0
    }
    fn decode(&self, x: f64) -> f64 {
        let position = x * (self.0.len() - 1) as f64;
        let i = (position.floor() as usize).min(self.0.len() - 2);
        let t = position - i as f64;
        let a = f64::from(self.0[i]);
        (a + (f64::from(self.0[i + 1]) - a) * t) / 65535.
    }
}

/// Bounded piecewise curve: (a*x+b)^g+e for x>=d, c*x+f otherwise.
/// Coefficients are [g,a,b,c,d,e,f]. Serialized discontinuities are retained;
/// no substitution of ideal named curves. Finite output is clipped to 0..1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParametricCurve([f64; 7]);
impl ParametricCurve {
    pub fn new(coefficients: [f64; 7]) -> Result<Self> {
        let [g, a, b, c, d, _, _] = coefficients;
        if coefficients.iter().any(|v| !v.is_finite())
            || g <= 0.
            || a <= 0.
            || c < 0.
            || (d <= 1. && a * d.max(0.) + b < 0.)
        {
            return Err(ColorError);
        }
        if d > 0. && !(c * d.min(1.) + coefficients[6]).is_finite() {
            return Err(ColorError);
        }
        let curve = Self(coefficients);
        for x in [0., 1., d.clamp(0., 1.)] {
            if !curve.raw(x).is_finite() {
                return Err(ColorError);
            }
        }
        Ok(curve)
    }
    pub fn coefficients(self) -> [f64; 7] {
        self.0
    }
    fn raw(self, x: f64) -> f64 {
        let [g, a, b, c, d, e, f] = self.0;
        if x >= d {
            (a * x + b).powf(g) + e
        } else {
            c * x + f
        }
    }
}
impl RgbCurve {
    fn validate(&self) -> Result<()> {
        if let Self::Gamma(gamma) = self
            && (!gamma.is_finite() || *gamma <= 0.)
        {
            return Err(ColorError);
        }
        Ok(())
    }
    pub fn decode(&self, value: f64) -> Result<f64> {
        if !value.is_finite() {
            return Err(ColorError);
        }
        let decoded = match self {
            Self::Linear => value,
            Self::Srgb => RgbSpace::Srgb.transfer(value, true),
            Self::Gamma(gamma) => {
                self.validate()?;
                value.abs().powf(*gamma).copysign(value)
            }
            Self::Sampled(curve) => {
                if !(0. ..=1.).contains(&value) {
                    return Err(ColorError);
                }
                curve.decode(value)
            }
            Self::Parametric(curve) => {
                if !(0. ..=1.).contains(&value) {
                    return Err(ColorError);
                }
                let raw = curve.raw(value);
                if !raw.is_finite() {
                    return Err(ColorError);
                }
                raw.clamp(0., 1.)
            }
        };
        if decoded.is_finite() {
            Ok(decoded)
        } else {
            Err(ColorError)
        }
    }
}

/// Validated custom RGB definition with relative XYZ/D65 colorants.
/// Matrix rows multiply column RGB vectors. White is normalized to Y=1.
/// Use `new_with_white_point` to explicitly adapt D50 colorants; `new_d65`
/// accepts already-adapted colorants. Adaptation must not be applied twice.
/// This is neither an ICC profile nor a display calibration descriptor.
#[derive(Clone, Debug, PartialEq)]
pub struct RgbDefinition {
    to_xyz_d65: [[f64; 3]; 3],
    curves: [RgbCurve; 3],
}
impl RgbDefinition {
    pub fn new_d65(to_xyz_d65: [[f64; 3]; 3], curves: [RgbCurve; 3]) -> Result<Self> {
        Self::validate_d65(to_xyz_d65, curves, [3. / 65536.; 3])
    }
    /// Construct from relative RGB → XYZ colorants in the declared matrix white.
    /// D50 is Bradford-adapted once to D65. This does not infer ICC tag semantics,
    /// media white, rendering intent, or an existing chromatic-adaptation tag.
    pub fn new_with_white_point(
        colorants: [[f64; 3]; 3],
        curves: [RgbCurve; 3],
        matrix_white: WhitePoint,
    ) -> Result<Self> {
        if matrix_white == WhitePoint::D65 {
            return Self::new_d65(colorants, curves);
        }
        Self::new_with_xyz_white(colorants, curves, matrix_white.xyz())
    }
    /// Explicit encoded/source XYZ white, normalized to Y=1. Colorants are
    /// already relative to that white; do not apply an ICC chad tag again.
    pub fn new_with_xyz_white(
        colorants: [[f64; 3]; 3],
        curves: [RgbCurve; 3],
        matrix_white: [f64; 3],
    ) -> Result<Self> {
        let adaptation = BradfordAdaptation::from_xyz_whites(matrix_white, WhitePoint::D65.xyz())?;
        for row in colorants {
            finite(row)?;
        }
        let white = finite(mul(colorants, [1.; 3]))?;
        if white
            .iter()
            .zip(matrix_white)
            .any(|(a, b)| (a - b).abs() > 3. / 65536.)
        {
            return Err(ColorError);
        }
        let adapted = adaptation.apply_colorants(colorants)?;
        // Propagate admitted source colorant quantization through the CAT;
        // it is not a conversion-error allowance or a silent renormalization.
        let tolerance = adaptation
            .matrix()
            .map(|row| row.iter().map(|v| v.abs()).sum::<f64>() * 3. / 65536. + 16. * f64::EPSILON);
        Self::validate_d65(adapted, curves, tolerance)
    }
    fn validate_d65(
        to_xyz_d65: [[f64; 3]; 3],
        curves: [RgbCurve; 3],
        white_tolerance: [f64; 3],
    ) -> Result<Self> {
        for row in to_xyz_d65 {
            finite(row)?;
        }
        for curve in &curves {
            curve.validate()?;
        }
        // Three independently quantized s15Fixed16 colorants can contribute
        // at most 3/65536 absolute white error. This is metadata admission,
        // not a tolerance for conversion results.
        let white = finite(mul(to_xyz_d65, [1.; 3]))?;
        let d65 = WhitePoint::D65.xyz();
        if white
            .iter()
            .zip(d65)
            .zip(white_tolerance)
            .any(|((a, b), tolerance)| (a - b).abs() > tolerance)
        {
            return Err(ColorError);
        }
        let inv = inverse(to_xyz_d65);
        for row in inv {
            finite(row)?;
        }
        let norm = |m: [[f64; 3]; 3]| {
            m.map(|r| r.iter().map(|v| v.abs()).sum::<f64>())
                .into_iter()
                .fold(0., f64::max)
        };
        // Reject matrices whose condition estimate exhausts practical f64 precision.
        if norm(to_xyz_d65) * norm(inv) > 1. / (128. * f64::EPSILON) {
            return Err(ColorError);
        }
        Ok(Self { to_xyz_d65, curves })
    }
    pub fn linear_to_xyz_d65_matrix(&self) -> [[f64; 3]; 3] {
        self.to_xyz_d65
    }
    /// Conservative retained metadata footprint, including immutable sample tables.
    pub fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + 2 * std::mem::size_of::<usize>()
            + self
                .curves
                .iter()
                .map(|curve| match curve {
                    RgbCurve::Sampled(table) => {
                        std::mem::size_of_val(table.samples()) + 2 * std::mem::size_of::<usize>()
                    }
                    _ => 0,
                })
                .sum::<usize>()
    }
    pub fn curves(&self) -> &[RgbCurve; 3] {
        &self.curves
    }
    /// Decode according to each curve domain. Named curves retain finite
    /// extended RGB; sampled/parametric curves use bounded 0..1 input/output.
    /// XYZ is not clipped; overflow is an error. Alpha stays with the caller.
    pub fn decode_to_xyz_d65(&self, rgb: [f64; 3]) -> Result<XyzD65> {
        finite(rgb)?;
        let linear = [
            self.curves[0].decode(rgb[0])?,
            self.curves[1].decode(rgb[1])?,
            self.curves[2].decode(rgb[2])?,
        ];
        finite(linear)?;
        XyzD65::new(mul(self.to_xyz_d65, linear))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn matrix() -> [[f64; 3]; 3] {
        let x = 3127. / 3290.;
        let z = 3583. / 3290.;
        [
            [x * 0.75, x * 0.25, 0.],
            [0., 0.5, 0.5],
            [z * 0.25, 0., z * 0.75],
        ]
    }
    #[test]
    fn independent_channel_curves_and_rational_xyz_reference() {
        let definition = RgbDefinition::new_d65(
            matrix(),
            [RgbCurve::Gamma(2.), RgbCurve::Gamma(3.), RgbCurve::Linear],
        )
        .unwrap();
        // Exact half inputs decode to [1/4,1/8,1/2]. The rational
        // reference is independently evaluated from the synthetic colorants.
        let reference = [21889. / 105280., 5. / 16., 25081. / 52640.];
        for sign in [-1., 1.] {
            let actual = definition
                .decode_to_xyz_d65([sign * 0.5; 3])
                .unwrap()
                .components();
            for (a, r) in actual.into_iter().zip(reference) {
                assert!((a - sign * r).abs() <= 8. * f64::EPSILON * r.abs());
            }
        }
        assert_eq!(
            definition.curves(),
            &[RgbCurve::Gamma(2.), RgbCurve::Gamma(3.), RgbCurve::Linear]
        );
    }
    #[test]
    fn extended_subnormal_zero_and_overflow() {
        let m = RgbSpace::DisplayP3.linear_to_xyz_matrix();
        let linear = RgbDefinition::new_d65(m, [const { RgbCurve::Linear }; 3]).unwrap();
        let actual = linear.decode_to_xyz_d65([2.; 3]).unwrap().components();
        for (a, r) in actual.into_iter().zip([6254. / 3290., 2., 7166. / 3290.]) {
            assert!((a - r).abs() <= 8. * f64::EPSILON * r);
        }
        assert_eq!(
            linear.decode_to_xyz_d65([0.; 3]).unwrap().components(),
            [0.; 3]
        );
        assert!(
            linear
                .decode_to_xyz_d65([f64::from_bits(1); 3])
                .unwrap()
                .components()
                .iter()
                .all(|v| v.is_finite())
        );
        let squared =
            RgbDefinition::new_d65(m, std::array::from_fn(|_| RgbCurve::Gamma(2.))).unwrap();
        assert!(squared.decode_to_xyz_d65([f64::MAX; 3]).is_err());
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(linear.decode_to_xyz_d65([value, 0., 0.]).is_err());
        }
    }
    #[test]
    fn rejects_invalid_metadata_and_singular_colorants() {
        for gamma in [0., -1., f64::NAN, f64::INFINITY] {
            assert!(
                RgbDefinition::new_d65(matrix(), std::array::from_fn(|_| RgbCurve::Gamma(gamma)))
                    .is_err()
            );
        }
        let mut m = matrix();
        m[0][0] = f64::NAN;
        assert!(RgbDefinition::new_d65(m, [const { RgbCurve::Linear }; 3]).is_err());
        let white = [3127. / 3290., 1., 3583. / 3290.];
        let singular = white.map(|v| [v, 0., 0.]);
        assert!(RgbDefinition::new_d65(singular, [const { RgbCurve::Linear }; 3]).is_err());
        let near_singular = white.map(|v| [v * (1. - 2e-16), v * 1e-16, v * 1e-16]);
        assert!(RgbDefinition::new_d65(near_singular, [const { RgbCurve::Linear }; 3]).is_err());
        assert!(
            RgbDefinition::new_d65(
                [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
                [const { RgbCurve::Linear }; 3]
            )
            .is_err()
        );
    }
    #[test]
    fn srgb_curve_independent_midpoint() {
        let definition = RgbDefinition::new_d65(
            RgbSpace::Srgb.linear_to_xyz_matrix(),
            [const { RgbCurve::Srgb }; 3],
        )
        .unwrap();
        // 120-digit reference for ((0.5+0.055)/1.055)^2.4.
        let y = definition.decode_to_xyz_d65([0.5; 3]).unwrap().components()[1];
        assert!((y - 0.21404114048223244).abs() <= 8. * f64::EPSILON * 0.21404114048223244);
    }
}
