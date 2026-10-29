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
            let x = (((center.x - domain.min.x) / (domain.max.x - domain.min.x) * SIDE as f64)
                as usize)
                .min(SIDE - 1);
            let y = (((center.y - domain.min.y) / (domain.max.y - domain.min.y) * SIDE as f64)
                as usize)
                .min(SIDE - 1);
            overview.cells[y * SIDE + x] += 1;
        }
        overview
    }

    pub fn domain(&self) -> WorldBounds {
        self.domain
    }

    pub fn cells(&self) -> impl Iterator<Item = OverviewCell> + '_ {
        let step = Point::new(
            (self.domain.max.x - self.domain.min.x) / SIDE as f64,
            (self.domain.max.y - self.domain.min.y) / SIDE as f64,
        );
        self.cells
            .iter()
            .enumerate()
            .filter(|(_, count)| **count > 0)
            .map(move |(index, &count)| {
                let min = Point::new(
                    self.domain.min.x + (index % SIDE) as f64 * step.x,
                    self.domain.min.y + (index / SIDE) as f64 * step.y,
                );
                OverviewCell {
                    bounds: WorldBounds {
                        min,
                        max: Point::new(min.x + step.x, min.y + step.y),
                    },
                    count,
                }
            })
    }

    fn projection(&self, size: Point) -> (f64, Point) {
        let width = self.domain.max.x - self.domain.min.x;
        let height = self.domain.max.y - self.domain.min.y;
        let scale = (size.x / width).min(size.y / height);
        (
            scale,
            Point::new(
                (size.x - width * scale) / 2.,
                (size.y - height * scale) / 2.,
            ),
        )
    }

    /// Aspect-preserving projection into a positive, finite logical-pixel size.
    pub fn world_to_map(&self, world: Point, size: Point) -> Point {
        let (scale, offset) = self.projection(size);
        Point::new(
            (world.x - self.domain.min.x) * scale + offset.x,
            (world.y - self.domain.min.y) * scale + offset.y,
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
        let (scale, offset) = self.projection(size);
        Some(Point::new(
            (self.domain.min.x + (local.x - offset.x) / scale)
                .clamp(self.domain.min.x, self.domain.max.x),
            (self.domain.min.y + (local.y - offset.y) / scale)
                .clamp(self.domain.min.y, self.domain.max.y),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
