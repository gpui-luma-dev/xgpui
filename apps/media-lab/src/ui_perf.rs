//! Render-start cadence in a real window; deliberately not GPU/presentation time.
use crate::model::{Lab, Phase};
use serde::Serialize;
use std::{path::PathBuf, time::Instant};

#[derive(Serialize, Default)]
struct Samples {
    raw_ms: Vec<f64>,
    p50_ms: f64,
    p95_ms: f64,
    max_ms: f64,
    over_25_ms: usize,
    over_50_ms: usize,
}
impl Samples {
    fn finish(&mut self) {
        let mut sorted = self.raw_ms.clone();
        sorted.sort_by(f64::total_cmp);
        if sorted.is_empty() {
            return;
        }
        self.p50_ms = sorted[(sorted.len() as f64 * 0.50).ceil() as usize - 1];
        self.p95_ms = sorted[(sorted.len() as f64 * 0.95).ceil() as usize - 1];
        self.max_ms = *sorted.last().unwrap();
        self.over_25_ms = sorted.iter().filter(|&&v| v > 25.).count();
        self.over_50_ms = sorted.iter().filter(|&&v| v > 50.).count();
    }
}
#[derive(Serialize)]
struct Report {
    schema_version: u32,
    profile: &'static str,
    os: &'static str,
    arch: &'static str,
    description: &'static str,
    source_size: [u32; 2],
    frame_upload_limit: usize,
    frame_byte_limit: usize,
    baseline: Samples,
    burst: Samples,
    cached: Samples,
    completed_sources: usize,
    errors: Vec<String>,
}
pub struct Run {
    path: PathBuf,
    stage: usize,
    previous: Option<Instant>,
    started: Instant,
    report: Report,
}
impl Run {
    pub fn new(path: PathBuf, lab: &Lab) -> anyhow::Result<Self> {
        anyhow::ensure!(
            !cfg!(debug_assertions),
            "visible performance runs require release mode"
        );
        let source = &lab.fixtures.samples[lab.selected];
        Ok(Self {
            path,
            stage: 0,
            previous: None,
            started: Instant::now(),
            report: Report {
                schema_version: 1,
                profile: "release",
                os: std::env::consts::OS,
                arch: std::env::consts::ARCH,
                description: "Visible window with zoom sweep: warm primary, 180 baseline intervals, 12 fresh decode/mip/upload snapshots, 180 cached intervals. Render-start cadence includes UI/driver/scheduling; no GPU timestamps or presented-frame proof. No long intervals discarded. Keep window visible and foreground; background load uncontrolled. Burst duration includes decoding and admission waits. Only warmup-to-baseline boundary excluded; burst-trigger work is included. Baseline has no burst previews; cached phase retains final previews.",
                source_size: [source.width, source.height],
                frame_upload_limit: lab.count_limit,
                frame_byte_limit: lab.byte_limit,
                baseline: Samples::default(),
                burst: Samples::default(),
                cached: Samples::default(),
                completed_sources: 0,
                errors: vec![],
            },
        })
    }
    /// Called at render start, before polling the next source state.
    pub fn tick(&mut self, lab: &mut Lab) -> bool {
        let now = Instant::now();
        if self.started.elapsed().as_secs() > 60 {
            self.report.errors.push("visible run timed out".into());
            self.stage = 4;
        }
        let dt = self
            .previous
            .replace(now)
            .map(|v| now.duration_since(v).as_secs_f64() * 1000.);
        match self.stage {
            0 => {
                if lab.primary.phase == Phase::Failed {
                    self.report
                        .errors
                        .push(lab.primary.error.clone().unwrap_or_default());
                    self.stage = 4;
                } else if lab.primary.phase == Phase::Ready && lab.settle_frames == 0 {
                    self.stage = 1;
                    self.previous = None;
                }
            }
            1 => {
                if let Some(dt) = dt {
                    self.report.baseline.raw_ms.push(dt);
                }
                if self.report.baseline.raw_ms.len() >= 180 {
                    lab.burst();
                    self.stage = 2;
                }
            }
            2 => {
                if let Some(dt) = dt {
                    self.report.burst.raw_ms.push(dt);
                }
                if !lab.burst.iter().any(|s| s.pending()) && lab.settle_frames == 0 {
                    self.report.completed_sources = lab
                        .burst
                        .iter()
                        .filter(|s| matches!(s.phase, Phase::Ready | Phase::Retired))
                        .count();
                    self.report
                        .errors
                        .extend(lab.burst.iter().filter_map(|s| s.error.clone()));
                    self.stage = 3;
                }
            }
            3 => {
                if let Some(dt) = dt {
                    self.report.cached.raw_ms.push(dt);
                }
                if self.report.cached.raw_ms.len() >= 180 {
                    self.stage = 4;
                }
            }
            _ => {}
        }
        if self.stage == 4 {
            self.report.baseline.finish();
            self.report.burst.finish();
            self.report.cached.finish();
            match serde_json::to_vec_pretty(&self.report)
                .map_err(anyhow::Error::from)
                .and_then(|bytes| std::fs::write(&self.path, bytes).map_err(anyhow::Error::from))
            {
                Ok(()) => eprintln!(
                    "Visible performance report: {} ({} sources, {} errors)",
                    self.path.display(),
                    self.report.completed_sources,
                    self.report.errors.len()
                ),
                Err(error) => eprintln!("Visible performance report failed: {error:#}"),
            }
            lab.sweep = false;
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cadence_summary_retains_long_stalls() {
        let mut samples = Samples {
            raw_ms: vec![16., 17., 300., 16.],
            ..Default::default()
        };
        samples.finish();
        assert_eq!(samples.raw_ms.len(), 4);
        assert_eq!(samples.p50_ms, 16.);
        assert_eq!(samples.p95_ms, 300.);
        assert_eq!(samples.max_ms, 300.);
        assert_eq!(samples.over_25_ms, 1);
        assert_eq!(samples.over_50_ms, 1);
    }
}
