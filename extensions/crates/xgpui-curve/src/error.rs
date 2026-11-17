//! Error type shared by fallible constructors and path effects.

use std::fmt;

/// Why a curve operation was rejected.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum CurveError {
    /// An input coordinate, parameter or length was NaN or infinite.
    NonFinite,
    /// An input was outside its documented range; the message names it.
    OutOfRange(&'static str),
    /// Too few points for the requested construction.
    TooFewPoints {
        /// Points required.
        required: usize,
        /// Points supplied.
        actual: usize,
    },
    /// Knot x coordinates must strictly increase.
    NotIncreasing,
    /// The operation would exceed a documented work bound.
    WorkLimit(&'static str),
    /// Grid dimensions do not match the supplied data.
    Dimensions,
    /// SVG path data could not be parsed.
    Svg(String),
}

impl fmt::Display for CurveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFinite => write!(f, "non-finite curve input"),
            Self::OutOfRange(what) => write!(f, "{what} is out of range"),
            Self::TooFewPoints { required, actual } => {
                write!(f, "needs at least {required} points, got {actual}")
            }
            Self::NotIncreasing => write!(f, "knot x coordinates must strictly increase"),
            Self::WorkLimit(what) => write!(f, "work limit exceeded: {what}"),
            Self::Dimensions => write!(f, "grid dimensions do not match the data"),
            Self::Svg(message) => write!(f, "invalid SVG path data: {message}"),
        }
    }
}

impl std::error::Error for CurveError {}

#[cfg(any(
    feature = "measure",
    feature = "func",
    feature = "mesh",
    feature = "spine",
    feature = "node",
    feature = "shape",
    feature = "stroke",
    feature = "fit"
))]
pub(crate) fn finite(values: &[f64]) -> Result<(), CurveError> {
    if values.iter().all(|v| v.is_finite()) {
        Ok(())
    } else {
        Err(CurveError::NonFinite)
    }
}
