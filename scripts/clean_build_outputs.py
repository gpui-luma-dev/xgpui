"""Preview or remove ignored Cargo target directories throughout this checkout."""

import argparse
import os
from pathlib import Path
import shutil
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--apply", action="store_true", help="delete the listed outputs")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    tracked = subprocess.check_output(
        ["git", "ls-files", "-z"], cwd=root
    ).decode().split("\0")
    targets = []
    for directory, names, _ in os.walk(root, followlinks=False):
        names[:] = sorted(name for name in names if name != ".git")
        if "target" not in names:
            continue
        names.remove("target")
        path = Path(directory) / "target"
        if path.is_symlink():
            continue
        relative = path.relative_to(root).as_posix()
        if any(p == relative or p.startswith(relative + "/") for p in tracked):
            raise SystemExit(f"Refusing to clean tracked files in {relative}")
        ignored = subprocess.run(
            ["git", "check-ignore", "-q", "--", relative + "/"], cwd=root
        )
        if ignored.returncode != 0:
            raise SystemExit(f"Refusing to clean non-ignored directory {relative}")
        targets.append(path)
    for path in targets:
        print(f"{'Removing' if args.apply else 'Would remove'} {path.relative_to(root)}", flush=True)
        if args.apply:
            shutil.rmtree(path)
    print(f"{len(targets)} target directories."
          + ("" if args.apply else " Run with --apply to remove them."))


if __name__ == "__main__":
    main()
