#!/usr/bin/env python3
"""Prepare a tree-diff proposal, without resolving downstream/upstream overlaps."""
import argparse
import hashlib
import json
import os
import pathlib
import re
import shutil
import subprocess
import tempfile

from import_release import SOURCE, capture, git

SCOPES = ("gpui", ".cargo/config.toml", "upstream.json")


def signature(path):
    if path.is_symlink():
        return "symlink:" + os.readlink(path)
    if path.is_file():
        return ("executable:" if path.stat().st_mode & 0o111 else "file:") + hashlib.sha256(path.read_bytes()).hexdigest()
    return None


def imported_files(root):
    result = set()
    for scope in SCOPES:
        path = root / scope
        if path.is_dir() and not path.is_symlink():
            result.update(p.relative_to(root).as_posix() for p in path.rglob("*") if p.is_file() or p.is_symlink())
        elif path.is_file() or path.is_symlink():
            result.add(scope)
    return result


def classify(path, packages):
    # Cargo requirements affect dependency resolution, including mirrored manifests.
    if pathlib.PurePosixPath(path).name == "Cargo.toml" or path == ".cargo/config.toml":
        return "cargo"
    # Other nested transformed-crates files are duplicate generation artifacts.
    for package in packages:
        if path.startswith(package + "/"):
            relative = path[len(package) + 1:]
            if relative not in ("Cargo.toml", "README.md"):
                return "code"
    return "metadata"


def compare(root, before, incoming, release):
    previous = json.loads((before / "upstream.json").read_text())
    source = json.loads((incoming / "upstream.json").read_text())
    packages = set(previous["packages"].values()) | set(source["packages"].values())
    tracked = set(git("ls-files", "--", *SCOPES, cwd=root).splitlines())
    entries = []
    for name in sorted(tracked | imported_files(incoming)):
        local, old, new = (signature(directory / name) for directory in (root, before, incoming))
        if local == new:
            continue
        kind = classify(name, packages)
        entries.append({
            "path": name, "kind": kind,
            "retain_local": kind == "code" and old == new and old != local,
            "change": "added" if local is None else "deleted" if new is None else "modified",
            "upstream_changed": old != new,
            "downstream_overlap": old != local,
            "local": local, "incoming": new,
        })
    return {"release": release, "base_commit": git("rev-parse", "HEAD", cwd=root),
            "source_commit": source["commit"], "previous_source_commit": previous["commit"],
            "entries": entries}


def apply(root, incoming, report, kind):
    entries = [entry for entry in report["entries"]
               if entry["kind"] == kind and not entry.get("retain_local", False)]
    # Refuse stale plans before writing anything. This is not a merge resolver.
    for entry in entries:
        name = entry["path"]
        if name.startswith("/") or ".." in pathlib.PurePosixPath(name).parts:
            raise ValueError(f"Unsafe path: {name}")
        if not (name.startswith("gpui/") or name in SCOPES):
            raise ValueError(f"Outside import scope: {name}")
        destination = root / name
        for parent in destination.parents:
            if parent == root:
                break
            if parent.is_symlink():
                raise ValueError(f"Symlink parent: {name}")
        if signature(destination) != entry["local"] or signature(incoming / name) != entry["incoming"]:
            raise ValueError(f"Tree changed since review plan: {name}")
    for entry in entries:
        destination, source = root / entry["path"], incoming / entry["path"]
        if destination.is_file() or destination.is_symlink():
            destination.unlink()
        if entry["incoming"] is not None:
            destination.parent.mkdir(parents=True, exist_ok=True)
            if source.is_symlink():
                destination.symlink_to(os.readlink(source))
            else:
                shutil.copy2(source, destination)


def prepare(root, incoming, report_path, source, revision, release, normalizer, name_map, expected_sha=None):
    if incoming.exists():
        raise ValueError("Snapshot destination must not exist")
    previous = json.loads((root / "upstream.json").read_text())
    with tempfile.TemporaryDirectory() as temporary:
        before = pathlib.Path(temporary)
        incoming.mkdir(parents=True)
        capture(before, source, previous["commit"], previous["ref"].removeprefix("release/"), previous["commit"])
        capture(incoming, source, revision, release, expected_sha)
        for tree in (before, incoming):
            command = [str(normalizer), "normalize", "--root", str(tree), "--map", str(name_map)]
            subprocess.run(command, check=True)
            subprocess.run([*command, "--check"], check=True)
        report = compare(root, before, incoming, release)
    report_path.write_text(json.dumps(report, indent=2) + "\n")
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("prepare", "code", "cargo", "metadata"))
    parser.add_argument("--snapshot", type=pathlib.Path, required=True)
    parser.add_argument("--report", type=pathlib.Path, required=True)
    parser.add_argument("--release")
    parser.add_argument("--expected-sha")
    parser.add_argument("--normalizer", type=pathlib.Path)
    parser.add_argument("--map", type=pathlib.Path)
    args = parser.parse_args()
    root = pathlib.Path(git("rev-parse", "--show-toplevel"))
    if git("status", "--porcelain", cwd=root):
        parser.error("Review preparation/application requires a clean checkout")
    if args.action != "prepare":
        branch = git("branch", "--show-current", cwd=root)
        if not branch.startswith(("sync/", "codex/")):
            parser.error("Apply proposals only on a dedicated sync or codex review branch")
        apply(root, args.snapshot.resolve(), json.loads(args.report.read_text()), args.action)
        return
    if not args.release or not re.fullmatch(r"\d+\.\d+\.\d+", args.release):
        parser.error("release must look like 1.25.3")
    if args.expected_sha and not re.fullmatch(r"[0-9a-fA-F]{40}", args.expected_sha):
        parser.error("expected-sha must be a full commit SHA")
    if not args.normalizer or not args.map:
        parser.error("prepare requires --normalizer and --map")
    with tempfile.TemporaryDirectory() as temporary:
        source = pathlib.Path(temporary)
        git("init", "--quiet", cwd=source)
        previous = json.loads((root / "upstream.json").read_text())
        git("fetch", "--depth=1", SOURCE, previous["commit"], cwd=source)
        git("fetch", "--depth=1", SOURCE, f"refs/heads/release/{args.release}", cwd=source)
        report = prepare(root, args.snapshot.resolve(), args.report.resolve(), source, "FETCH_HEAD",
                         args.release, args.normalizer.resolve(), args.map.resolve(), args.expected_sha)
        print(json.dumps({kind: sum(entry["kind"] == kind for entry in report["entries"])
                          for kind in ("code", "cargo", "metadata")}))


if __name__ == "__main__":
    main()
