use crate::spatial::SpatialIndex;
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

/// Retained scene with BVH queries. Order is back-to-front.
///
/// No element construction, layout, or text shaping occurs during queries.
/// Bounds updates refit the tree; rebuild when adding/removing items. Dense queries may still
/// visit every item; ordered query results additionally cost O(k log k) to sort.
pub struct Scene<T> {
    items: Vec<SceneItem<T>>,
    index: SpatialIndex,
    revision: u64,
}

impl<T> Scene<T> {
    pub fn new(items: impl IntoIterator<Item = SceneItem<T>>) -> Self {
        let items: Vec<_> = items.into_iter().collect();
        let index = SpatialIndex::new(&items);
        Self {
            items,
            index,
            revision: 0,
        }
    }
    /// Changes whenever bounds are updated; useful for invalidating retained captures.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Current scene extent from the BVH root, including refitted bounds.
    pub fn bounds(&self) -> Option<WorldBounds> {
        self.index.bounds()
    }

    /// Stable back-to-front order; positions in this slice identify bounds updates.
    pub fn items(&self) -> &[SceneItem<T>] {
        &self.items
    }

    /// Update one item's bounds and refit its leaf and ancestors without rebuilding.
    /// Returns false for a missing index or invalid bounds, without changing the scene.
    /// Large repeated moves can degrade spatial partition quality; rebuild if needed.
    pub fn set_bounds(&mut self, index: usize, bounds: WorldBounds) -> bool {
        if index >= self.items.len() || !bounds.is_valid() {
            return false;
        }
        self.items[index].bounds = bounds;
        self.revision = self.revision.wrapping_add(1);
        self.index.refit(&self.items, index);
        true
    }

    pub fn visible(
        &self,
        camera: Camera2D,
        viewport: Point,
    ) -> impl Iterator<Item = VisibleItem<'_, T>> {
        self.query(camera.visible_world_bounds(viewport))
            .map(move |item| {
                let min = camera.world_to_screen(item.bounds.min, viewport);
                let max = camera.world_to_screen(item.bounds.max, viewport);
                VisibleItem {
                    item,
                    screen_bounds: WorldBounds { min, max },
                    detail: LevelOfDetail::for_size(max.x - min.x, max.y - min.y),
                }
            })
    }

    /// All AABBs intersecting a world-space rectangle, in back-to-front order.
    /// Boundary contact counts as intersection. Suitable for rectangular selection.
    pub fn query(&self, bounds: WorldBounds) -> impl Iterator<Item = &SceneItem<T>> {
        self.index
            .query(&self.items, bounds)
            .into_iter()
            .map(|index| &self.items[index])
    }

    /// Topmost AABB hit, in the same order as painting. Points are world-space.
    pub fn hit_test(&self, world: Point) -> Option<&SceneItem<T>> {
        self.index
            .hit_test(&self.items, world)
            .map(|index| &self.items[index])
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

    #[test]
    fn indexed_queries_match_linear_reference_across_overlapping_rectangles() {
        let mut seed = 43_u64;
        let mut next = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            (seed >> 32) as u32 as f64 / u32::MAX as f64
        };
        let scene = Scene::new((0..4000).map(|data| {
            let min = Point::new(next() * 10000. - 5000., next() * 10000. - 5000.);
            SceneItem {
                bounds: WorldBounds {
                    min,
                    max: Point::new(min.x + next() * 800., min.y + next() * 800.),
                },
                data,
            }
        }));
        for _ in 0..200 {
            let min = Point::new(next() * 12000. - 6000., next() * 12000. - 6000.);
            let bounds = WorldBounds {
                min,
                max: Point::new(min.x + next() * 2000., min.y + next() * 2000.),
            };
            let expected: Vec<_> = scene
                .items
                .iter()
                .filter(|item| item.bounds.intersects(bounds))
                .map(|item| item.data)
                .collect();
            assert_eq!(
                scene
                    .query(bounds)
                    .map(|item| item.data)
                    .collect::<Vec<_>>(),
                expected
            );
            let hit = scene
                .items
                .iter()
                .rev()
                .find(|item| item.bounds.contains(min))
                .map(|item| item.data);
            assert_eq!(scene.hit_test(min).map(|item| item.data), hit);
        }
    }

    #[test]
    fn index_handles_empty_coincident_boundary_and_extreme_bounds() {
        let scene = Scene::new((0..300).map(|index| item(-10., 10., index)));
        let edge = WorldBounds {
            min: Point::new(10., 10.),
            max: Point::new(10., 10.),
        };
        assert_eq!(
            scene.query(edge).map(|item| item.data).collect::<Vec<_>>(),
            (0..300).collect::<Vec<_>>()
        );
        assert_eq!(scene.hit_test(edge.min).unwrap().data, 299);
        let extreme = Scene::new((0..100).map(|index| item(-f64::MAX, f64::MAX, index)));
        assert_eq!(extreme.hit_test(Point::default()).unwrap().data, 99);
        assert_eq!(extreme.query(edge).count(), 100);
        let empty = Scene::<()>::new([]);
        assert_eq!(empty.query(edge).count(), 0);
        assert!(empty.hit_test(Point::default()).is_none());
    }

    #[test]
    fn bounds_updates_refit_all_ancestors_and_reject_invalid_input() {
        let mut scene = Scene::new(
            (0..100).map(|index| item(index as f64 * 100., index as f64 * 100. + 10., index)),
        );
        assert_eq!(scene.revision(), 0);
        for step in 0..300 {
            let index = step % 100;
            let value = step as f64 * -500.;
            let bounds = WorldBounds {
                min: Point::new(value, value),
                max: Point::new(value + 10., value + 10.),
            };
            assert!(scene.set_bounds(index, bounds));
            let expected: Vec<_> = scene
                .items
                .iter()
                .filter(|item| item.bounds.intersects(bounds))
                .map(|item| item.data)
                .collect();
            assert_eq!(
                scene
                    .query(bounds)
                    .map(|item| item.data)
                    .collect::<Vec<_>>(),
                expected
            );
            let hit = scene
                .items
                .iter()
                .rev()
                .find(|item| item.bounds.contains(bounds.min))
                .map(|item| item.data);
            assert_eq!(scene.hit_test(bounds.min).map(|item| item.data), hit);
        }
        let invalid = WorldBounds {
            min: Point::new(f64::NAN, 0.),
            max: Point::default(),
        };
        assert!(!scene.set_bounds(0, invalid));
        assert!(!scene.set_bounds(100, scene.items[0].bounds));
        assert_eq!(scene.revision(), 300);
        assert!(scene.items[0].bounds.is_valid());
        assert!(scene.hit_test(Point::new(1., 1.)).is_none());
    }
}
