//! CPU-only bounds-update benchmark; excludes rendering and OS scheduling.
use std::time::Instant;
use xgpui_canvas::{Point, Scene, SceneItem, WorldBounds};

fn main() {
    for count in [1, 1_000, 10_000, 100_000] {
        let mut scene = Scene::new((0..100_000).map(|i| SceneItem {
            bounds: WorldBounds {
                min: Point::new((i % 400) as f64 * 200., (i / 400) as f64 * 100.),
                max: Point::new(
                    (i % 400) as f64 * 200. + 160.,
                    (i / 400) as f64 * 100. + 80.,
                ),
            },
            data: i,
        }));
        let started = Instant::now();
        for _ in 0..20 {
            for i in 0..count {
                let bounds = scene.items()[i].bounds.translated(Point::new(1., 1.));
                assert!(scene.set_bounds(i, bounds));
            }
        }
        println!(
            "{count} moved / 100000: {:.3} ms/update",
            started.elapsed().as_secs_f64() * 1000. / 20.
        );
    }
}
