#!/usr/bin/env python3
"""Summarize measured gallery phases, retaining all raw samples."""
import argparse
import json
import math
from pathlib import Path

p = argparse.ArgumentParser()
p.add_argument('directory', type=Path)
a = p.parse_args()

def quantile(xs, q):
    xs = sorted(xs)
    return xs[max(0, math.ceil(len(xs)*q)-1)] if xs else 0

rows = []
for path in sorted(a.directory.glob('*.json')):
    d = json.loads(path.read_text())
    if 'frames' not in d:
        continue
    for phase in ['baseline', 'zoom', 'scroll', 'mixed', 'settle', 'unmounted']:
        frames = [x['interval_ms'] for x in d['frames'] if x['phase'] == phase and x['t'] > 0.25]
        samples = [x for x in d['samples'] if x['phase'] == phase]
        if not frames or not samples:
            continue
        memory = lambda key: max((x['memory'].get(key, 0) for x in samples), default=0)/2**20
        rows.append({'case': path.stem, 'phase': phase, 'frames': len(frames), 'median_ms': quantile(frames, .5), 'p95_ms': quantile(frames, .95), 'p99_ms': quantile(frames, .99), 'max_ms': max(frames), 'over_33ms_pct': sum(x > 33.34 for x in frames)/len(frames)*100, 'rss_mib': memory('rss_bytes'), 'footprint_mib': memory('physical_footprint_bytes'), 'peak_footprint_mib': memory('peak_physical_footprint_bytes'), 'displayed_rgba_mib': max(x['displayed_rgba_bytes'] for x in samples)/2**20, 'max_mounted': max(x['mounted'] for x in samples), 'max_failed': max(x['failed'] for x in samples), 'end_footprint_mib': samples[-1]['memory'].get('physical_footprint_bytes', 0)/2**20})
(a.directory / 'summary.json').write_text(json.dumps(rows, indent=2)+'\n')
lines = ['# Native SVG gallery stress measurements', '', '24 tiles, eight shared parsed complex SVGs, 1140 × 720 logical window. Each case runs in its own process for 36 seconds: baseline 4s, zoom 8s, scroll 8s, mixed 8s, settle 4s, unmount 4s. Visible mode explicitly removes SVG elements outside the viewport plus one overscan row. Eager mode instantiates all 24.', '', 'Frame intervals are host Render invocation intervals under continuous frame demand, including scheduling, rendering and competition with workers. They are not GPU presentation or input-to-photon measurements. Memory is whole-process RSS and macOS physical footprint from libproc at approximately 100ms intervals; lifetime peak footprint is kernel recorded. Process-accounted graphics allocations are included, but WindowServer/system-wide GPU memory is not. Displayed RGBA is only the nominal current output size reported by mounted elements, not total memory. Sampling and JSON recording add overhead. Initial parsing is excluded from frame timings. No other benchmark runs concurrently.', '', '| Case | Phase | p50 / p95 / p99 ms | Max ms | >33ms % | RSS MiB | Footprint MiB | RGBA MiB | Mounted |', '|---|---|---:|---:|---:|---:|---:|---:|---:|']
for r in rows:
    lines.append(f"| {r['case']} | {r['phase']} | {r['median_ms']:.1f} / {r['p95_ms']:.1f} / {r['p99_ms']:.1f} | {r['max_ms']:.1f} | {r['over_33ms_pct']:.1f} | {r['rss_mib']:.0f} | {r['footprint_mib']:.0f} | {r['displayed_rgba_mib']:.1f} | {r['max_mounted']} |")
(a.directory / 'REPORT.md').write_text('\n'.join(lines)+'\n')
print('\n'.join(lines))
