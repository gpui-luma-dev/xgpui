"""Read-only manifest inventory for issue #49; never packages or publishes crates.

Python 3.11+. Output is a conservative all-target manifest graph, not Cargo's
resolved graph or a release authorization. Optional dependencies are retained.
"""

import argparse
import json
from pathlib import Path
import tomllib


KINDS = ("dependencies", "build-dependencies", "dev-dependencies")


def dependencies(document):
    for target, table in [(None, document), *document.get("target", {}).items()]:
        for kind in KINDS:
            for alias, value in table.get(kind, {}).items():
                spec = {"version": value} if isinstance(value, str) else value
                yield {
                    "alias": alias,
                    "package": spec.get("package", alias),
                    "kind": kind,
                    "target": target,
                    "optional": spec.get("optional", False),
                    **{key: spec[key] for key in ("version", "path", "git", "rev", "registry", "workspace") if key in spec},
                }


def dependency_layers(graph):
    """Return dependency-first layers and nodes blocked by cycles (not SCCs)."""
    pending = {name: set(deps) for name, deps in graph.items()}
    layers = []
    while pending:
        ready = sorted(name for name, deps in pending.items() if not deps)
        if not ready:
            break
        layers.append(ready)
        for name in ready:
            del pending[name]
        for deps in pending.values():
            deps.difference_update(ready)
    return layers, sorted(pending)


def audit(root):
    release = json.loads((root / "scripts/release-packages.json").read_text())
    upstream = json.loads((root / "upstream.json").read_text())
    names = set(release["packages"].values())
    manifests = {}
    for local, name in release["packages"].items():
        directory = upstream["packages"].get(local, f"extensions/crates/{local}")
        manifests[name] = root / directory / "Cargo.toml"
    packages, issues = [], []
    graph = {name: set() for name in names}
    development_graph = {name: set() for name in names}
    for name, path in sorted(manifests.items()):
        document = tomllib.loads(path.read_text())
        package = document["package"]
        expected_version = release.get("versions", {}).get(name, release["version"])
        problems = []
        if package["name"] != name or package["version"] != expected_version:
            problems.append("release-map-identity-mismatch")
        if package.get("publish") is False or package.get("publish") == []:
            problems.append("publishing-disabled")
        elif isinstance(package.get("publish"), list) and "crates-io" not in package["publish"]:
            problems.append("crates-io-not-allowed")
        for key in ("description", "repository"):
            if not package.get(key):
                problems.append(f"missing-{key}")
        if not (package.get("license") or package.get("license-file")):
            problems.append("missing-license")
        deps = list(dependencies(document))
        for dep in deps:
            context = f"{dep['kind']}:{dep['target'] or 'all'}:{dep['alias']}"
            # Cargo omits path-only development dependencies from the archive.
            local_dev = (dep['kind'] == 'dev-dependencies' and 'path' in dep
                         and not any(dep.get(key) for key in ('version', 'git', 'registry')))
            if dep.get("workspace"):
                problems.append(f"unresolved-workspace-dependency:{context}")
            if ("path" in dep or "git" in dep) and not dep.get("version") and not local_dev:
                problems.append(f"dependency-without-registry-version:{context}")
            if "git" in dep:
                problems.append(f"git-source-needs-registry-equivalence-review:{context}")
            if dep["package"] in names:
                development_graph[name].add(dep["package"])
                if dep["kind"] != "dev-dependencies":
                    graph[name].add(dep["package"])
                # Registry-bound internal edges use exact version pins.
                wanted = release.get("versions", {}).get(dep["package"], release["version"])
                if not local_dev and dep.get("version") != "=" + wanted:
                    problems.append(f"internal-version-mismatch:{context}")
                if "path" in dep and (path.parent / dep["path"] / "Cargo.toml").resolve() != manifests[dep["package"]].resolve():
                    problems.append(f"internal-path-mismatch:{context}")
        packages.append({
            "name": name, "version": package["version"],
            "manifest": str(path.relative_to(root)),
            "publish": package.get("publish", True),
            "metadata_review": [key for key in ("homepage", "readme") if not package.get(key)],
            "dependencies": deps, "issues": problems,
        })
        issues.extend({"package": name, "issue": problem} for problem in problems)
    patches = []
    for relative in ("Cargo.toml", ".cargo/config.toml", ".cargo/validation.toml"):
        document = tomllib.loads((root / relative).read_text())
        for registry, entries in document.get("patch", {}).items():
            for name, spec in entries.items():
                patches.append({"file": relative, "registry": registry, "package": name, "spec": spec})
    layers, blocked = dependency_layers(graph)
    _, development_blocked = dependency_layers(development_graph)
    return {
        "schema_version": 1,
        "scope": "manifest-only, all targets and optional edges; no registry resolution or build verification",
        "release_ready": False,
        "packages": packages,
        "dependency_first_layers": layers,
        "cycle_blocked_packages": blocked,
        "including_dev_cycle_blocked_packages": development_blocked,
        "patches_to_review_outside_repository": patches,
        "issues": issues,
        "remaining_gates": [
            "registry name/version availability and authorized ownership",
            "prerequisites #18, #47 and #48 with native platform evidence",
            "package contents, assets and license attribution",
            "isolated package and consumer builds without repository patches",
            "Windows, Linux and macOS validation",
            "explicit first-publication authorization",
        ],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    args = parser.parse_args()
    print(json.dumps(audit(args.root.resolve()), indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
