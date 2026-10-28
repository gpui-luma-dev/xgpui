#!/usr/bin/env python3
"""Run serial, isolated release workers; retain raw measurements and failures."""
import argparse
import collections
import datetime
import hashlib
import json
from pathlib import Path
import platform
import subprocess
import time
import xml.etree.ElementTree as ET

parser = argparse.ArgumentParser()
parser.add_argument("--binary", type=Path, default=Path("target/release/svg-lab"))
parser.add_argument("--corpus", type=Path, required=True)
parser.add_argument("--output", type=Path, required=True)
parser.add_argument("--previews", type=Path, default=Path("/tmp/xgpui-svg-previews"))
parser.add_argument("--zooms", type=float, nargs="+", default=[1, 4, 8, 16])
parser.add_argument("--density", type=float, default=2)
parser.add_argument("--iterations", type=int, default=5)
parser.add_argument("--timeout", type=float, default=120)
parser.add_argument("--host-label", default="CPU/RAM not recorded")
args = parser.parse_args()
args.output.mkdir(parents=True, exist_ok=True)
args.previews.mkdir(parents=True, exist_ok=True)
manifest = json.loads(Path(__file__).with_name("corpus.json").read_text())
rows = []
metadata = {"started_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
            "platform": platform.platform(), "machine": platform.machine(), "host_label": args.host_label,
            "binary_sha256": hashlib.sha256(args.binary.read_bytes()).hexdigest(),
            "iterations": args.iterations, "zooms": args.zooms, "density": args.density,
            "method": "One fresh release subprocess per file/zoom, serial. Five repeated parse and raster samples after first use; retain base and previous zoom during replacement. CPU only; no GPU upload or UI frame timing."}
for item in manifest:
    path = args.corpus / (item["id"] + ".svg")
    data = path.read_bytes()
    if hashlib.sha256(data).hexdigest() != item["sha256"]:
        raise SystemExit(f"Hash mismatch: {path}")
    root = ET.fromstring(data)
    counts = collections.Counter(x.tag.split("}")[-1] for x in root.iter())
    flags = {"scripts": counts["script"], "animations": sum(v for k, v in counts.items() if k.startswith("animate") or k == "set"),
             "external_refs": [v for el in root.iter() for k,v in el.attrib.items() if k.endswith("href") and not v.startswith(("#", "data:"))]}
    for zoom in args.zooms:
        command = [str(args.binary.resolve()), "--benchmark", str(path.resolve()), "--zoom", str(zoom), "--density", str(args.density), "--iterations", str(args.iterations), "--budget-mib", "256"]
        if zoom == args.zooms[0]:
            command += ["--preview", str((args.previews / (item["id"]+".png")).resolve())]
        started = time.monotonic()
        row = {"id": item["id"], "zoom": zoom, "sha256": item["sha256"], "source": item["url"], "elements": dict(counts), "compatibility": flags}
        try:
            run = subprocess.run(command, capture_output=True, text=True, timeout=args.timeout)
            row.update({"exit_code": run.returncode, "stderr": run.stderr})
            if run.returncode == 0:
                row["measurement"] = json.loads(run.stdout)
                m = row["measurement"]
                peak = f"{m['process_peak_rss_bytes']/1048576:.1f}MiB" if m['process_peak_rss_bytes'] else 'unavailable'
                print(f"{item['id']:24} {zoom:4g}x {m['raster']['median_ms']:9.2f}ms peak={peak}", flush=True)
            else:
                row["error"] = run.stdout + run.stderr
                print(f"FAILED {item['id']} {zoom}x: {row['error'][:200]}", flush=True)
        except subprocess.TimeoutExpired:
            row["error"] = f"Worker exceeded {args.timeout}s (killed)"
            print(f"TIMEOUT {item['id']} {zoom}x", flush=True)
        row["wall_seconds"] = time.monotonic()-started
        rows.append(row)
        (args.output / "raw.json").write_text(json.dumps({"metadata": metadata, "cases": rows}, indent=2)+"\n")
print(f"Saved {len(rows)} cases to {args.output / 'raw.json'}", flush=True)
