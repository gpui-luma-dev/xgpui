//! Bounded native review captures using the same interactive view as the lab.
use super::*;
use serde_json::json;
use std::time::{Duration, Instant};

pub fn capture(
    window: WindowHandle<xgpui_ext_lab::LabWindow>,
    lab: Entity<curve::CurveLab>,
    directory: PathBuf,
    cx: &mut App,
) {
    // AnyWindowHandle does not borrow the root entity across explicit draws.
    let window: AnyWindowHandle = window.into();
    cx.spawn(async move |cx| {
        let result: anyhow::Result<()> = async {
            std::fs::create_dir_all(&directory)?;
            let timer = cx.background_executor().clone();
            timer.timer(Duration::from_millis(500)).await;
            let mut frames = Vec::new();
            let mut hashes = std::collections::HashSet::new();
            let states = [
                ("01-grid", 0., 0.5),
                ("02-mapping", 0.25, 0.5),
                ("03-mapped", 0.5, 0.5),
                ("04-trace", 0.72, 0.5),
                ("05-complete", 1., 0.5),
                ("06-offset-line", 0.72, 0.7),
            ];
            for (name, phase, sigma) in states {
                window.update(cx, |_, _, cx| {
                    lab.update(cx, |lab, cx| {
                        lab.zeta_snapshot(phase, sigma);
                        cx.notify();
                    });
                })?;
                timer.timer(Duration::from_millis(80)).await;
                window.update(cx, |_, window, cx| -> anyhow::Result<()> {
                    // Warm layout/font caches, then draw the same paused state.
                    // GPU re-render/readback is timed separately from CPU draw.
                    window.draw(cx).clear(cx);
                    let start = Instant::now();
                    window.draw(cx).clear(cx);
                    let cpu_draw_ms = start.elapsed().as_secs_f64() * 1000.;
                    let start = Instant::now();
                    let image = window.render_to_image()?;
                    let gpu_render_and_readback_ms = start.elapsed().as_secs_f64() * 1000.;
                    let width = image.width();
                    let height = image.height();
                    anyhow::ensure!(width > 0 && height > 0, "empty capture");
                    let fingerprint = image.as_raw().iter().fold(
                        14695981039346656037u64,
                        |hash, byte| (hash ^ u64::from(*byte)).wrapping_mul(1099511628211),
                    );
                    anyhow::ensure!(hashes.insert(fingerprint), "capture did not change for {name}");
                    anyhow::ensure!(
                        image.pixels().any(|p| p[0] > 100 && p[1] > 100 && p[2] > 100),
                        "capture has no visible foreground"
                    );
                    image.save(directory.join(format!("{name}.png")))?;
                    frames.push(json!({
                        "name": name, "phase": phase, "sigma": sigma,
                        "width": width, "height": height, "cpu_draw_ms": cpu_draw_ms,
                        "gpu_render_and_readback_ms": gpu_render_and_readback_ms,
                    }));
                    Ok(())
                })??;
            }
            let report = json!({
                "schema": 1, "os": std::env::consts::OS, "arch": std::env::consts::ARCH,
                "logical_window": [1280, 900], "frames": frames,
                "notes": "Fixed paused states. CPU draw includes UI composition/path construction; GPU rendering and synchronous image readback are combined. These are capture costs, not presentation timings or end-to-end FPS. PNG encoding is excluded. Pixel dimensions follow display scale.",
            });
            std::fs::write(directory.join("captures.json"), serde_json::to_vec_pretty(&report)?)?;
            println!("Wrote six zeta review captures to {}", directory.display());
            Ok(())
        }.await;
        if let Err(error) = result {
            eprintln!("zeta capture failed: {error:#}");
            std::process::exit(1);
        }
        cx.update(|cx| cx.quit());
    }).detach();
}
