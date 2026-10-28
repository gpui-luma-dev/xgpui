# Extraction assessment

The experiment supports a reusable zoom-aware SVG element, but the current 8192px cap is too costly as a default policy. Keep the implementation in the lab until its resource and scheduling contract is explicit.

## Evidence

All 40 workers completed; none produced an empty image. All ten 640px previews were visually inspected for recognizable output, clipping, gradients, filters and text. The text chart was also loaded into the release lab at 400% and panned, confirming synchronized crops and readable black labels on the new light canvas. This is a smoke inspection, not pixel parity with a reference browser. Seven files are static artwork/charts; three retain only their static initial state because they depend on scripts/animation.

At 2× density, the seven static documents have median raster times of 2.84–36.59ms at 100%, 10.09–288.79ms at 400%, and 46.17–4283.01ms at the capped 1600%. A parsed-tree cache helps, but rasterization and filter intermediates dominate the difficult cases. The metal artwork has only 126 paths yet is much slower than the 240-path tiger: path count alone is not a useful complexity budget.

Metal at 7611×8192 allocates 237.8 MiB for one output; retaining base + previous + replacement accounts for 476.1 MiB, while whole-process peak reaches 1377.6 MiB. The car similarly reaches 766.4 MiB versus 274.4 MiB in retained/replacement image buffers. An output byte count cannot describe filter working memory.

## Proposed extension contract

- Parse once; retain intrinsic size and report parsing failures without replacing the last working document.
- Keep the current texture while the camera moves. Debounce/coalesce wheel requests and allow one raster job at a time. Dropping obsolete results is necessary but cannot interrupt an in-progress resvg call; a large stale job can delay the latest request for seconds.
- Use a configurable pixel/byte budget, not just a per-edge cap. Evaluate 16 and 32 MiB output budgets before choosing a default; both previous and replacement images, plus filter headroom, need accounting. Those budgets are proposals, not validated peak-RSS guarantees.
- Render a modest preview before requesting full detail. Expose requested versus actual resolution and pending/limited state rather than implying every zoom is immediately sharp.
- Preserve aspect ratio, clipping and alpha, with a caller-selected backdrop. The poll chart exposed unreadable black labels on the lab's dark background; imported files now start on a light canvas and can toggle light/dark.
- Explicitly support static SVG. Interactive applications, external-resource resolution and font portability require separate API decisions.

Before extraction, test the chosen byte budget and debounce policy under a recorded rapid-zoom sequence; measure final-request completion latency and UI/GPU residency separately. Consider viewport tiling/cropping if very deep zoom is a requirement, rather than repeatedly rasterizing an entire offscreen document. Embedded raster images, broader fonts/scripts and pathological filter/input cases remain coverage gaps.

See [raw measurements and methodology](REPORT.md) and [all samples](raw.json). No `xgpui-svg` crate was extracted.
