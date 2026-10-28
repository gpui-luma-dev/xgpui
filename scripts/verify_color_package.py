"""Package and validate xgpui-color in fresh scratch space; never publish.

Only the registry download cache is shared. Cargo configuration, target output,
lockfiles, source staging and the consumer are isolated from the checkout.
"""

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import tomllib


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def inspect_archive(archive, source, destination, package_id):
    with tarfile.open(archive) as bundle:
        files = {entry.name: entry for entry in bundle.getmembers()}
        for entry in files.values():
            parts = Path(entry.name).parts
            if not parts or parts[0] != package_id or ".." in parts or not (entry.isfile() or entry.isdir()):
                raise ValueError(f"Unexpected archive entry: {entry.name}")
        required = ["README.md", "LICENSE-APACHE", "src/math.wgsl", "Cargo.toml.orig"]
        required += [str(path.relative_to(source)) for path in source.rglob("*.rs")]
        for name in required:
            if f"{package_id}/{name}" not in files:
                raise ValueError(f"Missing packaged file: {name}")
        bundle.extractall(destination, filter="data")
    extracted = destination / package_id
    for name in required:
        original = source / ("Cargo.toml" if name == "Cargo.toml.orig" else name)
        if sha256(original) != sha256(extracted / name):
            raise ValueError(f"Packaged content differs from staged source: {name}")
    manifest = tomllib.loads((extracted / "Cargo.toml").read_text())
    dependency_tables = [manifest, *manifest.get("target", {}).values()]
    if any(table.get("dependencies") or table.get("build-dependencies") for table in dependency_tables) or manifest.get("patch") or manifest.get("replace"):
        raise ValueError("Unexpected runtime dependencies or patches in color package")
    if manifest["package"].get("publish") is not False:
        raise ValueError("Publication must remain disabled during preparation")
    return extracted, sorted(files)


def verify(root, output, offline, native_metal):
    output.mkdir(parents=True, exist_ok=False)
    report = {
        "started_at": datetime.now(timezone.utc).isoformat(),
        "status": "running", "offline": offline, "native_metal_requested": native_metal,
        "scope": "local archive plus path consumer; not a crates.io upload or registry-install test",
        "steps": [],
    }
    report_path = output / "report.json"

    def save():
        report_path.write_text(json.dumps(report, indent=2) + "\n")

    cache = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo")) / "registry"
    cargo_home = output / "cargo-home"
    cargo_home.mkdir()
    if cache.is_dir():
        (cargo_home / "registry").symlink_to(cache.resolve(), target_is_directory=True)
    env = {key: value for key, value in os.environ.items() if not key.startswith("CARGO_")}
    for key in ("RUSTFLAGS", "RUSTDOCFLAGS", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "RUSTC", "RUSTDOC"):
        env.pop(key, None)
    env.update(CARGO_HOME=str(cargo_home), CARGO_TARGET_DIR=str(output / "target"))

    def run(label, arguments, locked=False):
        command = ["cargo", *arguments]
        if offline:
            command.insert(2, "--offline")
        if locked:
            command.insert(2, "--locked")
        print(f"{label}: {' '.join(command)}", flush=True)
        result = subprocess.run(command, cwd=output, env=env, text=True, capture_output=True)
        log = output / f"{label}.log"
        log.write_text(result.stdout + result.stderr)
        report["steps"].append({"name": label, "command": command, "exit_code": result.returncode, "log": str(log)})
        save()
        if result.returncode:
            raise RuntimeError(f"{label} failed; see {log}")
        return result.stdout

    try:
        report["rustc"] = subprocess.check_output(["rustc", "-vV"], text=True).strip()
        report["cargo"] = subprocess.check_output(["cargo", "--version"], text=True).strip()
        staged = output / "source"
        shutil.copytree(root / "extensions/crates/xgpui-color", staged,
                        ignore=shutil.ignore_patterns("target", "Cargo.lock", ".git", ".cargo"))
        manifest = tomllib.loads((staged / "Cargo.toml").read_text())
        package = manifest["package"]
        if package["name"] != "xgpui-color":
            raise ValueError("Expected xgpui-color")
        if sha256(staged / "LICENSE-APACHE") != sha256(root / "gpui/crates/gpui/LICENSE-APACHE"):
            raise ValueError("Color license notice must match GPUI's license notice")
        package_id = f"{package['name']}-{package['version']}"
        run("package", ["package", "--manifest-path", str(staged / "Cargo.toml")])
        archive = output / "target/package" / f"{package_id}.crate"
        extracted, entries = inspect_archive(archive, staged, output / "unpacked", package_id)
        report.update(archive=str(archive), archive_sha256=sha256(archive), archive_files=entries)
        test_args = ["test", "--manifest-path", str(extracted / "Cargo.toml")]
        if not native_metal:
            test_args += ["--", "--skip", "metal_float32_matches_cpu_reference"]
        run("archive-tests", test_args, locked=True)

        consumer = output / "consumer"
        shutil.copytree(root / "scripts/fixtures/color-consumer", consumer,
                        ignore=shutil.ignore_patterns("target", "Cargo.lock"))
        consumer_manifest = consumer / "Cargo.toml"
        text = consumer_manifest.read_text()
        expected = f'xgpui-color = "={package["version"]}"'
        if text.count(expected) != 1:
            raise ValueError("Consumer fixture version must match the candidate")
        replacement = 'xgpui-color = { version = "=' + package["version"] + '", path = ' + json.dumps(str(extracted)) + ' }'
        consumer_manifest.write_text(text.replace(expected, replacement))
        run("consumer-lock", ["generate-lockfile", "--manifest-path", str(consumer_manifest)])
        metadata = json.loads(run("consumer-metadata", ["metadata", "--format-version", "1", "--manifest-path", str(consumer_manifest)], locked=True))
        if {item["name"] for item in metadata["packages"]} != {"xgpui-color", "xgpui-color-consumer-check"}:
            raise ValueError("Standalone consumer unexpectedly resolves extra runtime packages")
        color, = [item for item in metadata["packages"] if item["name"] == "xgpui-color"]
        if Path(color["manifest_path"]).resolve() != (extracted / "Cargo.toml").resolve():
            raise ValueError("Consumer did not resolve the extracted archive")
        run("consumer-run", ["run", "--manifest-path", str(consumer_manifest)], locked=True)
        report["consumer_runtime_packages"] = sorted(item["name"] for item in metadata["packages"])
        report["status"] = "passed"
        report["native_metal_executed"] = native_metal and "apple-darwin" in report["rustc"]
    except Exception as error:
        report["status"] = "failed"
        report["error"] = str(error)
        raise
    finally:
        report["finished_at"] = datetime.now(timezone.utc).isoformat()
        save()
        print(f"Report: {report_path}", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, help="New scratch directory; must not exist or be inside the repository")
    parser.add_argument("--offline", action="store_true", help="Use cached registry dependencies only")
    parser.add_argument("--native-metal", action="store_true", help="Run native Metal compute tests; requires a macOS GPU")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    output = args.out.resolve() if args.out else Path(tempfile.mkdtemp(prefix="xgpui-color-package-parent-")) / "verification"
    if output == root or root in output.parents:
        parser.error("--out must be outside the repository")
    verify(root, output, args.offline, args.native_metal)


if __name__ == "__main__":
    main()
