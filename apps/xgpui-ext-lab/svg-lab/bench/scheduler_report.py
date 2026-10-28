#!/usr/bin/env python3
"""Compare whole-process memory, scheduler accounting, cadence and settling."""
import argparse
import json
import math
from pathlib import Path

p=argparse.ArgumentParser()
p.add_argument('directory',type=Path)
a=p.parse_args()
def percentile(xs,q):
    xs=sorted(xs)
    return xs[max(0,math.ceil(len(xs)*q)-1)] if xs else None
rows=[]
for path in sorted(a.directory.glob('*/*.json')):
    d=json.loads(path.read_text())
    if 'frames' not in d:continue
    samples=d['samples']
    frames=[x['interval_ms'] for x in d['frames'] if x['phase'] in ['zoom','scroll','mixed']]
    settling=[x for x in samples if x['phase']=='settle']
    settled=next((s['t']-28 for s in settling if s['ready']==s['mounted'] and s['displaying']==s['mounted']),None)
    sched=[s['scheduler'] for s in samples if s.get('scheduler')]
    if sched:
        assert max(s['peak_managed_bytes'] for s in sched)<=d['scheduler']['max_bytes']
        assert max(s['peak_active_jobs'] for s in sched)<=d['scheduler']['max_jobs']
        assert sched[-1]['managed_bytes']==0 and sched[-1]['active_jobs']==0 and sched[-1]['queued_jobs']==0 and sched[-1]['members']==0
    rows.append({'case':str(path.relative_to(a.directory)).removesuffix('.json'),'tiles':d['tiles'],'pool_mib':d['scheduler']['max_bytes']/2**20 if d.get('scheduler') else None,'p50_ms':percentile(frames,.5),'p95_ms':percentile(frames,.95),'p99_ms':percentile(frames,.99),'max_ms':max(frames),'over_33ms_pct':sum(x>33.34 for x in frames)/len(frames)*100,'peak_rss_mib':max(s['memory']['rss_bytes'] for s in samples)/2**20,'peak_footprint_mib':max(s['memory']['peak_physical_footprint_bytes'] for s in samples)/2**20,'end_footprint_mib':samples[-1]['memory']['physical_footprint_bytes']/2**20,'peak_managed_mib':max((s['peak_managed_bytes'] for s in sched),default=0)/2**20,'peak_jobs':max((s['peak_active_jobs'] for s in sched),default=None),'peak_queue':max((s['queued_jobs'] for s in sched),default=None),'peak_rgba_mib':max(s['displayed_rgba_bytes'] for s in samples)/2**20,'max_failed':max(s['failed'] for s in samples),'settled_after_s':settled,'last_settle_ready':settling[-1]['ready'],'last_settle_mounted':settling[-1]['mounted'],'last_settle_displaying':settling[-1]['displaying']})
(a.directory/'summary.json').write_text(json.dumps(rows,indent=2)+'\n')
lines=['# Shared scheduler: native gallery measurements','','24 tiles unless marked 96, 16 MiB local output policy, same 36-second workload as the earlier gallery. Shared pools allow two concurrent jobs. Retained images charge 2× RGBA, in-flight work 3× RGBA. Limits apply to these reservations, not total process memory. Peak footprint is kernel recorded; RSS is sampled approximately every 100ms. Active cadence combines zoom, scroll and mixed phases. All-settled timing uses reported statuses, not GPU presentation.','','| Case | p95 / p99 (ms) | Max (ms) | >33ms (%) | Peak RSS / footprint (MiB) | Peak managed (MiB) | Peak jobs / sampled queue | Settled ready | First all-ready after settle (s) |','|---|---:|---:|---:|---:|---:|---:|---:|---:|']
for r in rows:
    settle=f"{r['settled_after_s']:.2f}" if r['settled_after_s'] is not None else 'not within 4s'
    lines.append(f"| {r['case']} | {r['p95_ms']:.1f} / {r['p99_ms']:.1f} | {r['max_ms']:.1f} | {r['over_33ms_pct']:.2f} | {r['peak_rss_mib']:.0f} / {r['peak_footprint_mib']:.0f} | {r['peak_managed_mib']:.1f} | {r['peak_jobs']} / {r['peak_queue']} | {r['last_settle_ready']}/{r['last_settle_mounted']} | {settle} |")
(a.directory/'REPORT.md').write_text('\n'.join(lines)+'\n')
print('\n'.join(lines))
