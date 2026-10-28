# Quad drawing benchmark

This first app tests one workload: colored rectangles. It is deliberately small so changes to GPUI's scene and rendering APIs have a repeatable exercise.

- `grid`: rectangles occupy separate cells. Tests scene insertion with mostly disjoint bounds.
- `overlap`: all rectangles cover the same central area. Tests ordering and alpha overdraw.
- `--count`, `--opacity`, `--width` and `--height` control the workload.

Both patterns normally produce one quad batch. Overlap increases ordering depth and overdraw; it does not inherently increase batch count. This app does not test text, images, layout-heavy widgets or custom shaders.

## Just recipes and arguments

From the repository's `apps/xgpui-ext-lab/quad-bench/` directory:

```sh
just                         # List recipes; opens no window
just args                    # Print the application's CLI help
just build                   # Optimized build only
just ui --count 5000 --pattern overlap
just mtl --count 5000 --pattern overlap  # macOS Metal FPS/GPU HUD; quits when done
just mtl --exit-delay-secs 10          # Leave final frame visible for 10 seconds
just ui --continuous                   # Draw until the window is closed
just headless --pattern grid --frames 30 --json apps/xgpui-ext-lab/quad-bench/results/grid.json --png apps/xgpui-ext-lab/quad-bench/results/grid.png
just cpu --count 20000 --frames 30
just check                   # Formatting, tests and Clippy
```

From the repository root, use `just --justfile apps/xgpui-ext-lab/quad-bench/justfile <recipe>`. Recipes always run Cargo from the repository root, so **output paths are relative to the repository root**, including when invoked inside `apps/xgpui-ext-lab/quad-bench/`. The headless and CPU recipes create `apps/xgpui-ext-lab/quad-bench/results/`; other output directories must already exist. Quote paths containing spaces. The UI, Metal, headless and CPU recipes forward additional arguments as separate shell arguments. `ui` and `mtl` open a window. Repeated app flags use the last value, so arguments after a recipe override its defaults.

| Argument | Default | Meaning and limits |
| --- | --- | --- |
| `--count N` | 1000 | Workload rectangles, excluding background; 1–20000 |
| `--pattern grid\|overlap` | grid | Separate grid cells or rectangles sharing the central area |
| `--opacity F` | 0.65 | Per-rectangle opacity; finite value greater than 0 and at most 1 |
| `--width N` | 960 | Initial UI width in logical pixels; headless image width in pixels; 64–4096 |
| `--height N` | 640 | Initial UI height in logical pixels; headless image height in pixels; 64–4096 |
| `--frames N` | 60 | Measured frames in bounded UI or headless mode; 1–10000 |
| `--warmup N` | 10 | Unmeasured UI or headless frames before sampling; 0–1000 |
| `--json PATH` | none | Write a bounded UI or headless report; JSON also goes to stdout |
| `--png PATH` | none | Save the last headless rendered frame; incompatible with CPU-only mode |
| `--headless` | off | No-window execution; supplied by the headless and CPU recipes |
| `--cpu-only` | off | Skip the renderer; requires headless; supplied by the CPU recipe |
| `--exit-delay-secs N` | 0 | Seconds to retain the final UI after saving results, then quit; 0–3600; incompatible with headless and continuous modes |
| `--continuous` | off | Keep the UI drawing; incompatible with headless mode and JSON output |
| `--help` | — | Print CLI help and exit without opening a window |

Use the Cargo commands below when selecting Cargo features such as `runtime-shaders`; flags after a recipe name go to the app, not Cargo.

## Run the UI

Run commands from the **xgpui repository root** so Cargo uses its local crate patches. The validation configuration supplies the captured release's compatibility patches. The app has its own workspace and lockfile, outside the synchronized source tree.

```sh
cargo run --release --locked --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin quad-bench --target-dir target -- --count 1000 --pattern grid
```

This opens a native window, draws the warmup frames, then measures the requested frames. Drawing stops when measurement completes. The title shows DONE, measured frames, callback FPS and median CPU paint time; the full JSON summary goes to stdout and optionally `--json PATH`. The app quits automatically after printing/saving the report. Use `--exit-delay-secs 10` to retain the final image and DONE title for ten seconds before quitting. Report write failures print an error and exit unsuccessfully without waiting. The delay is outside all measured intervals. Keep the window size fixed during measurement. Change the workload by restarting with different arguments; there are no custom controls.

Use `--continuous` for ongoing visual inspection. It updates the title every 60 measured paint callbacks and produces no final report.

CPU paint time includes generating geometry and enqueueing paint operations. It excludes later scene finishing, GPU execution and presentation. Callback cadence includes scheduling and display pacing. Neither number is isolated GPU time.

## Metal frame-rate testing (macOS)

From `apps/xgpui-ext-lab/quad-bench/`:

```sh
just mtl
just mtl --count 10000 --pattern overlap --frames 1200 --warmup 120 --json apps/xgpui-ext-lab/quad-bench/results/mtl-overlap.json
```

The `mtl` recipe sets `MTL_HUD_ENABLED=1` for this process, enabling [Apple's Metal Performance HUD](https://developer.apple.com/videos/play/tech-talks/110339/). The overlay provides FPS and GPU performance metrics. It does not change global macOS settings.

Defaults for this recipe are 120 warmup frames, 600 measured frames and `apps/xgpui-ext-lab/quad-bench/results/mtl.json`. Running it again replaces that report; use distinct `--json` paths to retain runs. Duration depends on frame cadence. The app saves the report and quits at the endpoint. Use `--exit-delay-secs 10` to inspect the final image and summary before automatic exit. During that delay the HUD may show idle performance; inspect its metrics while the workload is running.

The saved windowed report measures **paint callback cadence**, not confirmed display presentations or GPU completion. Its `callback_fps` is the number of sampled intervals divided by their total duration; `callback_interval_ms` and `cpu_paint_ms` provide median/p95/min/max. The final interval ends at the next frame callback after the final paint was submitted. HUD numbers are separate and are not exported into this JSON. Headless JSON keeps its existing composition/render/readback fields; the two report modes are distinct.

The benchmark disables GPUI’s inactive-window animation throttle (`WindowOptions::inactive_frame_interval: None`) in both bounded and continuous UI modes. Windowed report schema version 2 records `inactive_throttling_disabled`, `focused_frames`, `unfocused_frames` and `sampled_focus_transitions`. Focus is sampled at each measured paint callback; warmup and post-completion paints are excluded, and transitions occurring entirely between samples are not detected.

Display refresh, pacing, window scale, occlusion and HUD overhead still affect results. Keep the same display configuration and window size, keep the window visible, and use HUD-enabled runs only against other HUD-enabled runs. The M4 MacBook Pro with 128 GB RAM is a high-end reference machine; these timings do not establish performance on smaller machines. This is not yet an approved baseline or an automated regression gate.

## Run without a window (agents and automation)

```sh
mkdir -p apps/xgpui-ext-lab/quad-bench/results
cargo run --release --locked --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin quad-bench --target-dir target -- --headless --count 1000 --pattern grid --frames 60 --warmup 10 --json apps/xgpui-ext-lab/quad-bench/results/grid.json --png apps/xgpui-ext-lab/quad-bench/results/grid.png
```

Repeat with `--pattern overlap` and different output names. `results/` is ignored by Git. Output parent directories must already exist. JSON is also printed to stdout.

The headless path renders real scenes through GPUI's offscreen renderer (Metal on macOS; wgpu on Linux where supported). It opens no window. Add `--cpu-only` to measure scene work without initializing a renderer; omit `--png` in that mode. Windows currently requires `--cpu-only` for headless execution. This native CLI is not a WASM app.

JSON contains warmup-excluded median, nearest-rank p95, minimum and maximum:

| Field | Measurement |
| --- | --- |
| `compose_us` | Geometry generation, allocation and insertion into a new scene |
| `finish_us` | Scene finishing and sorting |
| `batch_traversal_us` | Traversing and counting finished primitive batches |
| `render_and_readback_ms` | Offscreen target allocation, CPU encoding, GPU completion and image readback together |

Readback time is an end-to-end offscreen measurement, **not GPU timestamp timing or window FPS**. Image validation and PNG/JSON writing are outside these measured intervals. Every rendered image is checked for the requested dimensions and opaque pixels. At opacity 0.1 or greater it also checks for colored workload pixels; lower opacity can round away in an 8-bit image.

The headless workload uses a fixed scale factor of 1; UI drawing uses the window's logical pixels and native scale. Compare headless runs with identical size, count, opacity, backend and build profile. Use release builds for performance comparisons. The report records OS, architecture, upstream source commit and whether optimizations are enabled. It does not record GPU model, power state or compiler version; record those separately when establishing a baseline. No regression thresholds are assumed yet.

## Validate

```sh
cargo fmt --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin quad-bench --check
cargo test --locked --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin quad-bench --target-dir target
cargo clippy --locked --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin quad-bench --target-dir target --all-targets -- -D warnings
```

Tests check geometry, scene primitive counts, overlap ordering, percentile calculation and input validation. Headless rendering provides a separate backend exercise. The app enables `test-support` to access GPUI's offscreen renderer. On macOS, add `--features runtime-shaders` before `--` to compile shaders at runtime instead of using the default build-time Metal shader compilation.
