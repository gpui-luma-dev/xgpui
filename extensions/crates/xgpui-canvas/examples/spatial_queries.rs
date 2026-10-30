//! Dependency-free CPU query comparison; no window, layout, or rendering.
use std::{hint::black_box, time::Instant};
use xgpui_canvas::{Point, Scene, SceneItem, WorldBounds};

fn main() {
    println!("objects,build_ms,indexed_query_mean_us,linear_query_mean_us,queries,mean_matches");
    for count in [10_000, 100_000, 1_000_000] {
        let bounds: Vec<_> = (0..count)
            .map(|id| {
                let min = Point::new((id % 1000) as f64 * 200., (id / 1000) as f64 * 120.);
                WorldBounds {
                    min,
                    max: Point::new(min.x + 160., min.y + 80.),
                }
            })
            .collect();
        let start = Instant::now();
        let scene = Scene::new(
            bounds
                .iter()
                .enumerate()
                .map(|(data, &bounds)| SceneItem { bounds, data }),
        );
        let build = start.elapsed().as_secs_f64() * 1000.;
        let mut indexed_time = 0.;
        let mut linear_time = 0.;
        let mut matches = 0;
        let queries = 200;
        for iteration in 0..queries {
            // Fixed-size sparse viewport, at positions inside each scene's extent.
            let min = Point::new(
                ((iteration * 43) % 990) as f64 * 200.,
                ((iteration * 7) % (count / 1000).max(1)) as f64 * 120.,
            );
            let query = WorldBounds {
                min,
                max: Point::new(min.x + 1000., min.y + 700.),
            };
            let start = Instant::now();
            let indexed: Vec<_> = black_box(&scene)
                .query(black_box(query))
                .map(|item| item.data)
                .collect();
            indexed_time += start.elapsed().as_secs_f64() * 1e6;
            let start = Instant::now();
            let linear: Vec<_> = black_box(&bounds)
                .iter()
                .enumerate()
                .filter(|(_, bounds)| bounds.intersects(black_box(query)))
                .map(|(index, _)| index)
                .collect();
            linear_time += start.elapsed().as_secs_f64() * 1e6;
            assert_eq!(indexed, linear);
            matches += indexed.len();
        }
        println!(
            "{count},{build:.3},{:.3},{:.3},{queries},{:.1}",
            indexed_time / queries as f64,
            linear_time / queries as f64,
            matches as f64 / queries as f64
        );
    }
}
