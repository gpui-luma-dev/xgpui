#!/usr/bin/env python3
"""Repository test orchestration. No dependencies beyond Python 3.11+."""
from __future__ import annotations

import argparse
from collections import Counter
from dataclasses import dataclass, field
from datetime import datetime, timezone
import fnmatch
import json
import os
from pathlib import Path
import platform
import shlex
import shutil
import signal
import subprocess
import sys
import time
import tomllib

ROOT = Path(__file__).resolve().parents[1]
LAB = "apps/xgpui-ext-lab/Cargo.toml"


@dataclass
class Check:
    name: str
    command: list[str]
    kind: str = "automated"
    support: str | None = None
    gpu: bool = False
    display: bool = False
    checklist: list[str] = field(default_factory=list)
    env: dict[str, str] = field(default_factory=dict)
    requires: list[str] = field(default_factory=list)
    library: bool = False


def cargo(action, manifest, profile, *args):
    return ["cargo", action, "--locked", "--config", ".cargo/validation.toml",
            "--manifest-path", manifest, "--target-dir", "target", "--profile", profile, *args]


def app(profile, binary, *args):
    return cargo("run", LAB, profile, "--bin", binary, "--features",
                 "runtime-shaders,portable-shaders,wgpu", "--", *map(str, args))


def native_plan(profile, output, host):
    checks = [Check("python-tooling", [sys.executable, "-m", "unittest", "discover",
                                      "-s", "scripts", "-p", "test_*.py"])]
    for manifest in sorted((ROOT / "tools").glob("*/Cargo.toml")):
        checks.append(Check("tool-" + manifest.parent.name,
                            ["cargo", "test", "--locked", "--manifest-path", str(manifest.relative_to(ROOT)),
                             "--target-dir", "target", "--profile", profile]))
    backend_hosts = {"gpui-apple": "Darwin", "gpui-macos": "Darwin",
                     "gpui-windows": "Windows", "gpui-linux": "Linux"}
    manifests = sorted((ROOT / "gpui/crates").glob("*/Cargo.toml"))
    manifests += sorted((ROOT / "extensions/crates").glob("*/Cargo.toml"))
    for manifest in manifests:
        name = manifest.parent.name
        data = tomllib.loads(manifest.read_text())
        features = data.get("features", {})
        selected = [f for f in ("test-support", "portable-shaders", "wgpu", "runtime_shaders")
                    if f in features]
        if name == "gpui-wgpu" and host == "Linux":
            selected.append("dma-buf")
        command = cargo("test", str(manifest.relative_to(ROOT)), profile, "--lib", "--tests")
        if selected:
            command += ["--features", ",".join(selected)]
        # Rustdoc examples are included as a separate suite below, not by --lib/--tests.
        command.remove("--locked")  # Standalone libraries may not track a lockfile.
        command += ["--", "--test-threads=1"]
        support = None
        if name in backend_hosts and host != backend_hosts[name]:
            support = f"requires {backend_hosts[name]}; current host is {host}"
        if name == "gpui-web":
            support = "Wasm backend: covered by test-web, not native cargo tests"
        gpu = name in {"gpui", "gpui-apple", "gpui-wgpu", "gpui-windows", "xgpui-shader", "xgpui-gamut", "xgpui-media"}
        checks.append(Check(name, command, support=support, gpu=gpu, library=True,
                            env={"MTL_DEBUG_LAYER": "1"} if host == "Darwin" and gpu else {}))
        if support is None:
            doc = cargo("test", str(manifest.relative_to(ROOT)), profile, "--doc")
            if selected:
                doc += ["--features", ",".join(selected)]
            doc.remove("--locked")
            checks.append(Check(name + "-doctests", doc, gpu=gpu, library=True))
    checks.append(Check("lab-tests", cargo("test", LAB, profile, "--bins", "--tests", "--features",
        "runtime-shaders,portable-shaders,wgpu" + (",native-window-tests" if host == "Windows" else ""),
        "--", "--test-threads=1"), gpu=True, display=True))
    checks.append(Check("lab-build", cargo("build", LAB, profile, "--bins", "--features",
                                           "runtime-shaders,portable-shaders,wgpu")))
    if host == "Darwin":
        checks.append(Check("issue110-regression", app(profile, "metal-producer-regression"),
                            gpu=True, display=True, env={"MTL_DEBUG_LAYER": "1"}))
    else:
        checks.append(Check("issue110-regression", [], support="original reproduction requires macOS Metal; native backend dense tests run above"))
    for name, args in [("media", []), ("media-adjustments", ["--adjustments"]),
                       ("media-paths", ["--paths"]), ("media-video", ["--video"])]:
        support = "generated native video fixture requires macOS" if name == "media-video" and host != "Darwin" else None
        checks.append(Check(name, app(profile, "media-lab", "--headless", *args,
                            "--png", output / (name + ".png")), gpu=True, support=support))
    for effect in ["linear", "radial", "conic", "diamond", "mesh"]:
        checks.append(Check("paint-" + effect, app(profile, "paint-lab", "--headless", "--effect",
                            effect, "--frames", "3", "--png", output / (effect + ".png")), gpu=True))
    checks += [Check("gamut-capture", app(profile, "gamut-lab", "--headless", "--frames", "3",
                                        "--png", output / "gamut.png"), gpu=True, display=True,
                     support=None if host in {"Darwin", "Windows"} else "native wide gamut fixture requires Metal/D3D11"),
               Check("dense-instanced-parity", app(profile, "portable-content-instanced", "--benchmark",
                     "--frames", "3", "--warmup", "1", "--json", output / "dense-instanced.json"), gpu=True),
               Check("canvas-input-smoke", app(profile, "canvas-lab", "--smoke", output / "canvas"),
                     gpu=True, display=True)]
    for binary in ["quad-bench", "color-field-bench"]:
        checks.append(Check(binary + "-capture", app(profile, binary, "--headless", "--frames", "3",
                      "--warmup", "1", "--png", output / (binary + ".png")), gpu=True))
    if host == "Linux":
        checks.append(Check("dma-buf-native", ["just", "media-dma-buf-test"], gpu=True))
    return checks


def ui_plan(profile, output, host):
    common = ["Resize and check clipping, rounded edges, alpha and text.",
              "Exercise the controls; verify updates, responsiveness and no flicker/freeze."]
    specs = [
        ("portable-content", "portable-content", [], ["Toggle D: 512 → 1 → 512 rapidly; no lone intermediate tile.", "R/O/C change color, opacity and clipping; ordinary UI keeps responding."]),
        ("portable-content-instanced", "portable-content-instanced", [], ["Use D/R/O/C; compare layout, colors and coverage with the original."]),
        ("paint-gradients", "paint-lab", [], ["Exercise linear/radial/conic/diamond/mesh; drag parameters rapidly and enable animation.", "Conic angle updates must remain smooth; no jitter/stale animation."]),
        ("shaders", "shader-examples", [], ["Switch all six examples, interact with their controls and pause/resume animation."]),
        ("media", "media-lab", [], ["Compare reference and media panels; change filtering and inspect alpha/mips."]),
        ("media-adjustments", "media-lab", ["--adjustments"], ["Change exposure/saturation/vignette, compare both placements."]),
        ("media-float", "media-lab", ["--float-images"], ["Check explicit sRGB preview versus negotiated native wide output; unsupported policies must be explicit."]),
        ("media-tiles", "media-lab", ["--tiled"], ["Pan/zoom and refine; no stale tiles, seams, invalid crop panic or alpha overlap."]),
        ("media-video", "media-lab", ["--video"], ["Cycle BGRA/NV12 formats; pause, burst and resume; both placements retain correct frames."]),
        ("media-paths", "media-lab", ["--paths"], ["Exercise trim/dash controls and animation."]),
        ("gamut", "gamut-lab", [], ["Inspect wide versus SDR controls, opacity and negotiated output status."]),
        ("canvas", "canvas-lab", [], ["Pan/zoom, select, move cards and exercise minimap/focus transitions."]),
        ("mandelbrot", "canvas-lab", ["--mandelbrot"], ["Pan/zoom rapidly; progressive results must match the current viewport."]),
        ("text", "text-lab", [], ["Compare native/Parley layout, edit and select text, scroll and inspect fallback."]),
        ("svg", "svg-lab", [], ["Resize/zoom and inspect scheduling, resolution and path coverage."]),
        ("quad-bench", "quad-bench", ["--continuous"], ["Inspect the quad workload and close the window after checking coverage/order."]),
        ("color-field-bench", "color-field-bench", ["--continuous"], ["Inspect color-field coverage; this is visual acceptance, not a performance measurement."]),
        ("color", "color-lab", [], ["Change color-space/mapping controls and verify reference labels/values."]),
    ]
    checks = [Check("lab-build", cargo("build", LAB, profile, "--bins", "--features",
                                      "runtime-shaders,portable-shaders,wgpu"))]
    for name, binary, args, specific in specs:
        support = None
        if name == "media-video" and host != "Darwin":
            support = "interactive generated CoreVideo fixture requires macOS; use native import tests on Windows/Linux"
        if name == "gamut" and host not in {"Darwin", "Windows"}:
            support = "native wide gamut comparison requires Metal/D3D11"
        checks.append(Check(name, app(profile, binary, *args), kind="manual", support=support,
                            display=True, checklist=common + specific))
    return checks


def web_plan(profile, output, host):
    return [
        Check("web-color-tests", cargo("test", LAB, profile, "--no-default-features", "--features", "web",
                                      "--test", "color-tasks", "--test", "italy-photo")),
        Check("docs-generate", ["cargo", "run", "--locked", "--manifest-path", "tools/crate-roadmap/Cargo.toml", "--", str(ROOT)]),
        Check("docs-verify", ["node", "docs/verify.mjs"]),
        Check("docs-code-examples", ["node", "--test", "docs/code-examples.test.mjs"]),
        Check("web-build", ["just", "web-build"]),
        Check("web-stage", [sys.executable, "scripts/stage_website.py"], requires=["web-build", "docs-generate"]),
        Check("browser-smoke", [sys.executable, "scripts/browser_smoke.py", "--output", str(output / "browser")], requires=["web-stage"]),
    ]


def stop_process(process):
    if process.poll() is not None:
        return
    if os.name == "nt":
        subprocess.run(["taskkill", "/PID", str(process.pid), "/T", "/F"], capture_output=True)
    else:
        try:
            os.killpg(process.pid, signal.SIGTERM)
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
    process.wait()


def execute(check, logfile, timeout, prompt=input):
    """A process exiting zero cannot by itself pass a manual check."""
    if not check.command:
        return "untested", "no command", None
    if not shutil.which(check.command[0]):
        return "untested", "missing executable: " + check.command[0], None
    if check.kind == "manual" and not sys.stdin.isatty():
        return "untested", "interactive acceptance requires a terminal; use --list to see checklists", None
    with logfile.open("w") as log:
        log.write(shlex.join(check.command) + "\n")
        log.flush()
        process = subprocess.Popen(check.command, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT,
                                   env={**os.environ, **check.env}, start_new_session=os.name != "nt")
        try:
            if check.kind == "manual":
                for item in check.checklist:
                    print("  - " + item, flush=True)
                print("Inspect the lab, close its window, then record your result.", flush=True)
                answer = prompt("Result [pass/fail/unsupported/untested] + optional note: ").strip()
                status, _, note = answer.partition(" ")
                status = {"pass": "passed", "fail": "failed", "untested": "untested", "unsupported": "unsupported"}.get(status.lower(), "untested")
                if status == "unsupported" and not note:
                    status, note = "untested", "unsupported requires a capability reason"
                if process.poll() is None and status != "failed":
                    status, note = "untested", "lab still running; close it before recording acceptance"
                elif process.poll() is not None and process.returncode != 0:
                    status, note = "failed", f"lab exited {process.returncode}; {note}"
                return status, note or "recorded by operator", process.poll()
            code = process.wait(timeout=timeout)
            return ("passed" if code == 0 else "untested" if code == 2 and check.name == "browser-smoke" else "failed",
                    "completed" if code == 0 else f"exit {code}; see log", code)
        except subprocess.TimeoutExpired:
            return "failed", f"exceeded {timeout}s wall-clock deadline; process tree terminated", None
        finally:
            stop_process(process)


def save_report(output, report):
    counts = Counter(row["status"] for row in report["checks"])
    report["summary"] = dict(counts)
    report["result"] = "failed" if counts["failed"] else "incomplete" if counts["untested"] or not counts["passed"] else "passed"
    (output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    lines = [f"# {report['mode']} — {report['result']}", "", f"Host: {report['host']}; revision: {report['revision']}",
             f"Profile: {report['profile']}; selected coverage: {report['selection']}", "",
             "Unsupported means not applicable to this host. Untested never counts as passed.", "",
             "| Check | Result | Detail / log |", "| --- | --- | --- |"]
    for row in report["checks"]:
        detail = row["detail"].replace("|", "\\|").replace("\n", " ")
        link = f" [{row['log']}]({row['log']})" if row.get("log") else ""
        lines.append(f"| {row['name']} | {row['status']} | {detail}{link} |")
    for row in report["checks"]:
        if row.get("checklist"):
            lines += ["", f"## {row['name']} checklist", ""]
            lines += ["- " + item for item in row["checklist"]]
    (output / "report.md").write_text("\n".join(lines) + "\n")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=["all", "ui", "web"])
    parser.add_argument("--profile", default="release")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--only", action="append", default=[], help="glob of check names; repeat or use comma-separated patterns")
    parser.add_argument("--list", action="store_true", help="show commands, checklists and host exclusions without running")
    parser.add_argument("--timeout", type=float, default=1800, help="per automated command, including builds")
    parser.add_argument("--no-gpu", action="store_true", help="record GPU checks as untested")
    parser.add_argument("--locked-libraries", action="store_true", help="require existing up-to-date standalone library locks instead of allowing Cargo resolution")
    parser.add_argument("--include-dma-buf", action="store_true", help="attempt Linux native DMA-BUF fixture; requires suitable hardware/layers")
    args = parser.parse_args(argv)
    if args.timeout <= 0:
        parser.error("--timeout must be positive")
    host = platform.system()
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ")
    output = (args.output or ROOT / "target/test-runs" / f"{stamp}-{args.mode}").resolve()
    checks = {"all": native_plan, "ui": ui_plan, "web": web_plan}[args.mode](args.profile, output, host)
    if args.locked_libraries:
        for check in checks:
            if check.library:
                check.command.insert(2, "--locked")
    patterns = [part for value in args.only for part in value.split(",") if part]
    selected = [c for c in checks if not patterns or any(fnmatch.fnmatchcase(c.name, p) for p in patterns)]
    if not selected:
        parser.error("--only matched no checks")
    if args.list:
        for check in selected:
            print(check.name + (" [unsupported: " + check.support + "]" if check.support else " [" + check.kind + "]"))
            print("  " + shlex.join(check.command))
            for item in check.checklist:
                print("  - " + item)
        return 0
    if output.exists():
        parser.error("--output must name a new directory; existing reports are never overwritten")
    output.mkdir(parents=True, exist_ok=False)
    revision = subprocess.run(["git", "rev-parse", "HEAD"], cwd=ROOT, capture_output=True, text=True).stdout.strip()
    report = dict(schema=1, mode=args.mode, host=platform.platform(), revision=revision, profile=args.profile,
                  selection=patterns or "all", started=stamp,
                  dirty=bool(subprocess.run(["git", "status", "--porcelain"], cwd=ROOT, capture_output=True, text=True).stdout),
                  checks=[dict(name=c.name, command=c.command, kind=c.kind, checklist=c.checklist, requires=c.requires,
                               status="untested", detail="not run yet") for c in selected])
    print(f"Reports and logs: {output}", flush=True)
    save_report(output, report)
    try:
        for check, row in zip(selected, report["checks"]):
            states = {r["name"]: r["status"] for r in report["checks"]}
            dependencies = [n for n in check.requires if n in states and states[n] != "passed"]
            reason = check.support
            if dependencies:
                row.update(status="untested", detail="prerequisite did not pass: " + ", ".join(dependencies))
            elif reason:
                row.update(status="unsupported", detail=reason)
            elif args.no_gpu and check.gpu:
                row.update(status="untested", detail="GPU checks disabled by --no-gpu")
            elif check.name == "dma-buf-native" and not args.include_dma_buf:
                row.update(status="untested", detail="requires explicit --include-dma-buf; no capability inferred")
            elif check.display and host == "Linux" and not (os.environ.get("DISPLAY") or os.environ.get("WAYLAND_DISPLAY")):
                row.update(status="untested", detail="requires an active X11/Wayland graphical session")
            else:
                print(f"RUN {check.name}", flush=True)
                start = time.monotonic()
                logfile = output / (check.name + ".log")
                row["log"] = logfile.name
                try:
                    status, detail, code = execute(check, logfile, args.timeout)
                    row.update(status=status, detail=detail, exit_code=code)
                except OSError as error:
                    row.update(status="failed", detail=str(error))
                row["seconds"] = round(time.monotonic() - start, 3)
                if logfile.exists():
                    row["warning_lines"] = sum(line.lstrip().startswith("warning:") for line in logfile.read_text(errors="replace").splitlines())
                if check.library:
                    manifest = ROOT / check.command[check.command.index("--manifest-path") + 1]
                    lock = manifest.parent / "Cargo.lock"
                    if lock.exists():
                        destination = output / (check.name + ".Cargo.lock")
                        shutil.copyfile(lock, destination)
                        row["resolved_lock"] = destination.name
            print(f"{row['status'].upper()} {check.name}: {row['detail']}", flush=True)
            save_report(output, report)
    except (KeyboardInterrupt, EOFError):
        print("Interrupted; remaining checks are untested.", flush=True)
    save_report(output, report)
    print(f"{report['result'].upper()}: {report['summary']}\n{output / 'report.md'}", flush=True)
    return 1 if report["result"] == "failed" else 2 if report["result"] == "incomplete" else 0


if __name__ == "__main__":
    raise SystemExit(main())
