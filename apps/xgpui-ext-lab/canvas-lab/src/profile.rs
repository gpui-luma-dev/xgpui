use std::{path::PathBuf, time::Instant};

/// Windowed camera sweep: 30 warmup + 120 measured redraws per scene size.
pub struct Profile {
    pub output: PathBuf,
    pub frame: usize,
    last: Option<Instant>,
    samples: Vec<(usize, f64, f64, usize, usize)>,
}

impl Profile {
    pub fn new(output: PathBuf) -> Self {
        Self {
            output,
            frame: 0,
            last: None,
            samples: Vec::new(),
        }
    }
    pub fn begin(&mut self, now: Instant) -> usize {
        self.frame += 1;
        let interval = self
            .last
            .replace(now)
            .map(|last| now.duration_since(last).as_secs_f64() * 1000.)
            .unwrap_or(0.);
        if (self.frame - 1) % 150 >= 30 {
            self.samples
                .push(((self.frame - 1) / 150, interval, 0., 0, 0));
        }
        self.count()
    }
    fn count(&self) -> usize {
        if self.frame <= 150 { 10_000 } else { 100_000 }
    }
    pub fn record_build(&mut self, milliseconds: f64, visible: usize, elements: usize) {
        if (self.frame - 1) % 150 >= 30 {
            let sample = self.samples.last_mut().unwrap();
            sample.2 = milliseconds;
            sample.3 = visible;
            sample.4 = elements;
        }
    }
    pub fn finish(&self, viewport: [f32; 2], scale_factor: f32) {
        let scenes: Vec<_> = [(10_000, 1.), (100_000, 1.), (100_000, 0.01)]
            .into_iter()
            .enumerate()
            .map(|(stage, (count, zoom))| {
                let samples: Vec<_> = self
                    .samples
                    .iter()
                    .filter(|sample| sample.0 == stage)
                    .collect();
                serde_json::json!({
                    "objects": count, "zoom": zoom, "measured_frames": samples.len(), "warmup_frames": 30,
                    "frame_interval_ms": distribution(samples.iter().map(|s| s.1).collect()),
                    "cpu_cull_and_build_ms": distribution(samples.iter().map(|s| s.2).collect()),
                    "visible_min": samples.iter().map(|s| s.3).min(),
                    "visible_max": samples.iter().map(|s| s.3).max(),
                    "element_min": samples.iter().map(|s| s.4).min(),
                    "element_max": samples.iter().map(|s| s.4).max(),
                })
            })
            .collect();
        let report = serde_json::json!({
            "app": "canvas-lab", "optimized_build": !cfg!(debug_assertions),
            "os": std::env::consts::OS, "arch": std::env::consts::ARCH,
            "viewport_logical_pixels": viewport, "scale_factor": scale_factor,
            "scope": "Frame intervals include scheduling, layout and rendering; CPU samples include only culling and child construction. Neither is a GPU timing.",
            "workload": "10k and 100k at 1x; 100k at 0.01x; pan by (1, 0.5) logical pixels per redraw",
            "scenes": scenes,
        });
        std::fs::write(&self.output, serde_json::to_vec_pretty(&report).unwrap())
            .expect("write canvas profile");
        println!("Canvas profile: {}", self.output.display());
    }
}

fn distribution(mut values: Vec<f64>) -> serde_json::Value {
    values.sort_by(f64::total_cmp);
    serde_json::json!({ "mean": values.iter().sum::<f64>() / values.len() as f64,
        "p95": values[(values.len() as f64 * 0.95).ceil() as usize - 1], "max": values.last() })
}
