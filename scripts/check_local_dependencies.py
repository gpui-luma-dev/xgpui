"""Verify local framework and published helper package identities in Cargo metadata."""

import json
import pathlib
import sys

root = pathlib.Path(__file__).resolve().parents[1]
provenance = json.loads((root / "upstream.json").read_text())
metadata = json.loads(pathlib.Path(sys.argv[1]).read_text())
registry = provenance.get("registry_packages", {})
helpers = registry.get("packages", {})
published = set(helpers.values())
distribution = provenance.get("distribution", {})
identities = distribution.get("packages", {})
local_paths = {identities.get(name, name): path for name, path in provenance["packages"].items()}
local_paths.update({name: f"extensions/xgpui-ext/crates/{name}" for name in identities if name.startswith("xgpui-")})
failures = []
for package in metadata["packages"]:
    name = package["name"]
    expected = local_paths.get(name)
    if expected is not None:
        expected_manifest = (root / expected / "Cargo.toml").resolve()
        if package["source"] is not None or pathlib.Path(package["manifest_path"]).resolve() != expected_manifest:
            failures.append(f"{name} {package['version']} resolved outside xgpui")
        expected_version = distribution.get("versions", {}).get(name, distribution.get("version"))
        if expected_version and package["version"] != expected_version:
            failures.append(f"{name} must use downstream version {expected_version}")
    if name in provenance["packages"] and name in identities and identities[name] != name:
        failures.append(f"{name} must use downstream package {identities[name]}")
    if name in helpers:
        failures.append(f"{name} must use published package {helpers[name]}")
    if name in published:
        if package["source"] != "registry+https://github.com/rust-lang/crates.io-index" or package["version"] != registry["version"]:
            failures.append(f"{name} must resolve from crates.io at {registry['version']}")
    if name == "gpui-unofficial" or (name.endswith("-gpui-unofficial") and name not in published):
        failures.append(f"Unexpected upstream framework package {name}; use the local xgpui fork")
if failures:
    raise SystemExit("\n".join(failures))
print("Resolved framework packages are local; supporting packages use the pinned crates.io release.")
