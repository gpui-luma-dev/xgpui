#!/usr/bin/env python3
"""Fetch public artwork to a local directory, verifying pinned SHA-256 when present."""
import argparse
import hashlib
import json
from pathlib import Path
import urllib.request

parser = argparse.ArgumentParser()
parser.add_argument("destination", type=Path)
parser.add_argument("--bundled-only", action="store_true", help="Copy only committed, license-audited artwork without network access")
args = parser.parse_args()
manifest = json.loads(Path(__file__).with_name("corpus.json").read_text())
args.destination.mkdir(parents=True, exist_ok=True)
for item in manifest:
    if args.bundled_only and not item.get("bundled"):
        continue
    target = args.destination / (item["id"] + ".svg")
    if target.exists() and item.get("sha256") and hashlib.sha256(target.read_bytes()).hexdigest() == item["sha256"]:
        print(f"{item['id']} verified", flush=True)
        continue
    if item.get("local"):
        data = (Path(__file__).resolve().parents[3] / item["local"]).read_bytes()
    else:
        with urllib.request.urlopen(item["url"], timeout=30) as response:
            data = response.read(32 * 1024 * 1024 + 1)
    assert len(data) <= 32 * 1024 * 1024, "input exceeds lab limit"
    digest = hashlib.sha256(data).hexdigest()
    if item.get("sha256") and digest != item["sha256"]:
        raise SystemExit(f"Hash mismatch: {item['id']}: {digest}")
    target.write_bytes(data)
    print(f"{item['id']} {len(data)} {digest}", flush=True)
