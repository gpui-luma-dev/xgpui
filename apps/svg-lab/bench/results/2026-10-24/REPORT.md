# SVG corpus measurements — 2026-10-24 (UTC run date)

[Extraction assessment](ASSESSMENT.md) accompanies this recorded run.

## Method

Release GPUI/resvg 0.46.0 CPU renderer on Apple M4 Max, 128 GiB RAM, macOS-27.0.1-arm64-arm-64bit-Mach-O. One serial, fresh subprocess per document/resolution; 2× display density; zoom 100%, 400%, 800%, 1600%. The artwork’s longest edge is 320 logical pixels at 100%, yielding nominal edges 640, 2560, 5120 and 8192 pixels (last is capped). Pixel dimensions are rounded up; floating-point normalization can add one pixel at uncapped sizes, so raw.json records actual dimensions.

Each worker measures first parse and five repeated parses, keeps a parsed tree, renders a fixed 320px base and the first zoom image, then measures five replacements while retaining the previous image. Tables show median of the five replacements. First zoom render is after the base render, not a cold rasterizer. Raw JSON retains all samples and maxima; five samples do not establish a reliable p95.

RSS comes from sysinfo; peak RSS is the kernel getrusage lifetime high-water mark, including renderer/fonts, parse repetitions, base, previous image, replacement and temporary filter buffers. It excludes GPU atlas/upload/presentation and OS file-picker/UI overhead. Exact image bytes are separate from whole-process RSS. RSS after drop can retain allocator pages; preview encoding occurs after the recorded peak and can affect the post-drop snapshot. File reads and renderer initialization are reported separately, outside parse/raster timings.

Sources and SHA-256 pins: [corpus.json](../../corpus.json). Artwork is fetched locally rather than redistributed; respect each original work’s license. Measurements are a single machine/run, not renderer rankings or cross-platform guarantees.

## Raster latency

| Artwork | Paths | Gradients | Filters | 100% ms | 400% ms | 800% ms | 1600% ms |
|---|---:|---:|---:|---:|---:|---:|---:|
| tiger | 240 | 0 | 0 | 5.68 | 26.22 | 69.47 | 145.58 |
| gallardo | 280 | 281 | 33 | 24.06 | 204.56 | 943.62 | 2619.26 |
| AJ_Digital_Camera | 58 | 175 | 0 | 11.38 | 120.78 | 442.93 | 1100.24 |
| USStates | 52 | 0 | 0 | 1.98 | 12.16 | 37.35 | 86.34 |
| rg1024_green_grapes | 76 | 40 | 2 | 36.59 | 170.56 | 695.36 | 1888.01 |
| rg1024_metal_effect | 126 | 18 | 6 | 32.28 | 288.79 | 1563.38 | 4283.01 |
| displayWebStats | 50 | 7 | 0 | 5.67 | 35.43 | 110.24 | 253.41 |
| dragon | 31 | 0 | 0 | 3.34 | 17.69 | 48.63 | 103.67 |
| Steps | 18 | 0 | 0 | 1.48 | 11.64 | 39.15 | 93.94 |
| ny1 | 5 | 0 | 0 | 2.84 | 10.09 | 23.73 | 46.17 |

## Memory and parsing

Peak numbers below are for each document’s largest zoom worker. Image bytes = base + previous zoom + replacement (no filter intermediates).

| Artwork | First parse ms | Repeated parse median ms | Largest image px | One output MiB | Retained/replacement images MiB | Peak process RSS MiB |
|---|---:|---:|---|---:|---:|---:|
| tiger | 0.87 | 0.76 | 7397 × 8192 | 231.2 | 462.7 | 474.4 |
| gallardo | 2.96 | 2.75 | 8192 × 4387 | 137.1 | 274.4 | 766.4 |
| AJ_Digital_Camera | 1.09 | 0.87 | 8192 × 4160 | 130.0 | 260.2 | 359.4 |
| USStates | 8.71 | 1.79 | 8192 × 4916 | 153.6 | 307.5 | 320.5 |
| rg1024_green_grapes | 0.71 | 0.53 | 5849 × 8192 | 182.8 | 365.8 | 649.8 |
| rg1024_metal_effect | 0.56 | 0.41 | 7611 × 8192 | 237.8 | 476.1 | 1377.6 |
| displayWebStats | 13.94 | 6.59 | 8192 × 4608 | 144.0 | 288.2 | 411.2 |
| dragon | 0.30 | 0.19 | 7525 × 8192 | 235.2 | 470.7 | 481.9 |
| Steps | 10.93 | 3.61 | 8192 × 8192 | 256.0 | 512.4 | 525.5 |
| ny1 | 15.80 | 8.32 | 8192 × 5298 | 165.6 | 331.4 | 345.9 |

## Compatibility and visual inspection

Successful parsing and nonzero pixels are smoke checks, not SVG conformance. The preview for each document is rendered at 100% (640px edge). Text uses the machine’s system fonts, so substitutions/layout can vary. Scripts and SVG animation are not executed.

| Artwork | Script elements | Animation elements | External href references | Visible pixels at 100% |
|---|---:|---:|---:|---:|
| tiger | 0 | 0 | 0 | 212654 |
| gallardo | 0 | 0 | 0 | 145304 |
| AJ_Digital_Camera | 0 | 0 | 0 | 188817 |
| USStates | 1 | 0 | 0 | 138000 |
| rg1024_green_grapes | 0 | 0 | 0 | 137953 |
| rg1024_metal_effect | 0 | 0 | 0 | 380800 |
| displayWebStats | 5 | 83 | 17 | 205847 |
| dragon | 0 | 0 | 0 | 170436 |
| Steps | 1 | 0 | 0 | 219906 |
| ny1 | 0 | 0 | 0 | 50923 |

USStates is an interactive quiz: the static map and initial instructions render, but clicking Begin cannot run its script. Steps is a script-driven game, and displayWebStats is script/animation-dependent with external JS/data references; their timing measures static initial output only, not the working applications. The other seven files provide static artwork/chart coverage. No corpus file was altered to remove unsupported behavior.

This corpus does not cover embedded raster images, external image resolution, every font/fallback script, or adversarial/decompression-heavy input. Those remain separate coverage gaps.

## Reproduce

```sh
python3 apps/svg-lab/bench/fetch.py /tmp/xgpui-svg-corpus
cargo build --release --offline --locked --config .cargo/validation.toml --manifest-path apps/svg-lab/Cargo.toml --target-dir target
python3 apps/svg-lab/bench/run.py --corpus /tmp/xgpui-svg-corpus --output /tmp/svg-bench-rerun
python3 apps/svg-lab/bench/report.py /tmp/svg-bench-rerun
```

Remove --offline when dependencies are not cached. Each worker has a 120-second timeout; failures/timeouts are retained in raw.json. Downloaded source artwork and PNG previews remain under /tmp; a fresh fetch verifies pinned hashes. Open those SVG files through the lab’s Open SVG control for interactive inspection.
