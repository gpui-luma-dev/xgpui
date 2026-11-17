//! SVG path data (`d` attribute) parsing and serialization, through kurbo:
//! absolute and relative commands, shorthand forms (`S`, `T`, `H`, `V`) and
//! elliptical arcs (converted to cubics).

use crate::error::CurveError;
use crate::geom::BezPath;

/// Parse SVG path data.
pub fn parse(d: &str) -> Result<BezPath, CurveError> {
    BezPath::from_svg(d).map_err(|e| CurveError::Svg(e.to_string()))
}

/// Serialize as SVG path data with absolute commands.
pub fn to_svg(path: &BezPath) -> String {
    path.to_svg()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::{ParamCurve, PathEl, Point};

    #[test]
    fn round_trips_every_command_kind() {
        let d = "M10 10 L20 10 H30 V20 l5 5 h-5 v5 Q40 40 50 30 T70 30 C80 20 90 40 100 30 S120 20 130 30 A10 15 30 0 1 150 30 Z m5 5 z";
        let path = parse(d).unwrap();
        let again = parse(&to_svg(&path)).unwrap();
        assert_eq!(path.elements().len(), again.elements().len());
        for (a, b) in path.segments().zip(again.segments()) {
            for i in 0..=4 {
                let t = i as f64 / 4.;
                assert!((a.eval(t) - b.eval(t)).hypot() < 1e-9);
            }
        }
        // Relative and shorthand commands resolve to absolute points.
        assert!(
            path.elements()
                .iter()
                .any(|e| *e == PathEl::LineTo(Point::new(35., 25.)))
        );
        assert!(matches!(path.elements().last(), Some(PathEl::ClosePath)));
    }

    #[test]
    fn invalid_data_is_an_error() {
        assert!(matches!(parse("M 10 10 L"), Err(CurveError::Svg(_))));
        assert!(parse("").unwrap().elements().is_empty());
    }
}
