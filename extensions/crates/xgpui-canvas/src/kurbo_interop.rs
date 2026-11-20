//! Optional coordinate conversions; preserve coordinates without silently validating bounds.
use crate::{Point, WorldBounds};

impl From<Point> for kurbo::Point {
    fn from(p: Point) -> Self {
        Self::new(p.x, p.y)
    }
}
impl From<kurbo::Point> for Point {
    fn from(p: kurbo::Point) -> Self {
        Self::new(p.x, p.y)
    }
}
impl From<WorldBounds> for kurbo::Rect {
    fn from(b: WorldBounds) -> Self {
        Self::new(b.min.x, b.min.y, b.max.x, b.max.y)
    }
}
impl From<kurbo::Rect> for WorldBounds {
    fn from(r: kurbo::Rect) -> Self {
        Self {
            min: Point::new(r.x0, r.y0),
            max: Point::new(r.x1, r.y1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conversions_preserve_extreme_coordinates_and_do_not_normalize_inverted_rectangles() {
        let p = Point::new(-f64::MAX, f64::MAX);
        assert_eq!(Point::from(kurbo::Point::from(p)), p);
        for r in [
            kurbo::Rect::new(-10., -20., 30., 40.),
            kurbo::Rect::new(5., 7., 5., 7.),
            kurbo::Rect::new(10., 20., -30., -40.),
        ] {
            let b = WorldBounds::from(r);
            assert_eq!(kurbo::Rect::from(b), r);
        }
        assert!(!WorldBounds::from(kurbo::Rect::new(10., 20., -30., -40.)).is_valid());
    }
}
