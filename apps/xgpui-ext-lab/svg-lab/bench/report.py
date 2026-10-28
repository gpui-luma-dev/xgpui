#!/usr/bin/env python3
"""Produce a compact, reviewable report without hiding failed/blank cases."""
import json
from pathlib import Path
import sys
p = Path(sys.argv[1])
j = json.loads((p / "raw.json").read_text())
by_id = {}
for row in j["cases"]:
    by_id.setdefault(row["id"], []).append(row)
lines = [f"# SVG corpus measurements — {j['metadata']['started_utc'][:10]} (UTC run date)", "", ("[Extraction assessment](ASSESSMENT.md) accompanies this recorded run." if (p / "ASSESSMENT.md").exists() else ""), "", "## Method", "",
         f"Release GPUI/resvg 0.46.0 CPU renderer on {j['metadata'].get('host_label', 'CPU/RAM not recorded')}, {j['metadata']['platform']}. One serial, fresh subprocess per document/resolution; 2× display density; zoom 100%, 400%, 800%, 1600%. The artwork’s longest edge is 320 logical pixels at 100%, yielding nominal edges 640, 2560, 5120 and 8192 pixels (last is capped). Pixel dimensions are rounded up; floating-point normalization can add one pixel at uncapped sizes, so raw.json records actual dimensions.",
         "", "Each worker measures first parse and five repeated parses, keeps a parsed tree, renders a fixed 320px base and the first zoom image, then measures five replacements while retaining the previous image. Tables show median of the five replacements. First zoom render is after the base render, not a cold rasterizer. Raw JSON retains all samples and maxima; five samples do not establish a reliable p95.",
         "", "RSS comes from sysinfo; peak RSS is the kernel getrusage lifetime high-water mark, including renderer/fonts, parse repetitions, base, previous image, replacement and temporary filter buffers. It excludes GPU atlas/upload/presentation and OS file-picker/UI overhead. Exact image bytes are separate from whole-process RSS. RSS after drop can retain allocator pages; preview encoding occurs after the recorded peak and can affect the post-drop snapshot. File reads and renderer initialization are reported separately, outside parse/raster timings.",
         "", "Sources and SHA-256 pins: [corpus.json](../../corpus.json). Artwork is fetched locally rather than redistributed; respect each original work’s license. Measurements are a single machine/run, not renderer rankings or cross-platform guarantees.", "", "## Raster latency", "",
         "| Artwork | Paths | Gradients | Filters | 100% ms | 400% ms | 800% ms | 1600% ms |", "|---|---:|---:|---:|---:|---:|---:|---:|"]
for name, rows in by_id.items():
    counts = rows[0]["elements"]
    values = {r["zoom"]: r.get("measurement", {}).get("raster", {}).get("median_ms") for r in rows}
    cells = [f"{values[z]:.2f}" if values.get(z) is not None else "FAILED" for z in j["metadata"]["zooms"]]
    lines.append(f"| {name} | {counts.get('path',0)} | {counts.get('linearGradient',0)+counts.get('radialGradient',0)} | {counts.get('filter',0)} | " + " | ".join(cells) + " |")
lines += ["", "## Memory and parsing", "", "Peak numbers below are for each document’s largest zoom worker. Image bytes = base + previous zoom + replacement (no filter intermediates).", "", "| Artwork | First parse ms | Repeated parse median ms | Largest image px | One output MiB | Retained/replacement images MiB | Peak process RSS MiB |", "|---|---:|---:|---|---:|---:|---:|"]
for name, rows in by_id.items():
    row = max(rows, key=lambda r:r["zoom"])
    m = row.get("measurement")
    if not m:
        lines.append(f"| {name} | FAILED | | | | | |")
        continue
    peak = m["process_peak_rss_bytes"]
    peak = f"{peak/1048576:.1f}" if peak else "unavailable"
    lines.append(f"| {name} | {m['parse_first_ms']:.2f} | {m['parse']['median_ms']:.2f} | {m['pixel_width']} × {m['pixel_height']} | {m['output_bytes']/1048576:.1f} | {m['replacement_image_bytes']/1048576:.1f} | {peak} |")
lines += ["", "## Compatibility and visual inspection", "", "Successful parsing and nonzero pixels are smoke checks, not SVG conformance. The preview for each document is rendered at 100% (640px edge). Text uses the machine’s system fonts, so substitutions/layout can vary. Scripts and SVG animation are not executed.", "", "| Artwork | Script elements | Animation elements | External href references | Visible pixels at 100% |", "|---|---:|---:|---:|---:|"]
for name, rows in by_id.items():
    row = rows[0]; c = row["compatibility"]; m = row.get("measurement",{})
    lines.append(f"| {name} | {c['scripts']} | {c['animations']} | {len(c['external_refs'])} | {m.get('visible_pixels', 'FAILED')} |")
lines += ["", "USStates is an interactive quiz: the static map and initial instructions render, but clicking Begin cannot run its script. Steps is a script-driven game, and displayWebStats is script/animation-dependent with external JS/data references; their timing measures static initial output only, not the working applications. The other seven files provide static artwork/chart coverage. No corpus file was altered to remove unsupported behavior.", "", "This corpus does not cover embedded raster images, external image resolution, every font/fallback script, or adversarial/decompression-heavy input. Those remain separate coverage gaps.", "", "## Reproduce", "", "```sh", "python3 apps/xgpui-ext-lab/svg-lab/bench/fetch.py /tmp/xgpui-svg-corpus", "cargo build --release --offline --locked --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin svg-lab --target-dir target", "python3 apps/xgpui-ext-lab/svg-lab/bench/run.py --corpus /tmp/xgpui-svg-corpus --output /tmp/svg-bench-rerun", "python3 apps/xgpui-ext-lab/svg-lab/bench/report.py /tmp/svg-bench-rerun", "```", "", "Remove --offline when dependencies are not cached. Each worker has a 120-second timeout; failures/timeouts are retained in raw.json. Downloaded source artwork and PNG previews remain under /tmp; a fresh fetch verifies pinned hashes. Open those SVG files through the lab’s Open SVG control for interactive inspection.", ""]
(p / "REPORT.md").write_text("\n".join(lines))
