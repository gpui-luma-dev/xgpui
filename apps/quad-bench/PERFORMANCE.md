# Performance comparison protocol

This protocol separates display-paced window callbacks from CPU scene work and synchronized offscreen rendering. It establishes candidate reference measurements, not an automatic release gate.

## Fixed cases

Use an optimized, locked build with default build-time shaders and no Metal HUD. Run each case in a fresh process five times, sequentially. Do not run cases concurrently.

| Mode | Pattern | Quad count | Warmup | Measured frames | Size |
| --- | --- | --- | --- | --- | --- |
| Headless | grid | 1000 | 30 | 300 | 960 × 640 physical pixels |
| Headless | overlap | 1000 | 30 | 300 | 960 × 640 physical pixels |
| Headless | grid | 10000 | 30 | 300 | 960 × 640 physical pixels |
| Headless | overlap | 10000 | 30 | 300 | 960 × 640 physical pixels |
| Windowed | grid | 1000 | 120 | 600 | 960 × 640 logical pixels |

All cases use opacity 0.65. For each headless trial, run the four cases in table order; repeat that sequence five times. Then run the windowed case five times. Windowed and headless resolutions differ at 2× scale: their render times are not directly comparable.

From the repository root, first build with `just --justfile apps/quad-bench/justfile build`. Then invoke `target/release/quad-bench` directly for each case. Example with a process timeout:

```sh
mkdir -p apps/quad-bench/results/my-run
MTL_HUD_ENABLED=0 perl -e 'alarm 45; exec @ARGV or die $!;' target/release/quad-bench --headless --pattern grid --count 1000 --warmup 30 --frames 300 --json apps/quad-bench/results/my-run/headless-grid-1000-1.json
MTL_HUD_ENABLED=0 perl -e 'alarm 45; exec @ARGV or die $!;' target/release/quad-bench --pattern grid --count 1000 --warmup 120 --frames 600 --json apps/quad-bench/results/my-run/ui-grid-1000-1.json
```

The timeout applies to the benchmark executable itself. Do not wrap Cargo or just with this alarm: terminating a parent runner may leave its application child alive. Preserve exit status and stderr for every process. A timeout or missing report is a failed trial, never a zero-time result. The timeout is a suite safety measure, not a performance threshold.

The app activates its window on startup and disables GPUI's inactive-window throttle. Do not minimize, cover or interact with it during a windowed trial. Activation does not guarantee uninterrupted frame delivery; inspect focused/unfocused frame counts and maximum callback interval. Runs with long pauses need investigation and must remain in the evidence.

## Preserve evidence

Store individual JSON reports, exit statuses, logs, protocol and summary in a new directory under `apps/quad-bench/results/`. That directory is ignored by Git and is local, not backed up by pushing the repository.

Record the exact CPU/GPU and core count, RAM, OS/build, display resolution/refresh/scale, AC/battery source, power settings, compiler version, shader mode, GPUI revision, repository revision and hashes of the benchmark source, lockfile and Cargo configuration. Record local changes explicitly. Do not publish machine serial numbers or other unrelated system details.

For each metric, summarize the median and range of the five per-run medians. Summarize per-run p95 values separately; their median is not a pooled p95. Retain maximum frame times so a smooth median cannot conceal a stalled run. Five trials provide an initial view of variability, not a reliable confidence interval.

Windowed callback FPS is primarily a pacing/continuity check on a 60 Hz display. Use headless composition/sorting/batch timings and render/readback timings to compare workload costs. Render/readback includes CPU encoding, allocations, GPU completion and copying; it is not isolated GPU execution time. This protocol does not measure memory consumption or general UI performance.

## Compare a new GPUI release

Run old and new GPUI revisions on the same machine with the same benchmark code, compiler, configuration and workload. Re-run the old version alongside the new one to distinguish changes from machine drift. A historical result alone is insufficient after OS, compiler or power-setting changes.

Report absolute and percentage changes per metric and case, alongside within-version variation. Do not select a universal percentage threshold before measuring noise. Repeated regressions require investigation; do not replace a baseline just to make a comparison pass.

Approved reference summaries and supporting reports belong in version-controlled `performance/baselines/<machine>/<app>/<release>/`. Promotion is explicit and reviewed. Temporary attempts stay in `results/`; no baseline has been promoted by this protocol. Keep the old approved reference when adding a new one. Other hardware needs its own reference series.
