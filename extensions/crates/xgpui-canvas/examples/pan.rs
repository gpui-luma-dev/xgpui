//! A second, headless consumer: gesture policy does not require card IDs or GPUI.
use xgpui_canvas::{Camera2D, PanGesture, Point};

fn main() {
    let mut camera = Camera2D::default();
    let mut gesture = PanGesture::default();
    gesture.begin(Point::new(10., 10.));
    if let Some(delta) = gesture.update(Point::new(40., 25.)) {
        camera.pan_by(delta);
    }
    gesture.end(); // Release, Escape, focus loss or another owner taking over.
    assert!(gesture.update(Point::new(100., 100.)).is_none());
    assert_eq!(
        camera.world_to_screen(Point::default(), Point::new(100., 100.)),
        Point::new(80., 65.)
    );
}
