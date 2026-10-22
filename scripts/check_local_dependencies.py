"""Fail if an imported package silently resolves from outside this capture."""

import json
import pathlib
import sys

root = pathlib.Path(__file__).resolve().parents[1]
provenance = json.loads((root / "upstream.json").read_text())
metadata = json.loads(pathlib.Path(sys.argv[1]).read_text())
failures = []
for package in metadata["packages"]:
    expected = provenance["packages"].get(package["name"])
    if expected is None:
        continue
    expected_manifest = (root / expected / "Cargo.toml").resolve()
    if package["source"] is not None or pathlib.Path(package["manifest_path"]).resolve() != expected_manifest:
        failures.append(f"{package['name']} {package['version']} resolved outside xgpui")
if failures:
    raise SystemExit("\n".join(failures))
print("All resolved captured packages use xgpui's local sources.")
