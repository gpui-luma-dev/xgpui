use crate::{Camera2D, Point, WorldBounds};

/// Suggested presentation based on the longest projected side, in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LevelOfDetail {
    /// Less than 4px: a small proxy, without text or layout.
    Point,
    /// Less than 48px: simplified bounds, without text or layout.
    Bounds,
    Full,
}

impl LevelOfDetail {
    pub fn for_size(width: f64, height: f64) -> Self {
        match width.max(height) {
            side if side < 4. => Self::Point,
            side if side < 48. => Self::Bounds,
            _ => Self::Full,
        }
    }
}

/// World bounds and caller-owned data, retained independently of GPUI elements.
pub struct SceneItem<T> {
    pub bounds: WorldBounds,
    pub data: T,
}

pub struct VisibleItem<'a, T> {
    pub item: &'a SceneItem<T>,
    /// Viewport-local logical pixels (f64 until the drawing boundary).
    pub screen_bounds: WorldBounds,
    pub detail: LevelOfDetail,
}

/// Retained scene with bounded linear queries. Order is back-to-front.
///
/// No element construction, layout, or text shaping occurs during queries.
/// A future spatial index can accelerate queries without changing item data.
pub struct Scene<T> {
    items: Vec<SceneItem<T>>,
}

impl<T> Scene<T> {
    pub fn new(items: impl IntoIterator<Item = SceneItem<T>>) -> Self {
        Self {
            items: items.into_iter().collect(),
        }
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn visible(
        &self,
        camera: Camera2D,
        viewport: Point,
    ) -> impl Iterator<Item = VisibleItem<'_, T>> {
        let visible = camera.visible_world_bounds(viewport);
        self.items.iter().filter_map(move |item| {
            if !item.bounds.intersects(visible) {
                return None;
            }
            let min = camera.world_to_screen(item.bounds.min, viewport);
            let max = camera.world_to_screen(item.bounds.max, viewport);
            Some(VisibleItem {
                item,
                screen_bounds: WorldBounds { min, max },
                detail: LevelOfDetail::for_size(max.x - min.x, max.y - min.y),
            })
        })
    }

    /// Topmost AABB hit, in the same order as painting. Points are world-space.
    pub fn hit_test(&self, world: Point) -> Option<&SceneItem<T>> {
        self.items
            .iter()
            .rev()
            .find(|item| item.bounds.contains(world))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn item(min: f64, max: f64, data: usize) -> SceneItem<usize> {
        SceneItem {
            bounds: WorldBounds {
                min: Point::new(min, min),
                max: Point::new(max, max),
            },
            data,
        }
    }
    #[test]
    fn culling_preserves_paint_order_and_hit_test_returns_topmost() {
        let scene = Scene::new([item(-10., 10., 1), item(0., 20., 2), item(1000., 1010., 3)]);
        let visible: Vec<_> = scene
            .visible(Camera2D::default(), Point::new(100., 100.))
            .collect();
        assert_eq!(
            visible
                .iter()
                .map(|entry| entry.item.data)
                .collect::<Vec<_>>(),
            [1, 2]
        );
        assert_eq!(scene.hit_test(Point::new(5., 5.)).unwrap().data, 2);
        assert_eq!(scene.hit_test(Point::new(-10., -10.)).unwrap().data, 1);
        assert!(scene.hit_test(Point::new(50., 50.)).is_none());
        assert_eq!(visible[0].detail, LevelOfDetail::Bounds);
        assert_eq!(visible[0].screen_bounds.min, Point::new(40., 40.));
    }
    #[test]
    fn lod_thresholds_and_zoom_use_logical_pixel_sizes() {
        assert_eq!(LevelOfDetail::for_size(3.99, 1.), LevelOfDetail::Point);
        assert_eq!(LevelOfDetail::for_size(1., 4.), LevelOfDetail::Bounds);
        assert_eq!(LevelOfDetail::for_size(47.99, 2.), LevelOfDetail::Bounds);
        assert_eq!(LevelOfDetail::for_size(2., 48.), LevelOfDetail::Full);
        let scene = Scene::new([item(-80., 80., 1)]);
        let viewport = Point::new(100., 100.);
        let mut camera = Camera2D::default();
        assert_eq!(
            scene.visible(camera, viewport).next().unwrap().detail,
            LevelOfDetail::Full
        );
        camera.zoom_at(Point::new(50., 50.), viewport, 0.1);
        assert_eq!(
            scene.visible(camera, viewport).next().unwrap().detail,
            LevelOfDetail::Bounds
        );
        camera.zoom_at(Point::new(50., 50.), viewport, 0.1);
        assert_eq!(
            scene.visible(camera, viewport).next().unwrap().detail,
            LevelOfDetail::Point
        );
        let empty = Scene::<()>::new([]);
        assert!(empty.is_empty());
        assert_eq!(empty.visible(camera, viewport).count(), 0);
    }
}
