//! Native-only, deterministic CPU baseline; no graphics context is required.
use super::*;
use serde_json::json;
use std::{hint::black_box, path::Path, time::Instant as Clock};

pub fn benchmark(output: &Path) -> anyhow::Result<()> {
    let mut trials = Vec::new();
    for sigma in [0.1, 0.5, 0.9] {
        for trial in 0..6 {
            let start = Clock::now();
            let points = black_box(sample_trace(sigma));
            let evaluation_ms = start.elapsed().as_secs_f64() * 1000.;
            let parameters: Vec<_> = (0..=TRACE_STEPS)
                .map(|i| TRACE_MAX * i as f64 / TRACE_STEPS as f64)
                .collect();
            let mut options = ParameterizedFitOptions::default();
            options.tolerance = FIT_TOLERANCE;
            options.max_segments = 4096;
            let start = Clock::now();
            let fit = black_box(fit_parameterized(&points, &parameters, options)?);
            let fitting_ms = start.elapsed().as_secs_f64() * 1000.;
            anyhow::ensure!(
                fit.diagnostics().tolerance_met,
                "fit failed tolerance at sigma={sigma}"
            );
            let start = Clock::now();
            let fit = black_box(FittedTrace::from_fit(fit));
            let measurement_ms = start.elapsed().as_secs_f64() * 1000.;
            let start = Clock::now();
            let mut vertices = 0;
            let transform = Affine::new([100., 0., 0., -100., 350., 300.]);
            let rect = Rect::new(0., 0., 900., 600.);
            for span in &fit.spans {
                let path = clip_to_rect(&(transform * &span.path), rect);
                if let Some(path) =
                    xgpui_curve::gpui_paint::stroke_path(&path, Affine::IDENTITY, 1.25)
                {
                    vertices += black_box(path.vertices.len());
                }
            }
            let path_build_ms = start.elapsed().as_secs_f64() * 1000.;
            if trial > 0 {
                trials.push(json!({"sigma":sigma,"trial":trial,"evaluation_ms":evaluation_ms,
                "fitting_ms":fitting_ms,"measurement_ms":measurement_ms,"path_build_ms":path_build_ms,
                "samples":points.len(),"segments":fit.fit.diagnostics().segments,"max_paired_error":fit.fit.diagnostics().max_sample_error,"vertices":vertices}));
            }
        }
    }
    let report = json!({"schema":1,"os":std::env::consts::OS,"arch":std::env::consts::ARCH,
        "build":if cfg!(debug_assertions) {"debug"}else{"optimized"},"trace_max":TRACE_MAX,
        "fit_tolerance":FIT_TOLERANCE,"warmup_trials_per_sigma":1,"measured_trials_per_sigma":5,
        "notes":"CPU only. Evaluation excludes grid/zero detection; measurement includes per-segment trim caches. Path build clips and tessellates the complete fitted trace at 100 px/unit in a 900x600 viewport. No GPU, window, presentation, readback, or end-to-end FPS is measured.","trials":trials});
    if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(output, serde_json::to_vec_pretty(&report)?)?;
    println!("Wrote zeta CPU baseline to {}", output.display());
    Ok(())
}
