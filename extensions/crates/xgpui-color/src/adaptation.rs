//! Explicit Bradford adaptation between supported reference whites.
use crate::{Result, finite, inverse, mul};

/// White of the supplied XYZ/colorant matrix, not a monitor mode or ICC label.
/// D50 uses xy=(0.3457,0.3585); D65 uses xy=(0.3127,0.3290).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WhitePoint {
    D50,
    D65,
}
impl WhitePoint {
    /// Relative XYZ normalized to Y=1.
    pub fn xyz(self) -> [f64; 3] {
        let (x, y) = match self {
            Self::D50 => (0.3457, 0.3585),
            Self::D65 => (0.3127, 0.3290),
        };
        [x / y, 1., (1. - x - y) / y]
    }
}

/// Linear chromatic adaptation only. No clipping, transfer curves or alpha.
/// D50/D65 agree with CSS Color 4's Bradford conversion convention.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BradfordAdaptation {
    matrix: [[f64; 3]; 3],
    identity: bool,
}
impl BradfordAdaptation {
    pub fn new(source: WhitePoint, destination: WhitePoint) -> Self {
        if source == destination {
            return Self {
                matrix: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
                identity: true,
            };
        }
        let cone = [
            [0.8951, 0.2664, -0.1614],
            [-0.7502, 1.7135, 0.0367],
            [0.0389, -0.0685, 1.0296],
        ];
        let a = mul(cone, source.xyz());
        let b = mul(cone, destination.xyz());
        let scaled: [[f64; 3]; 3] = std::array::from_fn(|i| cone[i].map(|v| v * b[i] / a[i]));
        let inv = inverse(cone);
        let matrix = std::array::from_fn(|i| {
            std::array::from_fn(|j| (0..3).map(|k| inv[i][k] * scaled[k][j]).sum())
        });
        Self {
            matrix,
            identity: false,
        }
    }
    pub fn matrix(self) -> [[f64; 3]; 3] {
        self.matrix
    }
    /// Adapt finite relative XYZ; negative and above-one values are retained.
    /// Reject nonfinite inputs and arithmetic overflow.
    pub fn apply(self, xyz: [f64; 3]) -> Result<[f64; 3]> {
        finite(xyz)?;
        if self.identity {
            return Ok(xyz);
        }
        finite(mul(self.matrix, xyz))
    }
    /// Adapt each column of a row-major RGB → XYZ colorant matrix.
    /// The supplied matrix must be relative to this adaptation's source white.
    pub fn apply_colorants(self, matrix: [[f64; 3]; 3]) -> Result<[[f64; 3]; 3]> {
        for row in matrix {
            finite(row)?;
        }
        let mut result = [[0.; 3]; 3];
        for column in 0..3 {
            let adapted = self.apply(std::array::from_fn(|row| matrix[row][column]))?;
            for row in 0..3 {
                result[row][column] = adapted[row];
            }
        }
        Ok(result)
    }
}
