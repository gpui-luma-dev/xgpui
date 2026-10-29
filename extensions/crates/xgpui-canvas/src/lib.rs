//! Experimental camera math. Screen coordinates are viewport-local logical pixels.
//! World coordinates stay in f64; convert to f32 only after projection.

#[cfg(feature = "gpui")]
mod viewport;
#[cfg(feature = "gpui")]
pub use viewport::{Canvas, GridPainter, GridStyle};
mod overview;
mod scene;
mod snapping;
mod spatial;
pub use overview::{Overview, OverviewCell};
pub use scene::{LevelOfDetail, Scene, SceneItem, VisibleItem};
pub use snapping::{AlignmentGuide, Axis, SnapResult};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct WorldBounds {
    pub min: Point,
    pub max: Point,
}

impl WorldBounds {
    pub fn translated(self, delta: Point) -> Self {
        Self {
            min: Point::new(self.min.x + delta.x, self.min.y + delta.y),
            max: Point::new(self.max.x + delta.x, self.max.y + delta.y),
        }
    }
    pub fn is_valid(self) -> bool {
        [self.min.x, self.min.y, self.max.x, self.max.y]
            .iter()
            .all(|v| v.is_finite())
            && self.min.x <= self.max.x
            && self.min.y <= self.max.y
    }
    pub fn contains(self, point: Point) -> bool {
        point.x >= self.min.x
            && point.x <= self.max.x
            && point.y >= self.min.y
            && point.y <= self.max.y
    }
    pub fn intersects(self, other: Self) -> bool {
        self.min.x <= other.max.x
            && self.max.x >= other.min.x
            && self.min.y <= other.max.y
            && self.max.y >= other.min.y
    }
}

/// Translation and uniform scale; rotation is intentionally deferred.
#[derive(Clone, Copy, Debug)]
pub struct Camera2D {
    center: Point,
    zoom: f64,
}

impl Default for Camera2D {
    fn default() -> Self {
        Self {
            center: Point::default(),
            zoom: 1.0,
        }
    }
}

impl Camera2D {
    /// Recenter without changing zoom. Invalid coordinates leave the camera unchanged.
    pub fn center_on(&mut self, world: Point) -> bool {
        if !world.x.is_finite() || !world.y.is_finite() {
            return false;
        }
        self.center = world;
        true
    }
    pub fn zoom(self) -> f64 {
        self.zoom
    }
    pub fn world_to_screen(self, world: Point, viewport: Point) -> Point {
        Point::new(
            (world.x - self.center.x) * self.zoom + viewport.x / 2.,
            (world.y - self.center.y) * self.zoom + viewport.y / 2.,
        )
    }
    pub fn screen_to_world(self, screen: Point, viewport: Point) -> Point {
        Point::new(
            self.center.x + (screen.x - viewport.x / 2.) / self.zoom,
            self.center.y + (screen.y - viewport.y / 2.) / self.zoom,
        )
    }
    pub fn pan_by(&mut self, delta: Point) {
        if delta.x.is_finite() && delta.y.is_finite() {
            self.center.x -= delta.x / self.zoom;
            self.center.y -= delta.y / self.zoom;
        }
    }
    pub fn zoom_at(&mut self, anchor: Point, viewport: Point, factor: f64) {
        self.zoom_at_with_limits(anchor, viewport, factor, 0.01, 64.);
    }
    /// Pointer-anchored zoom with application-selected finite, positive limits.
    pub fn zoom_at_with_limits(
        &mut self,
        anchor: Point,
        viewport: Point,
        factor: f64,
        min: f64,
        max: f64,
    ) {
        if !min.is_finite() || !max.is_finite() || min <= 0. || max < min {
            return;
        }
        if !factor.is_finite() || factor <= 0. || !anchor.x.is_finite() || !anchor.y.is_finite() {
            return;
        }
        let before = self.screen_to_world(anchor, viewport);
        self.zoom = (self.zoom * factor).clamp(min, max);
        let after = self.screen_to_world(anchor, viewport);
        self.center.x += before.x - after.x;
        self.center.y += before.y - after.y;
    }
    pub fn visible_world_bounds(self, viewport: Point) -> WorldBounds {
        WorldBounds {
            min: self.screen_to_world(Point::default(), viewport),
            max: self.screen_to_world(viewport, viewport),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn close(a: Point, b: Point) {
        assert!(
            (a.x - b.x).abs() < 1e-5 && (a.y - b.y).abs() < 1e-5,
            "{a:?} != {b:?}"
        );
    }
    #[test]
    fn extended_zoom_preserves_anchor_and_rejects_invalid_limits() {
        let mut camera = Camera2D::default();
        let viewport = Point::new(1000., 700.);
        let anchor = Point::new(317., 291.);
        let expected = camera.screen_to_world(anchor, viewport);
        camera.zoom_at_with_limits(anchor, viewport, 1e12, 0.01, 1e10);
        assert_eq!(camera.zoom(), 1e10);
        close(camera.screen_to_world(anchor, viewport), expected);
        camera.zoom_at_with_limits(anchor, viewport, 0.5, 2., 1.);
        assert_eq!(camera.zoom(), 1e10);
        camera.zoom_at(anchor, viewport, 1.);
        assert_eq!(camera.zoom(), 64.);
    }
    #[test]
    fn projection_and_anchor_survive_zoom_limits_and_large_world() {
        let mut camera = Camera2D::default();
        let viewport = Point::new(1000., 700.);
        camera.pan_by(Point::new(1e10, -1e10));
        let anchor = Point::new(137., 492.);
        let world = camera.screen_to_world(anchor, viewport);
        for factor in [2., 1e8, 1e-12, 3.] {
            camera.zoom_at(anchor, viewport, factor);
            close(camera.screen_to_world(anchor, viewport), world);
            close(camera.world_to_screen(world, viewport), anchor);
        }
    }
    #[test]
    fn culling_includes_edges_and_pan_moves_content() {
        let mut camera = Camera2D::default();
        let viewport = Point::new(100., 100.);
        camera.pan_by(Point::new(10., 20.));
        close(
            camera.world_to_screen(Point::default(), viewport),
            Point::new(60., 70.),
        );
        let visible = camera.visible_world_bounds(viewport);
        assert!(visible.intersects(WorldBounds {
            min: visible.max,
            max: visible.max
        }));
        assert!(!visible.intersects(WorldBounds {
            min: Point::new(1000., 1000.),
            max: Point::new(1100., 1100.)
        }));
    }
    #[test]
    fn invalid_input_does_not_poison_camera() {
        let mut camera = Camera2D::default();
        camera.zoom_at(Point::default(), Point::new(100., 100.), f64::NAN);
        camera.pan_by(Point::new(f64::INFINITY, 0.));
        assert_eq!(camera.zoom(), 1.);
        assert_eq!(camera.center, Point::default());
    }

    #[test]
    fn resize_preserves_world_center_and_updates_culling_bounds() {
        let mut camera = Camera2D::default();
        camera.pan_by(Point::new(123., -456.));
        let center = camera.screen_to_world(Point::new(500., 350.), Point::new(1000., 700.));
        for viewport in [
            Point::new(800., 600.),
            Point::new(1600., 1200.),
            Point::default(),
        ] {
            close(
                camera.screen_to_world(Point::new(viewport.x / 2., viewport.y / 2.), viewport),
                center,
            );
            let bounds = camera.visible_world_bounds(viewport);
            close(
                camera.world_to_screen(bounds.min, viewport),
                Point::default(),
            );
            close(camera.world_to_screen(bounds.max, viewport), viewport);
        }
    }
}
