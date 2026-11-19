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
    content_revision: u64,
    identity: usize,
}

impl<T> Scene<T> {
    /// Build a scene, preserving caller paint indices.
    ///
    /// # Panics
    /// Panics if any item has non-finite or inverted bounds.
    pub fn new(items: impl IntoIterator<Item = SceneItem<T>>) -> Self {
        let items: Vec<_> = items.into_iter().collect();
        assert!(
            items.iter().all(|item| item.bounds.is_valid()),
            "scene item bounds must be finite and ordered"
        );
        let index = SpatialIndex::new(&items);
        Self {
            items,
            index,
            revision: 0,
            content_revision: 0,
            identity: {
                static NEXT: std::sync::atomic::AtomicUsize =
                    std::sync::atomic::AtomicUsize::new(1);
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            },
        }
    }
    /// Changes whenever bounds are updated; useful for invalidating retained captures.
    /// Process-local geometry cache key, distinct even for replacement scenes at revision zero.
    pub fn cache_key(&self) -> (usize, u64) {
        (self.identity, self.revision)
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Appearance-aware cache key. Geometry and mutable data access both invalidate it.
    /// Like cache_key(), this is process-local and must not be serialized as object identity.
    pub fn content_cache_key(&self) -> (usize, u64, u64) {
        (self.identity, self.revision, self.content_revision)
    }

    /// Advances on successful mutable data access, even when no value is changed.
    pub fn content_revision(&self) -> u64 {
        self.content_revision
    }

    /// Mutate caller data without exposing bounds or rebuilding the index.
    /// Conservatively invalidates content captures on access; geometry keys stay unchanged.
    /// Callers changing geometric extent must also update the item's bounds.
    pub fn data_mut(&mut self, index: usize) -> Option<&mut T> {
        let item = self.items.get_mut(index)?;
        self.content_revision = self.content_revision.wrapping_add(1);
        Some(&mut item.data)
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

    /// Atomically apply distinct bounds updates and refit shared ancestors once.
    /// Rejects the whole batch for any missing/duplicate index or invalid bounds.
    /// An empty batch succeeds without invalidation; a nonempty batch advances
    /// geometry revision once. Indices remain stable paint-order positions.
    pub fn set_bounds_many(
        &mut self,
        updates: impl IntoIterator<Item = (usize, WorldBounds)>,
    ) -> bool {
        let updates: Vec<_> = updates.into_iter().collect();
        if updates.is_empty() {
            return true;
        }
        if let [(index, bounds)] = updates.as_slice() {
            return self.set_bounds(*index, *bounds);
        }
        let mut seen = std::collections::HashSet::with_capacity(updates.len());
        if updates.iter().any(|(index, bounds)| {
            *index >= self.items.len() || !bounds.is_valid() || !seen.insert(*index)
        }) {
            return false;
        }
        for &(index, bounds) in &updates {
            self.items[index].bounds = bounds;
        }
        self.index
            .refit_many(&self.items, updates.iter().map(|(index, _)| *index));
        self.revision = self.revision.wrapping_add(1);
        true
    }

    /// Restore spatial partition quality without changing geometry, data or paint indices.
    /// Geometry/content cache keys remain unchanged because rendered content is unchanged.
    pub fn rebuild_index(&mut self) {
        self.index = SpatialIndex::new(&self.items);
    }

    /// Visible objects paired with their stable-within-this-scene paint indices.
    pub fn visible_indexed(
        &self,
        camera: Camera2D,
        viewport: Point,
    ) -> impl Iterator<Item = (usize, VisibleItem<'_, T>)> {
        self.query_indexed(camera.visible_world_bounds(viewport))
            .map(move |(index, item)| {
                let min = camera.world_to_screen(item.bounds.min, viewport);
                let max = camera.world_to_screen(item.bounds.max, viewport);
                (
                    index,
                    VisibleItem {
                        item,
                        screen_bounds: WorldBounds { min, max },
                        detail: LevelOfDetail::for_size(max.x - min.x, max.y - min.y),
                    },
                )
            })
    }

    /// Rectangle candidates in back-to-front paint order, paired with paint indices.
    pub fn query_indexed(
        &self,
        bounds: WorldBounds,
    ) -> impl DoubleEndedIterator<Item = (usize, &SceneItem<T>)> {
        self.index
            .query(&self.items, bounds)
            .into_iter()
            .map(|index| (index, &self.items[index]))
    }

    /// Topmost AABB hit with its paint index. Exact shape picking remains caller-owned.
    pub fn hit_test_indexed(&self, world: Point) -> Option<(usize, &SceneItem<T>)> {
        self.index
            .hit_test(&self.items, world)
            .map(|index| (index, &self.items[index]))
    }

    /// All point/AABB candidates, front-to-back. Continue after rejecting transparent,
    /// locked or geometrically empty candidates in the caller's precise hit test.
    pub fn hits_at(&self, world: Point) -> impl Iterator<Item = (usize, &SceneItem<T>)> {
        self.query_indexed(WorldBounds {
            min: world,
            max: world,
        })
        .rev()
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
    fn indexed_queries_preserve_paint_order_and_allow_precise_hit_rejection() {
        let scene = Scene::new([false, true, false].map(|opaque| SceneItem {
            bounds: item(0., 10., 0).bounds,
            data: opaque,
        }));
        let point = Point::new(5., 5.);
        assert_eq!(
            scene
                .query_indexed(item(0., 10., 0).bounds)
                .map(|(i, _)| i)
                .collect::<Vec<_>>(),
            [0, 1, 2]
        );
        assert_eq!(
            scene.hits_at(point).map(|(i, _)| i).collect::<Vec<_>>(),
            [2, 1, 0]
        );
        assert_eq!(scene.hit_test_indexed(point).unwrap().0, 2);
        assert_eq!(
            scene.hits_at(point).find(|(_, item)| item.data).unwrap().0,
            1
        );
        assert_eq!(
            scene
                .visible_indexed(Camera2D::default(), Point::new(100., 100.))
                .map(|(i, _)| i)
                .collect::<Vec<_>>(),
            [0, 1, 2]
        );
        assert_eq!(scene.hits_at(Point::new(10., 10.)).count(), 3);
        assert_eq!(scene.hits_at(Point::new(f64::NAN, 0.)).count(), 0);
        assert_eq!(Scene::<()>::new([]).hits_at(point).count(), 0);
    }

    #[test]
    fn data_mutation_invalidates_content_without_touching_geometry() {
        let mut scene = Scene::new([item(0., 10., 7)]);
        let geometry = scene.cache_key();
        let content = scene.content_cache_key();
        assert!(scene.data_mut(1).is_none());
        assert_eq!(scene.content_cache_key(), content);
        *scene.data_mut(0).unwrap() = 42;
        assert_eq!(scene.cache_key(), geometry);
        assert_ne!(scene.content_cache_key(), content);
        assert_eq!(scene.content_revision(), 1);
        assert_eq!(scene.hit_test(Point::default()).unwrap().data, 42);
        let content = scene.content_cache_key();
        let _ = scene.data_mut(0);
        assert_ne!(scene.content_cache_key(), content);
        assert!(scene.set_bounds(0, item(20., 30., 0).bounds));
        assert_eq!(scene.content_revision(), 2);
        assert_ne!(scene.cache_key(), geometry);
        assert_ne!(scene.content_cache_key().1, content.1);
    }

    #[test]
    fn batch_updates_are_atomic_and_invalidate_geometry_once() {
        let mut scene = Scene::new([item(0., 10., 0), item(20., 30., 1)]);
        let before = scene.cache_key();
        for updates in [
            vec![
                (0, item(100., 110., 0).bounds),
                (99, item(0., 1., 0).bounds),
            ],
            vec![
                (0, item(100., 110., 0).bounds),
                (0, item(200., 210., 0).bounds),
            ],
            vec![
                (0, item(100., 110., 0).bounds),
                (1, item(f64::NAN, 1., 0).bounds),
            ],
            vec![(0, item(100., 110., 0).bounds), (1, item(2., 1., 0).bounds)],
        ] {
            assert!(!scene.set_bounds_many(updates));
            assert_eq!(scene.cache_key(), before);
            assert_eq!(scene.items()[0].bounds.min, Point::default());
            assert_eq!(scene.hit_test(Point::default()).unwrap().data, 0);
        }
        assert!(scene.set_bounds_many([]));
        assert_eq!(scene.cache_key(), before);
        assert!(scene.set_bounds_many([
            (0, item(-20., -10., 0).bounds),
            (1, item(40., 50., 0).bounds)
        ]));
        assert_eq!(scene.revision(), 1);
        assert_eq!(scene.content_revision(), 0);
        assert!(scene.hit_test(Point::default()).is_none());
        assert_eq!(scene.hit_test(Point::new(45., 45.)).unwrap().data, 1);
    }

    #[test]
    fn batches_and_rebuilds_match_linear_queries_after_large_moves() {
        let mut scene = Scene::new((0..513).map(|i| item(i as f64 * 5., i as f64 * 5. + 10., i)));
        for round in 0..20 {
            let updates: Vec<_> = (0..513)
                .filter(|i| (i + round) % 7 == 0)
                .map(|i| {
                    (
                        i,
                        item(
                            i as f64 * -11. + round as f64,
                            i as f64 * -11. + round as f64 + 20.,
                            i,
                        )
                        .bounds,
                    )
                })
                .collect();
            assert!(scene.set_bounds_many(updates));
            for rebuild in [false, true] {
                let key = scene.cache_key();
                let content = scene.content_cache_key();
                if rebuild {
                    scene.rebuild_index();
                }
                assert_eq!(scene.cache_key(), key);
                assert_eq!(scene.content_cache_key(), content);
                for n in 0..50 {
                    let q = item(n as f64 * -100., n as f64 * -100. + 300., 0).bounds;
                    let expected: Vec<_> = scene
                        .items()
                        .iter()
                        .enumerate()
                        .filter(|(_, item)| item.bounds.intersects(q))
                        .map(|(i, _)| i)
                        .collect();
                    assert_eq!(
                        scene.query_indexed(q).map(|(i, _)| i).collect::<Vec<_>>(),
                        expected
                    );
                    let expected = scene
                        .items()
                        .iter()
                        .enumerate()
                        .rev()
                        .find(|(_, item)| item.bounds.contains(q.min))
                        .map(|(i, _)| i);
                    assert_eq!(scene.hit_test_indexed(q.min).map(|(i, _)| i), expected);
                }
            }
        }
        let mut empty = Scene::<()>::new([]);
        let key = empty.content_cache_key();
        empty.rebuild_index();
        assert_eq!(empty.content_cache_key(), key);
    }

    #[test]
    fn constructor_rejects_nonfinite_and_inverted_bounds() {
        for bounds in [
            item(f64::NAN, 1., 0).bounds,
            item(0., f64::INFINITY, 0).bounds,
            item(2., 1., 0).bounds,
        ] {
            assert!(
                std::panic::catch_unwind(|| Scene::new([SceneItem { bounds, data: () }])).is_err()
            );
        }
    }

    #[test]
    fn cache_key_tracks_geometry_and_distinguishes_replacement_scenes() {
        let mut scene = Scene::new([item(0., 10., 0)]);
        let original = scene.cache_key();
        let replacement = Scene::new([item(0., 10., 0)]);
        assert_ne!(original, replacement.cache_key());
        assert!(!scene.set_bounds(99, scene.items()[0].bounds));
        assert_eq!(original, scene.cache_key());
        assert!(scene.set_bounds(0, item(20., 30., 0).bounds));
        assert_ne!(original, scene.cache_key());
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
