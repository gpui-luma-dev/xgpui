"""Stage documentation and optional browser bundles under target/site."""

import argparse
from pathlib import Path
import shutil


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--require-web", action="store_true")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    bundles = root / "target" / "web"
    for name, module in [("extensions", "extensions_web"), ("shaders", "shader_examples")]:
        if args.require_web and not (bundles / name / f"{module}_bg.wasm").is_file():
            parser.error(f"missing {name} bundle; run just web-build first")
    destination = root / "target" / "site"
    if destination.exists():
        shutil.rmtree(destination)
    shutil.copytree(root / "docs", destination, ignore=shutil.ignore_patterns("__pycache__"))
    # Never publish stale demo copies from the previous source-tree layout.
    for name in ("extensions", "shaders"):
        output = destination / "examples" / name
        if output.exists():
            shutil.rmtree(output)
        if (bundles / name).is_dir():
            shutil.copytree(bundles / name, output)
    print(f"Staged website at {destination}")


if __name__ == "__main__":
    main()
