"""Inspect release archives and compile isolated consumers; never publish.

Archives are generated without lockfiles to avoid resolving unpublished candidate
versions. Only extracted validation copies get internal dependency paths. No
third-party patches, checkout configuration or checkout targets are reused.
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


def scalar(value):
    if isinstance(value, bool):
        return str(value).lower()
    if isinstance(value, (str, int, float)):
        return json.dumps(value, ensure_ascii=False)
    if isinstance(value, list):
        return "[" + ", ".join(scalar(item) for item in value) + "]"
    raise ValueError(f"Unsupported TOML value: {type(value)}")


def dump_toml(document):
    """Serialize scratch manifests, retaining nested tables and array tables."""
    lines = []

    def table(values, path=(), array=False):
        if path:
            key = ".".join(json.dumps(item) for item in path)
            lines.append(("[[" if array else "[") + key + ("]]" if array else "]"))
        for key, value in values.items():
            if not isinstance(value, dict) and not (isinstance(value, list) and value and isinstance(value[0], dict)):
                lines.append(json.dumps(key) + " = " + scalar(value))
        lines.append("")
        for key, value in values.items():
            if isinstance(value, dict):
                table(value, (*path, key))
            elif isinstance(value, list) and value and isinstance(value[0], dict):
                for item in value:
                    table(item, (*path, key), True)

    table(document)
    text = "\n".join(lines)
    if tomllib.loads(text) != document:
        raise ValueError("Scratch TOML serialization changed manifest values")
    return text


def rewire_candidates(document, candidates):
    rewrites = []
    for table in [document, *document.get("target", {}).values()]:
        for kind in ("dependencies", "build-dependencies", "dev-dependencies"):
            for alias, value in table.get(kind, {}).items():
                spec = {"version": value} if isinstance(value, str) else value
                name = spec.get("package", alias)
                if name in candidates:
                    if not spec.get("version"):
                        raise ValueError(f"Candidate {name} has no registry version")
                    spec["path"] = str(candidates[name])
                    table[kind][alias] = spec
                    rewrites.append({"alias": alias, "package": name, "kind": kind})
    return rewrites


def inspect_archive(archive, staged, destination, package_id):
    with tarfile.open(archive) as bundle:
        entries = bundle.getmembers()
        for entry in entries:
            parts = Path(entry.name).parts
            if not parts or parts[0] != package_id or ".." in parts or not (entry.isfile() or entry.isdir()):
                raise ValueError(f"Unexpected archive entry: {entry.name}")
        bundle.extractall(destination, filter="data")
    extracted = destination / package_id
    required = {"README.md", "LICENSE-APACHE", "Cargo.toml.orig"}
    # Cover code, shader build inputs and embedded resources, including Apple’s
    # bundled GPUI shader-binding sources and licensed test fonts.
    suffixes = {".rs", ".wgsl", ".metal", ".hlsl", ".h", ".ttf", ".otf",
                ".c", ".cc", ".cpp", ".m", ".mm", ".rc", ".xml",
                ".png", ".jpg", ".jpeg", ".svg", ".bin", ".spv"}
    for path in staged.rglob("*"):
        # Cargo intentionally excludes nested packages, such as hello_web.
        ancestors = list(path.relative_to(staged).parents)[:-1]
        if any((staged / ancestor / "Cargo.toml").is_file() for ancestor in ancestors):
            continue
        if path.is_file() and (path.suffix in suffixes or path.name.startswith(("LICENSE", "NOTICE"))):
            required.add(str(path.relative_to(staged)))
    for relative in sorted(required):
        original = staged / ("Cargo.toml" if relative == "Cargo.toml.orig" else relative)
        packaged = extracted / relative
        if not packaged.is_file():
            raise ValueError(f"Archive omits required input: {relative}")
        if original.read_bytes() != packaged.read_bytes():
            raise ValueError(f"Archive content mismatch: {relative}")
    document = tomllib.loads((extracted / "Cargo.toml").read_text())
    if document.get("patch") or document.get("replace"):
        raise ValueError("Unexpected archived dependency overrides")
    for table in [document, *document.get("target", {}).values()]:
        for kind in ("dependencies", "build-dependencies", "dev-dependencies"):
            for value in table.get(kind, {}).values():
                if isinstance(value, dict) and "path" in value:
                    raise ValueError("Unnormalized path dependency in archive")
    return extracted, [entry.name for entry in entries if entry.isfile()]


API = {
    "xgpui": "pub fn size() -> gpui::Size<gpui::Pixels> { gpui::size(gpui::px(320.), gpui::px(240.)) }",
    "xgpui-macros": "pub use gpui_macros::Action;",
    "xgpui-apple": "pub use gpui_apple::AppleDispatcher;",
    "xgpui-macos": "pub use gpui_macos::MacPlatform;",
    "xgpui-windows": "pub use gpui_windows::WindowsPlatform;",
    "xgpui-linux": "pub use gpui_linux::current_platform;",
    "xgpui-web": "pub use gpui_web::{WebPlatform, WebBackendPreference};",
    "xgpui-platform": "pub use gpui_platform::{application, current_platform};",
    "xgpui-wgpu": "pub fn features() -> gpui_wgpu::wgpu::Features { gpui_wgpu::wgpu::Features::empty() }",
    "xgpui-color": "pub use xgpui_color::{EncodedRgb, RgbSpace, WGSL_COLOR_MATH};",
    "xgpui-shader": "pub use xgpui_shader::{painted, Paint};",
    "xgpui-gamut": "pub use xgpui_gamut::WideColor;",
    "xgpui-media": "pub fn filter() -> xgpui_media::ImageFilter { xgpui_media::ImageFilter::new().brightness(1.1) }",
    "xgpui-parley": "pub use xgpui_parley::ParleyTextSystem;",
    "xgpui-svg": "pub fn scheduler() -> xgpui_svg::SvgScheduler { xgpui_svg::SvgScheduler::default() }",
}

PROFILES = [
    ("xgpui-apple", "font-kit", ["font-kit"], True, "pub use gpui_apple::AppleTextSystem;"),
    ("xgpui-web", "single-threaded", [], False, API["xgpui-web"]),
    ("xgpui-web", "single-threaded-application", [], False, API["xgpui-web"]),
    ("xgpui-shader", "portable", ["portable-shaders"], True, "pub use xgpui_shader::{Brush, BrushStop, LayerEffect};"),
    ("xgpui-gamut", "instanced", ["instanced"], True, "pub use xgpui_gamut::instanced::{WidePainter, WideGradient};"),
    ("xgpui-media", "wgpu", ["wgpu"], True, API["xgpui-media"]),
    ("xgpui-parley", "system-fonts", ["system-fonts"], True, API["xgpui-parley"]),
]


def verify(root, output, offline, retry=False, only_profiles=False, nightly_web=False):
    output.mkdir(parents=True, exist_ok=retry)
    release = json.loads((root / "scripts/release-packages.json").read_text())
    report = {"started_at": datetime.now(timezone.utc).isoformat(), "packages": {}, "steps": [],
              "scope": "archive inspection and target-specific consumer checks; not registry publication or native execution"}
    report_path = output / "report.json"
    if retry:
        report = json.loads(report_path.read_text())
        report.setdefault("previous_consumer_results", []).append({
            name: {key: value for key, value in row.items() if key.startswith(("consumer", "dependency_metadata"))}
            for name, row in report["packages"].items()
        })
    attempt = len(report.get("previous_consumer_results", [])) + 1

    def save():
        report_path.write_text(json.dumps(report, indent=2) + "\n")

    home = output / "cargo-home"
    home.mkdir(exist_ok=retry)
    registry_cache = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo")) / "registry"
    if registry_cache.is_dir() and not (home / "registry").exists():
        (home / "registry").symlink_to(registry_cache.resolve(), target_is_directory=True)
    env = {key: value for key, value in os.environ.items() if not key.startswith("CARGO_")}
    for key in ("RUSTFLAGS", "RUSTDOCFLAGS", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "RUSTC", "RUSTDOC"):
        env.pop(key, None)
    env.update(CARGO_HOME=str(home), CARGO_TARGET_DIR=str(output / "target"), CLANG_MODULE_CACHE_PATH=str(output / "clang-cache"))
    report["rustc"] = subprocess.check_output(["rustc", "-vV"], text=True).strip()
    report["cargo"] = subprocess.check_output(["cargo", "--version"], text=True).strip()

    def run(label, args, toolchain=None, rustflags=None):
        command = ["cargo", *(["+" + toolchain] if toolchain else []), args[0], *(["--offline"] if offline else []), *args[1:]]
        log = output / "logs" / (label + f"-attempt{attempt}.log")
        log.parent.mkdir(exist_ok=True)
        print(label, flush=True)
        child_env = env.copy()
        if rustflags is not None:
            child_env["RUSTFLAGS"] = " ".join(rustflags)
        if toolchain:
            child_env["CARGO_TARGET_DIR"] = str(output / ("target-" + toolchain))
        with log.open("w") as stream:
            result = subprocess.run(command, cwd=output, env=child_env, text=True, stdout=stream, stderr=subprocess.STDOUT)
        report["steps"].append({"name": label, "command": command, "exit_code": result.returncode, "log": str(log),
                                "rustflags": rustflags})
        save()
        return result.returncode == 0, log

    extracted_paths = {}
    staged_paths = {}
    for local, name in release["packages"].items():
        source = root / ("extensions/crates" if local.startswith("xgpui-") else "gpui/crates") / local
        stage = output / "staged" / source.relative_to(root)
        if not retry:
            shutil.copytree(source, stage, ignore=shutil.ignore_patterns("target", "Cargo.lock", ".git", ".cargo"))
        staged_paths[name] = stage
    for name, stage in staged_paths.items():
        if retry:
            row = report["packages"][name]
            if row.get("archive_status") == "passed":
                extracted_paths[name] = output / "unpacked" / (name + "-" + row["version"])
            continue
        package = tomllib.loads((stage / "Cargo.toml").read_text())["package"]
        row = report["packages"][name] = {"version": package["version"], "consumer_status": "not-run"}
        ok, log = run("package-" + name, ["package", "--no-verify", "--exclude-lockfile", "--manifest-path", str(stage / "Cargo.toml")])
        if not ok:
            row.update(archive_status="failed", log=str(log))
            continue
        package_id = name + "-" + package["version"]
        archive = output / "target/package" / (package_id + ".crate")
        try:
            extracted, files = inspect_archive(archive, stage, output / "unpacked", package_id)
            row.update(archive_status="passed", archive=str(archive), files=files,
                       archive_sha256=hashlib.sha256(archive.read_bytes()).hexdigest())
            extracted_paths[name] = extracted
        except ValueError as error:
            row.update(archive_status="failed", error=str(error))
        save()

    candidates = {name: output / "validation-copies" / name for name in extracted_paths}
    for name, extracted in extracted_paths.items():
        if not retry:
            shutil.copytree(extracted, candidates[name])
    for name, directory in candidates.items():
        path = directory / "Cargo.toml"
        document = tomllib.loads(path.read_text())
        report["packages"][name]["validation_only_rewrites"] = rewire_candidates(document, candidates)
        path.write_text(dump_toml(document))
    save()

    host = next(line.split(": ", 1)[1] for line in report["rustc"].splitlines() if line.startswith("host: "))
    targets = {"xgpui-windows": "x86_64-pc-windows-msvc", "xgpui-linux": "x86_64-unknown-linux-gnu", "xgpui-web": "wasm32-unknown-unknown"}
    for name, directory in candidates.items():
        if only_profiles:
            continue
        if name == "xgpui-color":
            report["packages"][name]["consumer_status"] = "covered-by-dedicated-color-validator"
            continue
        if retry and report["packages"][name].get("consumer_status") == "passed" and report["packages"][name].get("dependency_metadata_status") == "passed":
            continue
        target = targets.get(name, host)
        row = report["packages"][name]
        row["target"] = target
        consumer = output / "consumers" / name
        (consumer / "src").mkdir(parents=True, exist_ok=retry)
        package = tomllib.loads((directory / "Cargo.toml").read_text())["package"]
        document = tomllib.loads((directory / "Cargo.toml").read_text())
        alias = document.get("lib", {}).get("name", name.replace("-", "_"))
        dependency = {"package": name, "version": "=" + package["version"], "path": str(directory)}
        manifest = {"package": {"name": "consumer-" + name, "version": "0.0.0", "edition": "2024", "publish": False},
                    "workspace": {}, "dependencies": {alias: dependency}}
        manifest_path = consumer / "Cargo.toml"
        manifest_path.write_text(dump_toml(manifest))
        (consumer / "src/lib.rs").write_text(API[name] + "\n")
        ok, log = run("consumer-" + name, ["check", "--jobs", "4", "--target", target, "--manifest-path", str(manifest_path)])
        row.update(consumer_status="passed" if ok else "failed", consumer_log=str(log))
        # Resolve metadata separately so a compile failure still records where
        # candidate dependencies came from. No path may point into the checkout.
        ok_meta, meta_log = run("metadata-" + name, ["metadata", "--format-version", "1", "--manifest-path", str(manifest_path)])
        row["dependency_metadata_status"] = "failed"
        if ok_meta:
            text = meta_log.read_text()
            start = text.find('{"packages":')
            try:
                metadata, _ = json.JSONDecoder().raw_decode(text[start:])
                for resolved in metadata["packages"]:
                    path = Path(resolved["manifest_path"]).resolve()
                    if root == path or root in path.parents:
                        raise ValueError("Consumer resolved a checkout package")
                    if resolved["name"] in release["packages"].values():
                        expected = candidates.get(resolved["name"])
                        if expected is None or path != (expected / "Cargo.toml").resolve() or resolved["source"] is not None:
                            raise ValueError("Consumer resolved a candidate outside the extracted validation copies")
                row["dependency_metadata_status"] = "passed"
                row["resolved_candidates"] = sorted(p["name"] for p in metadata["packages"] if p["name"] in candidates)
            except (ValueError, KeyError) as error:
                row["dependency_metadata_error"] = str(error)
                row["consumer_status"] = "failed"
        save()
    for name, profile, features, defaults, api in PROFILES:
        if name not in candidates:
            continue
        row = report["packages"][name]
        if retry and row.get("feature_profiles", {}).get(profile, {}).get("status") == "passed":
            continue
        consumer = output / "feature-consumers" / name / profile
        (consumer / "src").mkdir(parents=True, exist_ok=retry)
        document = tomllib.loads((candidates[name] / "Cargo.toml").read_text())
        alias = document.get("lib", {}).get("name", name.replace("-", "_"))
        manifest = {"package": {"name": "feature-consumer-" + name + "-" + profile, "version": "0.0.0", "edition": "2024", "publish": False},
                    "workspace": {}, "dependencies": {alias: {"package": name, "version": "=" + row["version"], "path": str(candidates[name]),
                                                              "default-features": defaults, "features": features}}}
        if profile == "single-threaded-application":
            # Match the maintained lab's explicit non-atomic, single-threaded
            # application contract rather than pretending the bare crate suffices.
            manifest["dependencies"]["wgpu"] = {"version": "=30.0.1", "default-features": False,
                                                   "features": ["fragile-send-sync-non-atomic-wasm"]}
        manifest_path = consumer / "Cargo.toml"
        manifest_path.write_text(dump_toml(manifest))
        (consumer / "src/lib.rs").write_text(api + "\n")
        ok, log = run("feature-" + name + "-" + profile, ["check", "--jobs", "4", "--target", row["target"], "--manifest-path", str(manifest_path)])
        row.setdefault("feature_profiles", {})[profile] = {"status": "passed" if ok else "failed", "features": features,
                                                           "default_features": defaults, "log": str(log)}
        save()
    if nightly_web and "xgpui-web" in candidates:
        config = tomllib.loads((root / "gpui/crates/gpui-web/examples/hello_web/.cargo/config.toml").read_text())
        flags = config["target"]["wasm32-unknown-unknown"]["rustflags"]
        manifest = output / "consumers/xgpui-web/Cargo.toml"
        ok, log = run("feature-xgpui-web-multithreaded-nightly",
                      ["check", "-Z", "build-std=std,panic_abort", "--jobs", "4", "--target", "wasm32-unknown-unknown", "--manifest-path", str(manifest)],
                      toolchain="nightly", rustflags=flags)
        report["packages"]["xgpui-web"].setdefault("feature_profiles", {})["multithreaded-nightly"] = {
            "status": "passed" if ok else "failed", "default_features": True, "rustflags": flags, "log": str(log),
            "rustc": subprocess.check_output(["rustup", "run", "nightly", "rustc", "-vV"], text=True).strip()}
        save()
    native_fixture = root / "scripts/fixtures/native-package-consumer"
    fixture_hash = hashlib.sha256(b"".join(path.relative_to(native_fixture).as_posix().encode() + path.read_bytes()
                                           for path in sorted(native_fixture.rglob("*")) if path.is_file()
                                           and path.name != "Cargo.lock" and "target" not in path.parts)).hexdigest()
    if "apple-darwin" in host and not (retry and report.get("native_execution", {}).get("status") == "passed"
                                      and report["native_execution"].get("fixture_sha256") == fixture_hash):
        consumer = output / "combined-consumer"
        shutil.copytree(root / "scripts/fixtures/native-package-consumer", consumer, dirs_exist_ok=retry,
                        ignore=shutil.ignore_patterns("target", "Cargo.lock"))
        path = consumer / "Cargo.toml"
        manifest = tomllib.loads(path.read_text())
        rewrites = rewire_candidates(manifest, candidates)
        path.write_text(dump_toml(manifest))
        ok, log = run("native-consumer-run", ["run", "--jobs", "4", "--target", host, "--manifest-path", str(path)])
        report["native_execution"] = {"status": "passed" if ok else "failed", "log": str(log), "candidate_rewrites": rewrites,
                                      "fixture_sha256": fixture_hash}
    report["finished_at"] = datetime.now(timezone.utc).isoformat()
    report["release_ready"] = False
    report["status"] = "passed" if all(row.get("archive_status") == "passed" and row.get("consumer_status") in ("passed", "covered-by-dedicated-color-validator")
                                         and all(profile["status"] == "passed" for profile in row.get("feature_profiles", {}).values())
                                         for row in report["packages"].values()) else "completed-with-blockers"
    if report.get("native_execution", {}).get("status") == "failed":
        report["status"] = "completed-with-blockers"
    save()
    print(f"Report: {report_path}", flush=True)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--offline", action="store_true")
    parser.add_argument("--retry-consumers", action="store_true", help="Retry failed consumers against the existing archived snapshot, preserving prior results")
    parser.add_argument("--profiles-only", action="store_true", help="With --retry-consumers, run only pending feature profiles")
    parser.add_argument("--nightly-web", action="store_true", help="Also check default browser features using installed nightly, rust-src and the documented atomic-WASM flags")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    output = args.out.resolve()
    if output == root or root in output.parents:
        parser.error("--out must be outside the checkout")
    if args.profiles_only and not args.retry_consumers:
        parser.error("--profiles-only requires --retry-consumers")
    report = verify(root, output, args.offline, args.retry_consumers, args.profiles_only, args.nightly_web)
    if report["status"] != "passed":
        raise SystemExit(1)


if __name__ == "__main__":
    main()
