//! Explicit custom RGB metadata. No profile parser or file-format knowledge.
use crate::{ColorError, Result, RgbSpace, XyzD65, finite, inverse, mul};

/// Independent channel decoding curve. Gamma uses sign-preserving powers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RgbCurve {
    Linear,
    Srgb,
    Gamma(f64),
}
impl RgbCurve {
    fn validate(self) -> Result<()> {
        if let Self::Gamma(gamma) = self
            && (!gamma.is_finite() || gamma <= 0.)
        {
            return Err(ColorError);
        }
        Ok(())
    }
    fn decode(self, value: f64) -> f64 {
        match self {
            Self::Linear => value,
            Self::Srgb => RgbSpace::Srgb.transfer(value, true),
            Self::Gamma(gamma) => value.abs().powf(gamma).copysign(value),
        }
    }
}

/// Validated custom RGB definition with relative XYZ/D65 colorants.
/// Matrix rows multiply column RGB vectors. White is normalized to Y=1.
/// An adapter must perform any source-white adaptation before construction.
/// This is neither an ICC profile nor a display calibration descriptor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RgbDefinition {
    to_xyz_d65: [[f64; 3]; 3],
    curves: [RgbCurve; 3],
}
impl RgbDefinition {
    pub fn new_d65(to_xyz_d65: [[f64; 3]; 3], curves: [RgbCurve; 3]) -> Result<Self> {
        for row in to_xyz_d65 {
            finite(row)?;
        }
        for curve in curves {
            curve.validate()?;
        }
        // Three independently quantized s15Fixed16 colorants can contribute
        // at most 3/65536 absolute white error. This is metadata admission,
        // not a tolerance for conversion results.
        let white = mul(to_xyz_d65, [1.; 3]);
        let d65 = [0.3127 / 0.3290, 1., (1. - 0.3127 - 0.3290) / 0.3290];
        if white
            .iter()
            .zip(d65)
            .any(|(a, b)| (a - b).abs() > 3. / 65536.)
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
    pub fn linear_to_xyz_d65_matrix(self) -> [[f64; 3]; 3] {
        self.to_xyz_d65
    }
    pub fn curves(self) -> [RgbCurve; 3] {
        self.curves
    }
    /// Decode finite RGB without clipping. Extended values are retained;
    /// overflow is an error. Alpha is handled by the media caller.
    pub fn decode_to_xyz_d65(self, rgb: [f64; 3]) -> Result<XyzD65> {
        finite(rgb)?;
        let linear = std::array::from_fn(|i| self.curves[i].decode(rgb[i]));
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
            [RgbCurve::Gamma(2.), RgbCurve::Gamma(3.), RgbCurve::Linear]
        );
    }
    #[test]
    fn extended_subnormal_zero_and_overflow() {
        let m = RgbSpace::DisplayP3.linear_to_xyz_matrix();
        let linear = RgbDefinition::new_d65(m, [RgbCurve::Linear; 3]).unwrap();
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
        let squared = RgbDefinition::new_d65(m, [RgbCurve::Gamma(2.); 3]).unwrap();
        assert!(squared.decode_to_xyz_d65([f64::MAX; 3]).is_err());
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(linear.decode_to_xyz_d65([value, 0., 0.]).is_err());
        }
    }
    #[test]
    fn rejects_invalid_metadata_and_singular_colorants() {
        for gamma in [0., -1., f64::NAN, f64::INFINITY] {
            assert!(RgbDefinition::new_d65(matrix(), [RgbCurve::Gamma(gamma); 3]).is_err());
        }
        let mut m = matrix();
        m[0][0] = f64::NAN;
        assert!(RgbDefinition::new_d65(m, [RgbCurve::Linear; 3]).is_err());
        let white = [3127. / 3290., 1., 3583. / 3290.];
        let singular = white.map(|v| [v, 0., 0.]);
        assert!(RgbDefinition::new_d65(singular, [RgbCurve::Linear; 3]).is_err());
        let near_singular = white.map(|v| [v * (1. - 2e-16), v * 1e-16, v * 1e-16]);
        assert!(RgbDefinition::new_d65(near_singular, [RgbCurve::Linear; 3]).is_err());
        assert!(
            RgbDefinition::new_d65(
                [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
                [RgbCurve::Linear; 3]
            )
            .is_err()
        );
    }
    #[test]
    fn srgb_curve_independent_midpoint() {
        let definition =
            RgbDefinition::new_d65(RgbSpace::Srgb.linear_to_xyz_matrix(), [RgbCurve::Srgb; 3])
                .unwrap();
        // 120-digit reference for ((0.5+0.055)/1.055)^2.4.
        let y = definition.decode_to_xyz_d65([0.5; 3]).unwrap().components()[1];
        assert!((y - 0.21404114048223244).abs() <= 8. * f64::EPSILON * 0.21404114048223244);
    }
}
