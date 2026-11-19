use crate::{Point, Scene, WorldBounds};

const SIDE: usize = 32;

/// Cached center-density overview, bounded to 32×32 cells, independent of GPUI.
/// Build once and refresh after edits; projection and painting need no scene scan.
pub struct Overview {
    domain: WorldBounds,
    cells: [usize; SIDE * SIDE],
}

pub struct OverviewCell {
    pub bounds: WorldBounds,
    pub count: usize,
}

impl Overview {
    pub fn new<T>(scene: &Scene<T>) -> Self {
        let mut domain = scene.bounds().unwrap_or(WorldBounds {
            min: Point::new(-1., -1.),
            max: Point::new(1., 1.),
        });
        // Give zero-width/height scenes a usable navigation domain.
        if domain.min.x == domain.max.x {
            let padding = (domain.min.x.abs() * 1e-6).max(1.);
            domain.min.x = (domain.min.x - padding).max(-f64::MAX);
            domain.max.x = (domain.max.x + padding).min(f64::MAX);
        }
        if domain.min.y == domain.max.y {
            let padding = (domain.min.y.abs() * 1e-6).max(1.);
            domain.min.y = (domain.min.y - padding).max(-f64::MAX);
            domain.max.y = (domain.max.y + padding).min(f64::MAX);
        }
        let mut overview = Self {
            domain,
            cells: [0; SIDE * SIDE],
        };
        for item in scene.items() {
            let center = Point::new(
                item.bounds.min.x * 0.5 + item.bounds.max.x * 0.5,
                item.bounds.min.y * 0.5 + item.bounds.max.y * 0.5,
            );
            let x = ((fraction(center.x, domain.min.x, domain.max.x) * SIDE as f64) as usize)
                .min(SIDE - 1);
            let y = ((fraction(center.y, domain.min.y, domain.max.y) * SIDE as f64) as usize)
                .min(SIDE - 1);
            overview.cells[y * SIDE + x] += 1;
        }
        overview
    }

    pub fn domain(&self) -> WorldBounds {
        self.domain
    }

    pub fn cells(&self) -> impl Iterator<Item = OverviewCell> + '_ {
        self.cells
            .iter()
            .enumerate()
            .filter(|(_, count)| **count > 0)
            .map(move |(index, &count)| {
                let x = (index % SIDE) as f64 / SIDE as f64;
                let y = (index / SIDE) as f64 / SIDE as f64;
                let next = 1. / SIDE as f64;
                OverviewCell {
                    bounds: WorldBounds {
                        min: Point::new(
                            interpolate(self.domain.min.x, self.domain.max.x, x),
                            interpolate(self.domain.min.y, self.domain.max.y, y),
                        ),
                        max: Point::new(
                            interpolate(self.domain.min.x, self.domain.max.x, x + next),
                            interpolate(self.domain.min.y, self.domain.max.y, y + next),
                        ),
                    },
                    count,
                }
            })
    }

    // Return occupied map size and letterbox offset, without materializing a world extent.
    fn projection(&self, size: Point) -> (Point, Point) {
        let magnitude = self
            .domain
            .min
            .x
            .abs()
            .max(self.domain.max.x.abs())
            .max(self.domain.min.y.abs())
            .max(self.domain.max.y.abs());
        let extent = |min: f64, max: f64| {
            let span = max - min;
            if span.is_finite() {
                span / magnitude
            } else {
                max / magnitude - min / magnitude
            }
        };
        let width = extent(self.domain.min.x, self.domain.max.x);
        let height = extent(self.domain.min.y, self.domain.max.y);
        // Normalize both extents and viewport before comparing aspect ratios.
        // One extent is exactly 1, avoiding overflowing a fit scale for huge map sizes.
        let extent = width.max(height);
        let width = width / extent;
        let height = height / extent;
        let map_extent = size.x.max(size.y);
        let occupied = if width * (size.y / map_extent) >= height * (size.x / map_extent) {
            Point::new(
                size.x,
                if width == 1. {
                    size.x * height
                } else {
                    size.x / width
                },
            )
        } else {
            Point::new(
                if height == 1. {
                    size.y * width
                } else {
                    size.y / height
                },
                size.y,
            )
        };
        let occupied = Point::new(occupied.x.min(size.x), occupied.y.min(size.y));
        (
            occupied,
            Point::new((size.x - occupied.x) / 2., (size.y - occupied.y) / 2.),
        )
    }

    /// Aspect-preserving projection into a positive, finite logical-pixel size.
    pub fn world_to_map(&self, world: Point, size: Point) -> Point {
        let (occupied, offset) = self.projection(size);
        Point::new(
            fraction(world.x, self.domain.min.x, self.domain.max.x) * occupied.x + offset.x,
            fraction(world.y, self.domain.min.y, self.domain.max.y) * occupied.y + offset.y,
        )
    }

    /// Inverse projection, clamped to scene extent (including letterboxed regions).
    pub fn map_to_world(&self, local: Point, size: Point) -> Option<Point> {
        if ![local.x, local.y, size.x, size.y]
            .iter()
            .all(|v| v.is_finite())
            || size.x <= 0.
            || size.y <= 0.
        {
            return None;
        }
        let (occupied, offset) = self.projection(size);
        let position = |value: f64, offset: f64, extent: f64| {
            if extent == 0. {
                0.5
            } else {
                ((value - offset) / extent).clamp(0., 1.)
            }
        };
        Some(Point::new(
            interpolate(
                self.domain.min.x,
                self.domain.max.x,
                position(local.x, offset.x, occupied.x),
            ),
            interpolate(
                self.domain.min.y,
                self.domain.max.y,
                position(local.y, offset.y, occupied.y),
            ),
        ))
    }
}

// Halve before subtracting only when the direct finite span would overflow.
fn fraction(value: f64, min: f64, max: f64) -> f64 {
    let span = max - min;
    if span.is_finite() {
        (value - min) / span
    } else {
        (value * 0.5 - min * 0.5) / (max * 0.5 - min * 0.5)
    }
}

fn interpolate(min: f64, max: f64, t: f64) -> f64 {
    if t == 0. {
        return min;
    }
    if t == 1. {
        return max;
    }
    let span = max - min;
    if span.is_finite() {
        min + span * t
    } else {
        min * (1. - t) + max * t
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extreme_finite_domains_have_finite_cells_and_roundtrip() {
        for (min, max) in [
            (-f64::MAX, f64::MAX),
            (f64::MAX, f64::MAX),
            (-f64::MAX, -f64::MAX),
            (1e-300, 2e-300),
        ] {
            let scene = Scene::new([crate::SceneItem {
                bounds: WorldBounds {
                    min: Point::new(min, min),
                    max: Point::new(max, max),
                },
                data: (),
            }]);
            let overview = Overview::new(&scene);
            let size = Point::new(200., 100.);
            let domain = overview.domain();
            for cell in overview.cells() {
                assert!(cell.bounds.is_valid(), "{:?}", cell.bounds);
            }
            for point in [domain.min, domain.max] {
                let mapped = overview.world_to_map(point, size);
                assert!(mapped.x.is_finite() && mapped.y.is_finite(), "{mapped:?}");
                let back = overview.map_to_world(mapped, size).unwrap();
                assert!(back.x.is_finite() && back.y.is_finite(), "{back:?}");
                assert!((back.x / point.x - 1.).abs() < 1e-12);
                assert!((back.y / point.y - 1.).abs() < 1e-12);
            }
        }
    }

    #[test]
    fn narrow_distant_domains_fit_huge_maps_without_infinite_scale() {
        let scene = Scene::new([crate::SceneItem {
            bounds: WorldBounds {
                min: Point::new(1e300, 1e300),
                max: Point::new(1e300 + 1e285, 1e300 + 2e285),
            },
            data: (),
        }]);
        let overview = Overview::new(&scene);
        for size in [Point::new(f64::MAX, f64::MAX), Point::new(200., 100.)] {
            let low = overview.world_to_map(overview.domain().min, size);
            let high = overview.world_to_map(overview.domain().max, size);
            assert!(
                low.x.is_finite() && low.y.is_finite() && high.x.is_finite() && high.y.is_finite()
            );
            assert!(low.x >= 0. && low.y >= 0. && high.x <= size.x && high.y <= size.y);
            assert!(high.x > low.x && high.y > low.y);
        }
    }

    #[test]
    fn extreme_density_bins_do_not_collapse_to_zero() {
        let scene = Scene::new([-f64::MAX, 0., f64::MAX].map(|v| crate::SceneItem {
            bounds: WorldBounds {
                min: Point::new(v, v),
                max: Point::new(v, v),
            },
            data: (),
        }));
        let overview = Overview::new(&scene);
        let cells: Vec<_> = overview.cells().collect();
        assert_eq!(cells.len(), 3);
        assert!(cells.iter().all(|c| c.count == 1 && c.bounds.is_valid()));
        let size = Point::new(f64::MAX, f64::MAX);
        let mapped = overview.world_to_map(Point::default(), size);
        assert!(mapped.x.is_finite() && mapped.y.is_finite());
    }

    #[test]
    fn overview_preserves_counts_and_projection_after_scene_edits() {
        let mut scene = Scene::new((0..10000).map(|data| {
            let min = Point::new((data % 100) as f64 * 20., (data / 100) as f64 * 10.);
            crate::SceneItem {
                bounds: WorldBounds {
                    min,
                    max: Point::new(min.x + 10., min.y + 5.),
                },
                data,
            }
        }));
        assert!(scene.set_bounds(
            0,
            WorldBounds {
                min: Point::new(-1000., -500.),
                max: Point::new(-990., -490.)
            }
        ));
        let overview = Overview::new(&scene);
        assert!(overview.cells().count() <= 1024);
        assert_eq!(
            overview.cells().map(|cell| cell.count).sum::<usize>(),
            10000
        );
        assert_eq!(overview.domain().min, Point::new(-1000., -500.));
        let size = Point::new(220., 140.);
        for point in [
            overview.domain().min,
            overview.domain().max,
            Point::default(),
        ] {
            let roundtrip = overview
                .map_to_world(overview.world_to_map(point, size), size)
                .unwrap();
            assert!((roundtrip.x - point.x).abs() < 1e-8 && (roundtrip.y - point.y).abs() < 1e-8);
        }
        assert_eq!(
            overview.map_to_world(Point::new(-1e9, -1e9), size).unwrap(),
            overview.domain().min
        );
        assert!(
            overview
                .map_to_world(Point::default(), Point::default())
                .is_none()
        );
    }

    #[test]
    fn empty_and_zero_area_scenes_have_usable_domains() {
        let empty = Overview::new(&Scene::<()>::new([]));
        assert_eq!(empty.cells().count(), 0);
        let scene = Scene::new([crate::SceneItem {
            bounds: WorldBounds {
                min: Point::default(),
                max: Point::default(),
            },
            data: (),
        }]);
        let overview = Overview::new(&scene);
        assert_eq!(overview.cells().map(|cell| cell.count).sum::<usize>(), 1);
        let mut camera = crate::Camera2D::default();
        assert!(camera.center_on(Point::new(500., -300.)));
        assert_eq!(
            camera.screen_to_world(Point::new(50., 50.), Point::new(100., 100.)),
            Point::new(500., -300.)
        );
        assert!(!camera.center_on(Point::new(f64::NAN, 0.)));
        assert_eq!(camera.zoom(), 1.);
    }
}
