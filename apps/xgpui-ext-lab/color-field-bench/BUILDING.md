# Color-field quad benchmark

This app tests one operation from **gpui-luma-color**: painting a rectangular HSV saturation/value field at a fixed hue. It compares that field's layered fast path with its sampled-quad fallback. This is a standalone adaptation of the drawing algorithms, not a dependency on the full SDK or a reproduction of its color picker controls.

## Source evidence

The inspected source is `gpui-luma/crates/luma-color/src/color_field/field/paint_vector.rs`, at gpui-luma revision `b5a4a23ce6ed98731c1e3aef0abec02da4879274`:

- `paint_sv_rect_background`: an opaque hue quad, a horizontal white-to-transparent gradient, then a vertical transparent-to-black gradient.
- `paint_domain_background`: an aspect-aware grid whose cells sample the model at their midpoints.
- `color_field/model.rs::SvAtHueModel`: saturation is horizontal position; value is one minus vertical position.

The sampled method deliberately bypasses the source's rectangle fast-path dispatch so the two strategies can be compared for the same field. The normal source implementation selects the fast path for this rectangular SV model.

Changes in this adaptation are explicit: no domain/controller/SDK dependencies; rectangular domains only; direct bounded HSV-to-sRGB conversion instead of the source's palette/gamut bridge; shared generated PaintQuad data for UI and headless rendering; deterministic hue animation; separate stage timings and analytical image checks. It does not measure palette conversion overhead, high-gamut output, cached-image rendering, control layout, thumbs, borders or pointer handling. `workload.rs` carries the adaptation notice; the source's Apache-2.0 license is included.

## Commands

From `apps/xgpui-ext-lab/color-field-bench/`:

```sh
just                         # List commands; no window
just args                    # CLI help; no window
just build                   # Optimized build only
just ui --method layered --animate
just ui --method sampled --animate --exit-delay-secs 10
just mtl --method sampled     # macOS: animated field with Metal FPS/GPU HUD
just headless --method layered --animate --frames 300 --warmup 30 --json apps/xgpui-ext-lab/color-field-bench/results/layered.json --png apps/xgpui-ext-lab/color-field-bench/results/layered.png
just cpu --method sampled --frames 300 --warmup 30
just benchmark release-1.25.2-candidate-1
just check
```

From the repository root, use `just --justfile apps/xgpui-ext-lab/color-field-bench/justfile <recipe>`. Recipes run Cargo from that root to use the local crate and compatibility patches. **All relative output paths are relative to the repository root.** The headless, CPU and Metal recipes create `apps/xgpui-ext-lab/color-field-bench/results/`; other parent directories must already exist. Quote paths containing spaces. Extra flags go to the app, not Cargo. The last repeated flag wins.

`ui` and `mtl` open windows. Bounded UI runs print/save the report, then quit; `--exit-delay-secs 10` retains the final frame for ten seconds after saving. They activate the window at startup and disable GPUI's inactive-window throttle. A 60-second watchdog exits unsuccessfully if no report has been saved; it does not count the post-save delay. Report errors also exit unsuccessfully.

For continuous visual inspection, use `just ui --continuous --animate`. To include the Metal HUD, use `MTL_HUD_ENABLED=1 just ui --continuous --animate`. Continuous mode has no final report or automatic exit; it conflicts with JSON output. The `mtl` recipe supplies JSON output and animation, so use the `ui` recipe for continuous or fixed-hue HUD inspection. HUD metrics are separate from JSON timing data. See [Apple's Metal HUD documentation](https://developer.apple.com/videos/play/tech-talks/110339/).

## Arguments

| Argument | Default | Meaning |
| --- | --- | --- |
| `--method layered\|sampled` | layered | Three gradient/solid layers, or midpoint-sampled solid color cells |
| `--samples N` | 128 | Sample target along the longest axis, 16–512; applies to sampled only |
| `--hue F` | 210 | Initial hue in degrees; finite, 0 inclusive to 360 exclusive |
| `--animate` | off | Add one hue degree per drawn frame, including warmup; wraps at 360 |
| `--width N` / `--height N` | 512 / 384 | Initial UI logical size or headless physical image size; each 64–1024 |
| `--frames N` | 600 | Measured bounded-run frames; 1–10000 |
| `--warmup N` | 120 | Frames excluded before measurement; 0–1000 |
| `--headless` | off | Offscreen rendering and image validation; opens no window |
| `--cpu-only` | off | Headless generation and scene preparation without a GPU; requires headless |
| `--json PATH` | none | Save the completed report; JSON is also printed to stdout |
| `--png PATH` | none | Save final offscreen image; requires headless, conflicts with CPU-only |
| `--continuous` | off | UI visual inspection until closed; conflicts with headless, JSON and exit delay |
| `--exit-delay-secs N` | 0 | UI post-save viewing delay, 0–3600 seconds |
| `--timeout-secs N` | 60 | Bounded UI deadline before saving; 0 disables it; maximum 3600; unused in headless/continuous modes |

The sampled algorithm preserves the source's minimum 0.25 sampling scale. At 512 × 384 and `--samples 128`, it creates a 128 × 96 grid: **12,288 quads**. Reducing `--samples` below 128 at this size does not reduce that grid. The layered method always generates **3 quads**. Reports include the actual sampling grid and primitive count. Neither path adds a background quad.

## What the numbers mean

- `generate_us`: allocation, HSV color evaluation/conversion and PaintQuad generation. Both modes use the same generator.
- Headless `compose_us`: conversion to scene primitives and Scene insertion, excluding generation. `finish_us` is scene finishing/sorting.
- Headless `render_readback_ms`: offscreen target allocation, CPU encoding, GPU completion and copying to a CPU image. It is **not isolated GPU execution time**.
- UI `cpu_paint_ms`: calls to Window::paint_quad, excluding generation. `callback_interval_ms` and `callback_fps` measure paint-callback scheduling and platform pacing, not confirmed display presentations.
- The three-layer CPU stages can be at or below timer resolution (including zero finish medians). Do not interpret relative percentages for these tiny timings; repeated tests or amortized measurements are needed before gating them.
- Unsupported/unmeasured stages are `null`. Focus counts apply only to measured UI paint callbacks. The report records both first and last measured hues.

Headless mode uses scale 1 and validates nine interior pixels per frame against an analytical HSV SV field. Tolerance allows quantization, dithering and the sampled path's bounded cell approximation. These are targeted correctness checks, not an exhaustive color-accuracy or gamut test. Image checking and PNG/JSON writes are outside measured intervals. UI rendering uses native scale and pixel snapping, so a 2× window is a different physical workload. Keep dimensions, scale, sampling, animation, compiler and shader mode fixed for comparisons; keep a window visible and its size unchanged.

The current offscreen factory supports Metal on macOS and wgpu on Linux where available. Other targets can use CPU-only mode if the platform crates build; the GUI and CLI have not been validated there. This app is a native executable, not a WASM demo. On macOS, add `--features runtime-shaders` to direct Cargo commands to select runtime Metal shader compilation; default recipes use build-time shaders.

## Repeated release comparisons

`just benchmark <new-run-name>` builds once and runs five interleaved trials of each method: 512 × 384, samples 128, starting hue 210, animation enabled, 30 warmup and 300 measured frames. It saves every report, log and exit status, plus source/binary hashes, compiler/revision data and macOS machine/display/power metadata. An existing run directory is rejected instead of overwritten. The recipe requires Bash, Perl and shasum; each executable inherits a 60-second process alarm, including headless runs.

Output lives in `apps/xgpui-ext-lab/color-field-bench/results/<new-run-name>/`, ignored by Git. It is local evidence and is not backed up by a push. For a release comparison, run old and new GPUI revisions with the same app code and environment. Compare the range of per-run medians and p95 values; do not silently discard timeouts or stalls. Do not compare against quad-bench as if it were the same workload. Approved baselines should be explicitly reviewed and copied into version-controlled `performance/baselines/<machine>/color-field-bench/<release>/`; no baseline is promoted automatically.
