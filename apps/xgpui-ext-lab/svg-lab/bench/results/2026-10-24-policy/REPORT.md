# Bounded progressive SVG policy — 2026-10-24

## Decision

Use a **16 MiB default output budget**, a 640px preview edge and 120ms settling delay. Keep 32 MiB as an explicit quality option. The lab now uses the extracted `xgpui-svg` document/policy/queue layer. This is a host-driven raster backend, not an SVG DOM or a turnkey viewer widget.

## Method

Apple M4 Max, 128 GiB RAM, release build, 2× density, serial isolated CPU workers. The same ten pinned documents as the baseline. Each budget has three steady raster cases (100/400/1600%, five samples each) and a burst case: **80 total cases**. All succeeded and every final output obeyed its byte budget. Raw samples and job timelines are in [policy-raw.json](policy-raw.json). These were measured before mechanical extraction; [post-extraction spot checks](post-extraction.json) verify the extracted queue on the car and metal files at both budgets.

Steady replacement runs retain base + previous zoom + replacement. Burst runs retain base and the actual current image, so their peaks differ from repeated large-image replacements. RSS includes parsing/fonts/filter intermediates. GPU/UI memory and frame latency are not measured. The three script-dependent documents still represent static initial state only.

The burst requests zoom 1, 2, 4, 8, 16, 4, 16 at 0, 20, 40, 60, 80, 100, 120ms. It uses the same RasterQueue as the UI, executed on a worker thread with a 1ms host polling interval. The base is available before the clock starts. Detail waits 120ms after the latest **different output resolution**; repeated budget-capped targets do not restart the timer. Same-document coarse previews remain useful during the burst; stale detail or document/budget results are discarded. This tests scheduling and CPU completion, not UI event delivery.

## Steady raster at 1600%

| Artwork | Legacy ms | 16 MiB ms | 32 MiB ms | Legacy peak RSS MiB | 16 MiB peak RSS MiB | 32 MiB peak RSS MiB |
|---|---:|---:|---:|---:|---:|---:|
| tiger | 145.6 | 20.5 | 32.3 | 474.4 | 43.9 | 75.8 |
| gallardo | 2619.3 | 235.6 | 526.8 | 766.4 | 103.0 | 184.4 |
| AJ_Digital_Camera | 1100.2 | 145.8 | 279.4 | 359.4 | 53.0 | 95.7 |
| USStates | 86.3 | 12.1 | 21.4 | 320.5 | 45.2 | 77.3 |
| rg1024_green_grapes | 1888.0 | 150.9 | 311.6 | 649.8 | 66.5 | 131.0 |
| rg1024_metal_effect | 4283.0 | 195.9 | 438.8 | 1377.6 | 100.3 | 194.5 |
| displayWebStats | 253.4 | 41.4 | 72.4 | 411.2 | 58.7 | 102.9 |
| dragon | 103.7 | 13.8 | 21.6 | 481.9 | 43.3 | 75.4 |
| Steps | 93.9 | 8.1 | 14.1 | 525.5 | 45.6 | 77.5 |
| ny1 | 46.2 | 9.9 | 14.9 | 345.9 | 46.7 | 78.6 |

## Rapid zoom — default 16 MiB

Every burst issued **two jobs: one preview and one settled detail**, rather than rendering every intermediate zoom. Panning and zooming out to a smaller target reuse the current texture.

| Artwork | Preview ready ms from first request | Final detail ms after last request | Burst peak RSS MiB | Final image px |
|---|---:|---:|---:|---|
| tiger | 26.5 | 73.4 | 29.4 | 1946 × 2155 |
| gallardo | 45.4 | 375.5 | 83.8 | 2798 × 1498 |
| AJ_Digital_Camera | 32.5 | 277.5 | 38.2 | 2874 × 1459 |
| USStates | 22.9 | 139.5 | 30.3 | 2643 × 1586 |
| rg1024_green_grapes | 61.7 | 216.0 | 44.9 | 1730 × 2423 |
| rg1024_metal_effect | 54.2 | 250.9 | 88.1 | 1974 × 2124 |
| displayWebStats | 26.5 | 188.2 | 43.7 | 2730 × 1536 |
| dragon | 23.9 | 147.4 | 28.6 | 1962 × 2136 |
| Steps | 22.8 | 57.1 | 31.1 | 2048 × 2048 |
| ny1 | 24.1 | 147.4 | 31.7 | 2546 × 1646 |

## Contract and limits

The output budget applies independently to one RGBA image. Fixed base, current image, replacement, parsed tree, filter scratch buffers and GPUI GPU resources consume additional memory. **16 MiB is not a 16 MiB process-memory cap.** When lowering a budget, the lab releases an oversized current image to its base; an already-running older job drains before new detail can start. resvg has no cooperative mid-call cancellation in this integration.

The new default deliberately sacrifices full-resolution deep zoom. The UI exposes requested, budget target and actual displayed pixel dimensions, plus pending/limited/error state. Caller-selected light/dark backgrounds, aspect ratio, SVG clipping, full color and synchronized camera controls are retained. Normalization now pins the longest logical edge exactly, avoiding the baseline’s occasional one-pixel floating-point overshoot.

The scheduling tests cover stale generations, failures without retry loops, duplicate results, one active job, preview acceptance, quiet-period detail, and texture reuse. Pixel-budget tests include square, portrait, landscape, tiny budgets, extreme aspect ratios and invalid inputs. The document renderer independently rejects over-budget jobs.

Viewport tiling, external-resource policy, hard filter-memory limits, GPU residency and UI frame instrumentation remain future work; this initial extraction does not claim those capabilities.

## Reproduce

```sh
cargo build --release --offline --locked --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin svg-lab --target-dir target
python3 apps/xgpui-ext-lab/svg-lab/bench/policy_run.py --output /tmp/svg-policy-results
python3 apps/xgpui-ext-lab/svg-lab/bench/policy_report.py /tmp/svg-policy-results
```

The report generator expects the sibling baseline results for the comparison table; use `--baseline` to select it explicitly.

Live release-lab checks: loaded the metal SVG, switched to 1600%, changed 16→32→16 MiB, panned the shared camera and observed final detail at 1974×2124 with Ready / Resolution limited status. This verifies behavior, not UI/GPU memory or frame timing.
