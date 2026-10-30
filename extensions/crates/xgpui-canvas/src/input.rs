use crate::Point;

/// Pointer pan state in viewport-local logical pixels. The caller owns button,
/// modifier and focus policy. Call `end` on release, cancellation or focus loss.
#[derive(Clone, Copy, Debug, Default)]
pub struct PanGesture {
    previous: Option<Point>,
}

impl PanGesture {
    pub fn begin(&mut self, position: Point) {
        self.previous = finite(position).then_some(position);
    }

    /// Returns a screen-space delta only while a valid gesture is active.
    /// Invalid samples end the gesture so the next sample cannot jump the camera.
    pub fn update(&mut self, position: Point) -> Option<Point> {
        if !finite(position) {
            self.end();
            return None;
        }
        let previous = self.previous.as_mut()?;
        let delta = Point::new(position.x - previous.x, position.y - previous.y);
        *previous = position;
        Some(delta)
    }

    pub fn end(&mut self) {
        self.previous = None;
    }

    pub fn is_active(&self) -> bool {
        self.previous.is_some()
    }
}

fn finite(point: Point) -> bool {
    point.x.is_finite() && point.y.is_finite()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_cancel_and_invalid_samples_prevent_unowned_updates() {
        let mut gesture = PanGesture::default();
        assert!(gesture.update(Point::new(10., 20.)).is_none());
        gesture.begin(Point::new(10., 20.));
        assert_eq!(
            gesture.update(Point::new(14., 18.)),
            Some(Point::new(4., -2.))
        );
        gesture.end();
        assert!(!gesture.is_active());
        assert!(gesture.update(Point::new(100., 200.)).is_none());
        gesture.begin(Point::default());
        assert!(gesture.update(Point::new(f64::NAN, 0.)).is_none());
        assert!(!gesture.is_active());
    }
}
