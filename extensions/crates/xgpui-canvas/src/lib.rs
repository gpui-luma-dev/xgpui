//! BETA spatial viewport: f64 camera, retained scene culling, snapping and
//! procedural grids. Expect significant API changes.
//!
//! Screen coordinates are viewport-local logical pixels.
//! World coordinates stay in f64; convert to f32 only after projection.

#[cfg(feature = "gpui")]
mod viewport;
#[cfg(feature = "gpui")]
pub use viewport::{Canvas, GridOptions, GridPainter, GridStyle};
#[cfg(feature = "kurbo")]
mod kurbo_interop;
mod input;
pub use input::PanGesture;
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
    /// World-space point at the viewport center.
    pub fn center(self) -> Point {
        self.center
    }

    /// Fit finite, ordered bounds into a positive viewport with uniform logical-pixel
    /// padding, using the default zoom limits (0.01..=64).
    /// Returns false without changing the camera if fitting is invalid or requires
    /// zoom below the minimum. A point-sized target uses the maximum zoom.
    pub fn fit_bounds(&mut self, bounds: WorldBounds, viewport: Point, padding: f64) -> bool {
        self.fit_bounds_with_limits(bounds, viewport, padding, 0.01, 64.)
    }

    /// Fit bounds with application-selected positive finite zoom limits.
    /// The maximum may leave extra space; the minimum never silently crops the target.
    pub fn fit_bounds_with_limits(
        &mut self,
        bounds: WorldBounds,
        viewport: Point,
        padding: f64,
        min: f64,
        max: f64,
    ) -> bool {
        if !bounds.is_valid()
            || ![viewport.x, viewport.y, padding, min, max]
                .iter()
                .all(|v| v.is_finite())
            || viewport.x <= 0.
            || viewport.y <= 0.
            || padding < 0.
            || min <= 0.
            || max < min
        {
            return false;
        }
        let available = Point::new(
            (viewport.x * 0.5 - padding) * 2.,
            (viewport.y * 0.5 - padding) * 2.,
        );
        if available.x <= 0. || available.y <= 0. {
            return false;
        }
        let midpoint = |lo: f64, hi: f64| {
            let sum = lo + hi;
            if sum.is_finite() {
                sum * 0.5
            } else {
                lo * 0.5 + hi * 0.5
            }
        };
        let center = Point::new(
            midpoint(bounds.min.x, bounds.max.x),
            midpoint(bounds.min.y, bounds.max.y),
        );
        // Use the actual representable center: a midpoint can round all the way
        // to one endpoint of a tiny span at a distant world coordinate.
        let fit_axis = |lo: f64, hi: f64, center: f64, pixels: f64| {
            (pixels * 0.5) / (center - lo).max(hi - center)
        };
        let zoom = fit_axis(bounds.min.x, bounds.max.x, center.x, available.x).min(fit_axis(
            bounds.min.y,
            bounds.max.y,
            center.y,
            available.y,
        ));
        if zoom < min {
            return false;
        }
        self.center = center;
        self.zoom = zoom.min(max);
        true
    }

    /// Project both corners, preserving the ordinary point-projection semantics.
    /// Input and projected coordinates must be representable; no clamping is applied.
    pub fn world_bounds_to_screen(self, bounds: WorldBounds, viewport: Point) -> WorldBounds {
        WorldBounds {
            min: self.world_to_screen(bounds.min, viewport),
            max: self.world_to_screen(bounds.max, viewport),
        }
    }

    /// Inverse-project both corners, without validation or clamping.
    pub fn screen_bounds_to_world(self, bounds: WorldBounds, viewport: Point) -> WorldBounds {
        WorldBounds {
            min: self.screen_to_world(bounds.min, viewport),
            max: self.screen_to_world(bounds.max, viewport),
        }
    }

    /// Absolute affine coefficients [a, b, c, d, e, f], with x' = a*x + c*y + e.
    /// Returns None if viewport/coefficients are non-finite. Absolute transforms can
    /// lose local precision at distant centers; use point projection for those views.
    pub fn world_to_screen_affine(self, viewport: Point) -> Option<[f64; 6]> {
        let coefficients = [
            self.zoom,
            0.,
            0.,
            self.zoom,
            viewport.x * 0.5 - self.center.x * self.zoom,
            viewport.y * 0.5 - self.center.y * self.zoom,
        ];
        (viewport.x.is_finite()
            && viewport.y.is_finite()
            && coefficients.iter().all(|v| v.is_finite()))
        .then_some(coefficients)
    }

    /// Inverse absolute affine coefficients, with the same precision caveat.
    pub fn screen_to_world_affine(self, viewport: Point) -> Option<[f64; 6]> {
        let scale = 1. / self.zoom;
        let coefficients = [
            scale,
            0.,
            0.,
            scale,
            self.center.x - viewport.x * 0.5 * scale,
            self.center.y - viewport.y * 0.5 * scale,
        ];
        (viewport.x.is_finite()
            && viewport.y.is_finite()
            && coefficients.iter().all(|v| v.is_finite()))
        .then_some(coefficients)
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
    /// Pan in logical pixels. Non-finite input or an overflowing result leaves both axes unchanged.
    pub fn pan_by(&mut self, delta: Point) {
        if delta.x.is_finite() && delta.y.is_finite() {
            self.center_on(Point::new(
                self.center.x - delta.x / self.zoom,
                self.center.y - delta.y / self.zoom,
            ));
        }
    }
    pub fn zoom_at(&mut self, anchor: Point, viewport: Point, factor: f64) {
        self.zoom_at_with_limits(anchor, viewport, factor, 0.01, 64.);
    }
    /// Pointer-anchored zoom with application-selected finite, positive limits.
    /// Non-finite inputs or unrepresentable anchor/center coordinates leave the entire camera unchanged.
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
        if !viewport.x.is_finite() || !viewport.y.is_finite() {
            return;
        }
        let before = self.screen_to_world(anchor, viewport);
        let mut next = *self;
        next.zoom = (self.zoom * factor).clamp(min, max);
        let after = next.screen_to_world(anchor, viewport);
        if ![before.x, before.y, after.x, after.y]
            .iter()
            .all(|v| v.is_finite())
        {
            return;
        }
        if next.center_on(Point::new(
            self.center.x + (before.x - after.x),
            self.center.y + (before.y - after.y),
        )) {
            *self = next;
        }
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
    fn bounds(min: Point, max: Point) -> WorldBounds {
        WorldBounds { min, max }
    }

    #[test]
    fn fit_bounds_centers_and_respects_padding() {
        let mut camera = Camera2D::default();
        let target = bounds(Point::new(100., -50.), Point::new(300., 50.));
        let viewport = Point::new(1000., 600.);
        assert!(camera.fit_bounds(target, viewport, 50.));
        assert_eq!(camera.center(), Point::new(200., 0.));
        assert_eq!(camera.zoom(), 4.5);
        let projected = camera.world_bounds_to_screen(target, viewport);
        close(projected.min, Point::new(50., 75.));
        close(projected.max, Point::new(950., 525.));
        let back = camera.screen_bounds_to_world(projected, viewport);
        close(back.min, target.min);
        close(back.max, target.max);
    }

    #[test]
    fn fit_bounds_handles_points_lines_and_zoom_limits() {
        let mut camera = Camera2D::default();
        let viewport = Point::new(100., 100.);
        let point = Point::new(7., -9.);
        assert!(camera.fit_bounds(bounds(point, point), viewport, 0.));
        assert_eq!(camera.center(), point);
        assert_eq!(camera.zoom(), 64.);
        assert!(camera.fit_bounds(bounds(point, Point::new(7., 91.)), viewport, 10.));
        assert_eq!(camera.zoom(), 0.8);
        let before = camera;
        let large = bounds(Point::default(), Point::new(100000., 100000.));
        assert!(!camera.fit_bounds(large, viewport, 0.));
        assert_eq!(camera.center(), before.center());
        assert_eq!(camera.zoom(), before.zoom());
        assert!(camera.fit_bounds_with_limits(large, viewport, 0., 1e-6, 64.));
        assert_eq!(camera.zoom(), 0.001);
        assert!(camera.fit_bounds_with_limits(bounds(point, point), viewport, 0., 1., 2.));
        assert_eq!(camera.zoom(), 2.);
    }

    #[test]
    fn fit_bounds_rejects_invalid_inputs_atomically() {
        let target = bounds(Point::default(), Point::new(10., 10.));
        for (target, viewport, padding, min, max) in [
            (target, Point::new(100., 100.), 50., 0.01, 64.),
            (target, Point::new(f64::NAN, 100.), 0., 0.01, 64.),
            (target, Point::new(0., 100.), 0., 0.01, 64.),
            (target, Point::new(100., 100.), -1., 0.01, 64.),
            (target, Point::new(100., 100.), f64::INFINITY, 0.01, 64.),
            (target, Point::new(100., 100.), 0., 2., 1.),
            (target, Point::new(100., 100.), 0., 0., 1.),
            (
                bounds(Point::new(2., 0.), Point::new(1., 1.)),
                Point::new(100., 100.),
                0.,
                0.01,
                64.,
            ),
        ] {
            let mut camera = Camera2D::default();
            camera.center_on(Point::new(4., 5.));
            assert!(!camera.fit_bounds_with_limits(target, viewport, padding, min, max));
            assert_eq!(camera.center(), Point::new(4., 5.));
            assert_eq!(camera.zoom(), 1.);
        }
    }

    #[test]
    fn fit_extreme_finite_and_subnormal_bounds() {
        let mut camera = Camera2D::default();
        let target = bounds(
            Point::new(-f64::MAX, -f64::MAX),
            Point::new(f64::MAX, f64::MAX),
        );
        assert!(camera.fit_bounds_with_limits(target, Point::new(1000., 1000.), 0., 1e-310, 64.));
        close(
            camera.world_to_screen(target.min, Point::new(1000., 1000.)),
            Point::default(),
        );
        close(
            camera.world_to_screen(target.max, Point::new(1000., 1000.)),
            Point::new(1000., 1000.),
        );
        let unit = f64::from_bits(1);
        let target = bounds(Point::new(unit, unit), Point::new(2. * unit, 2. * unit));
        assert!(camera.fit_bounds(target, Point::new(100., 100.), 0.));
        assert_eq!(camera.center(), Point::new(2. * unit, 2. * unit));
        assert_eq!(camera.zoom(), 64.);
    }

    #[test]
    fn fitted_view_contains_bounds_when_midpoint_rounds_to_an_endpoint() {
        let lo = 1e10_f64;
        let hi = f64::from_bits(lo.to_bits() + 1);
        let target = bounds(Point::new(lo, lo), Point::new(hi, hi));
        let viewport = Point::new(1000., 1000.);
        let mut camera = Camera2D::default();
        assert!(camera.fit_bounds_with_limits(target, viewport, 50., 1e-6, 1e10));
        let screen = camera.world_bounds_to_screen(target, viewport);
        assert!(screen.min.x >= 50. - 1e-8 && screen.min.y >= 50. - 1e-8);
        assert!(
            screen.max.x <= 950. + 1e-8 && screen.max.y <= 950. + 1e-8,
            "{screen:?}"
        );
    }

    #[test]
    fn affine_coefficients_match_projection_and_reject_overflow() {
        let mut camera = Camera2D::default();
        let viewport = Point::new(1000., 600.);
        camera.center_on(Point::new(123., -45.));
        camera.zoom_at(Point::new(500., 300.), viewport, 3.);
        let apply = |m: [f64; 6], p: Point| {
            Point::new(
                m[0] * p.x + m[2] * p.y + m[4],
                m[1] * p.x + m[3] * p.y + m[5],
            )
        };
        for p in [Point::default(), Point::new(200., -100.)] {
            let screen = camera.world_to_screen(p, viewport);
            close(
                apply(camera.world_to_screen_affine(viewport).unwrap(), p),
                screen,
            );
            close(
                apply(camera.screen_to_world_affine(viewport).unwrap(), screen),
                p,
            );
        }
        camera.center_on(Point::new(f64::MAX, 0.));
        assert!(camera.world_to_screen_affine(viewport).is_none());
        assert!(
            camera
                .screen_to_world_affine(Point::new(f64::NAN, 0.))
                .is_none()
        );
        let mut tiny = Camera2D::default();
        tiny.zoom_at_with_limits(Point::default(), Point::default(), 1e-310, 1e-310, 1.);
        assert!(tiny.screen_to_world_affine(viewport).is_none());
    }

    #[test]
    fn overflowing_pan_leaves_both_axes_unchanged() {
        let mut camera = Camera2D::default();
        camera.center_on(Point::new(f64::MAX, 17.));
        camera.pan_by(Point::new(-f64::MAX, 5.));
        assert_eq!(camera.center, Point::new(f64::MAX, 17.));
        camera.center_on(Point::default());
        camera.zoom_at_with_limits(Point::default(), Point::default(), 1e-310, 1e-310, 1.);
        camera.pan_by(Point::new(1., 1.));
        assert_eq!(camera.center, Point::default());
    }

    #[test]
    fn invalid_or_unrepresentable_zoom_is_atomic() {
        for viewport in [Point::new(f64::NAN, 100.), Point::new(100., f64::INFINITY)] {
            let mut camera = Camera2D::default();
            camera.zoom_at(Point::default(), viewport, 2.);
            assert_eq!(camera.center, Point::default());
            assert_eq!(camera.zoom(), 1.);
        }
        let mut camera = Camera2D::default();
        camera.zoom_at_with_limits(Point::new(1., 1.), Point::default(), 1e-310, 1e-310, 1.);
        assert_eq!(camera.center, Point::default());
        assert_eq!(camera.zoom(), 1.);
        // An unrepresentable old anchor must not contaminate a representable new zoom.
        camera.zoom_at_with_limits(Point::default(), Point::default(), 1e-310, 1e-310, 1.);
        camera.zoom_at_with_limits(Point::new(1., 1.), Point::default(), 1e308, 1e-310, 1.);
        assert_eq!(camera.center, Point::default());
        assert_eq!(camera.zoom(), 1e-310);
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
