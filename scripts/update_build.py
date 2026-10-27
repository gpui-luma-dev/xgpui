#!/usr/bin/env python3
"""Compatibility entry point for the authoritative Rust manifest generator."""
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]


def main():
    return subprocess.call([
        "cargo", "run", "--locked", "--manifest-path",
        str(ROOT / "tools/release-sync/Cargo.toml"), "--", "build-manifests",
        *sys.argv[1:],
    ], cwd=ROOT)


if __name__ == "__main__":
    sys.exit(main())
