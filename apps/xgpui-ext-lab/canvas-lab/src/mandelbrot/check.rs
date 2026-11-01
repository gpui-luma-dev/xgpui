//! Bounded native camera sweep. Measures render-callback intervals, not GPU time
//! or confirmed displayed FPS. No capture/readback occurs during moving phases.
use super::*;

pub(super) struct Check {
    directory: PathBuf,
    stage: usize,
    frame: usize,
    stage_started: Instant,
    previous: Option<Instant>,
    intervals: Vec<f64>,
    completed: Vec<serde_json::Value>,
    missing_front: usize,
    uncovered: usize,
    stage_coverage: [[usize; 2]; 9],
}
impl Check {
    pub fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            stage: 0,
            frame: 0,
            stage_started: Instant::now(),
            previous: None,
            intervals: Vec::new(),
            completed: Vec::new(),
            missing_front: 0,
            uncovered: 0,
            stage_coverage: [[0; 2]; 9],
        }
    }
    fn advance(&mut self) {
        self.stage += 1;
        self.frame = 0;
        self.previous = None;
        self.stage_started = Instant::now();
    }
    pub fn tick(&mut self, lab: &mut Lab, window: &mut Window, _cx: &mut Context<Lab>) -> bool {
        let viewport = lab.viewport();
        if viewport.x <= 0. || viewport.y <= 0. {
            window.request_animation_frame();
            return true;
        }
        let moving = matches!(self.stage, 1 | 3 | 5 | 7);
        if moving {
            let now = Instant::now();
            if let Some(previous) = self.previous {
                self.intervals
                    .push(now.duration_since(previous).as_secs_f64() * 1000.);
            }
            self.previous = Some(now);
        }
        let (zoom, center, name) = match self.stage {
            0 => (1., WorldPoint::default(), "startup"),
            1 => (
                32_f64.powf(self.frame as f64 / 90.),
                WorldPoint::new(-75. + self.frame as f64 * 0.1, 30.),
                "zoom-in",
            ),
            2 => (32., WorldPoint::new(-66., 30.), "gpu-detail"),
            3 => (
                32. * (0.01_f64 / 32.).powf(self.frame as f64 / 120.),
                WorldPoint::default(),
                "zoom-out",
            ),
            4 => (0.01, WorldPoint::default(), "minimum-zoom"),
            5 => (
                32. * (1e10_f64 / 32.).powf(self.frame as f64 / 150.),
                WorldPoint::new(-73.0931661111453, 39.547771261599),
                "deep-zoom-in",
            ),
            6 => (
                1e10,
                WorldPoint::new(-73.0931661111453, 39.547771261599),
                "maximum-zoom",
            ),
            7 => (
                1e10_f64.powf(1. - self.frame as f64 / 150.),
                WorldPoint::new(-73.0931661111453, 39.547771261599),
                "deep-zoom-out",
            ),
            _ => (1., WorldPoint::default(), "reset"),
        };
        lab.camera = Camera2D::default();
        lab.camera.center_on(center);
        lab.camera.zoom_at_with_limits(
            WorldPoint::new(viewport.x / 2., viewport.y / 2.),
            viewport,
            zoom,
            0.01,
            1e10,
        );
        if moving {
            let current = lab.camera.visible_world_bounds(viewport);
            match &lab.fractal.front {
                None => {
                    self.missing_front += 1;
                    self.stage_coverage[self.stage][0] += 1;
                }
                Some(front) => {
                    let ratio = zoom / front.request.view.zoom;
                    if !(1. / MAX_REPROJECTION..=MAX_REPROJECTION).contains(&ratio) {
                        self.missing_front += 1;
                        self.stage_coverage[self.stage][0] += 1;
                    }
                    let coverage = front.request.bounds();
                    if !(coverage.contains(current.min) && coverage.contains(current.max)) {
                        self.uncovered += 1;
                        self.stage_coverage[self.stage][1] += 1;
                    }
                }
            }
            self.frame += 1;
            let count = match self.stage {
                1 => 90,
                3 => 120,
                _ => 150,
            };
            if self.frame > count {
                self.advance();
            }
        } else {
            let expected = View::new(lab.camera, viewport, window.scale_factor());
            if let Some(front) = &lab.fractal.front
                && lab.fractal.refined
                && lab.fractal.sharpening.is_none()
                && front.request.view == expected
            {
                std::fs::create_dir_all(&self.directory).expect("create frame-check directory");
                self.completed.push(serde_json::json!({ "stage": name, "elapsed_ms": self.stage_started.elapsed().as_secs_f64() * 1000., "zoom": zoom, "gpu": front.request.gpu, "dimensions": front.request.dimensions }));
                let path = self.directory.join(format!("{name}.png"));
                window.on_next_frame(move |window, _| {
                    window
                        .render_to_image()
                        .expect("capture frame-check view")
                        .save(path)
                        .expect("save frame-check view");
                });
                if self.stage >= 8 {
                    self.intervals.sort_by(f64::total_cmp);
                    let count = self.intervals.len();
                    let mean = self.intervals.iter().sum::<f64>() / count.max(1) as f64;
                    let p95 = self
                        .intervals
                        .get((count as f64 * 0.95) as usize)
                        .copied()
                        .unwrap_or(0.);
                    let report = serde_json::json!({ "scope": "Local render-callback intervals during bounded camera sweeps; not isolated GPU time or confirmed displayed FPS", "inactive_window_throttle_disabled": true, "platform": format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH), "viewport_logical": [viewport.x, viewport.y], "scale_factor": window.scale_factor(), "moving_samples": count, "moving_frames_without_usable_front": self.missing_front, "moving_frames_with_exposed_guard_band": self.uncovered, "coverage_by_stage_missing_exposed": self.stage_coverage, "moving_mean_ms": mean, "moving_p95_ms": p95, "moving_max_ms": self.intervals.last(), "completed_views": self.completed });
                    std::fs::write(
                        self.directory.join("report.json"),
                        serde_json::to_vec_pretty(&report).unwrap(),
                    )
                    .expect("save frame-check report");
                    window.on_next_frame(|_, cx| cx.quit());
                    window.request_animation_frame();
                    return false;
                }
                self.advance();
            } else if self.stage_started.elapsed() > Duration::from_secs(20) {
                panic!("Mandelbrot frame-check timed out at {name}");
            }
        }
        window.request_animation_frame();
        true
    }
}
