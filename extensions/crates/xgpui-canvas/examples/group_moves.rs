//! CPU-only comparison of individual/batch bounds updates, including input preparation.
//! Excludes initial index construction, rendering and OS scheduling.
use std::time::Instant;
use xgpui_canvas::{Point, Scene, SceneItem, WorldBounds};

fn scene() -> Scene<usize> {
    Scene::new((0..100_000).map(|i| SceneItem {
        bounds: WorldBounds {
            min: Point::new((i % 400) as f64 * 200., (i / 400) as f64 * 100.),
            max: Point::new(
                (i % 400) as f64 * 200. + 160.,
                (i / 400) as f64 * 100. + 80.,
            ),
        },
        data: i,
    }))
}

fn main() {
    for count in [1, 1_000, 10_000, 100_000] {
        for batch in [false, true] {
            let mut scene = scene();
            let started = Instant::now();
            for _ in 0..20 {
                let updates: Vec<_> = (0..count)
                    .map(|i| (i, scene.items()[i].bounds.translated(Point::new(1., 1.))))
                    .collect();
                if batch {
                    assert!(scene.set_bounds_many(updates));
                } else {
                    for (i, bounds) in updates {
                        assert!(scene.set_bounds(i, bounds));
                    }
                }
            }
            let elapsed = started.elapsed().as_secs_f64() * 1000. / 20.;
            // Check geometry and index, outside the measured update scope.
            assert_eq!(scene.items()[0].bounds.min, Point::new(20., 20.));
            let query = scene.items()[count - 1].bounds;
            let expected: Vec<_> = scene
                .items()
                .iter()
                .enumerate()
                .filter(|(_, item)| item.bounds.intersects(query))
                .map(|(i, _)| i)
                .collect();
            assert_eq!(
                scene
                    .query_indexed(query)
                    .map(|(i, _)| i)
                    .collect::<Vec<_>>(),
                expected
            );
            println!(
                "{count} moved / 100000, {}: {elapsed:.3} ms/update",
                if batch { "batch" } else { "individual" }
            );
        }
    }
}
