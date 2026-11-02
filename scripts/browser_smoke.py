#!/usr/bin/env python3
"""Real Chromium startup/resize smoke checks; not visual parity or full UI acceptance."""
from __future__ import annotations

import argparse
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import json
import fnmatch
import platform
from pathlib import Path
import re
import threading

ROOT = Path(__file__).resolve().parents[1]


class QuietHandler(SimpleHTTPRequestHandler):
    def log_message(self, *_args):
        pass


def routes(site):
    result = []
    for gallery, parameter in [("extensions", "demo"), ("shaders", "example")]:
        index = site / "examples" / gallery / "index.html"
        if not index.is_file():
            raise FileNotFoundError(f"Missing {index}; run the web build/staging steps")
        for name in sorted(set(re.findall(r"\?" + parameter + r"=([a-z0-9-]+)", index.read_text()))):
            if gallery == "extensions" and name == "italy":
                result.append(("extensions-italy-dom", "/examples/extensions/?demo=italy", "dom"))
                continue
            for backend in ["webgpu", "webgl"]:
                result.append((f"{gallery}-{name}-{backend}",
                               f"/examples/{gallery}/?{parameter}={name}&backend={backend}", backend))
    if not result:
        raise ValueError("No gallery demo links found")
    return result


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--site", type=Path, default=ROOT / "target/site")
    parser.add_argument("--headed", action="store_true")
    parser.add_argument("--only", action="append", default=[], help="partial smoke: match case name globs")
    args = parser.parse_args(argv)
    args.output.mkdir(parents=True, exist_ok=True)
    try:
        from playwright.sync_api import sync_playwright
    except ImportError:
        print("UNTESTED: Playwright is missing. See docs/LOCAL_DEVELOPMENT.md test runner setup.")
        return 2
    try:
        cases = routes(args.site)
        if args.only:
            cases = [case for case in cases if any(fnmatch.fnmatchcase(case[0], pattern) for pattern in args.only)]
            if not cases:
                parser.error("--only matched no browser cases")
    except (OSError, ValueError) as error:
        print(f"FAILED: {error}")
        return 1
    server = ThreadingHTTPServer(("127.0.0.1", 0), partial(QuietHandler, directory=str(args.site)))
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    base = f"http://127.0.0.1:{server.server_port}"
    results = []
    try:
        with sync_playwright() as playwright:
            try:
                browser = playwright.chromium.launch(channel="chromium", headless=not args.headed,
                    args=["--enable-gpu"])
            except Exception as error:
                print(f"UNTESTED: Chromium could not launch: {error}")
                return 2
            try:
                metadata = dict(browser=browser.version, host=platform.platform(), selection=args.only or "all",
                                channel="chromium", args=["--enable-gpu"])
                probe = browser.new_page()
                probe.goto(base)
                capabilities = probe.evaluate("""async () => {
                    let gpu = null;
                    try { gpu = await navigator.gpu?.requestAdapter(); } catch (_) {}
                    return {webgpu: !!gpu, webgl: !!document.createElement('canvas').getContext('webgl2'), dom: true,
                        adapter: gpu ? {vendor: gpu.info?.vendor, architecture: gpu.info?.architecture, description: gpu.info?.description} : null};
                }""")
                probe.close()
                for name, path, backend in cases:
                    row = dict(name=name, url=base + path, backend=backend,
                               status="untested", detail="not run")
                    results.append(row)
                    if not capabilities[backend]:
                        row.update(status="unsupported", detail=f"Chromium has no {backend} adapter/context")
                        continue
                    context = browser.new_context(viewport={"width": 1100, "height": 760}, device_scale_factor=1)
                    errors = []
                    page = context.new_page()
                    def record_error(error):
                        text = str(error)
                        if text not in errors and len(errors) < 50:
                            errors.append(text)
                    page.on("pageerror", record_error)
                    page.on("console", lambda message: record_error(message.text) if message.type == "error" else None)
                    page.add_init_script("""(() => {
                        window.__gpuiContexts = [];
                        const original = HTMLCanvasElement.prototype.getContext;
                        HTMLCanvasElement.prototype.getContext = function(kind, ...rest) {
                            const result = original.call(this, kind, ...rest);
                            if (result) window.__gpuiContexts.push(kind);
                            return result;
                        };
                    })()""")
                    try:
                        page.goto(base + path, wait_until="load", timeout=60000)
                        page.wait_for_function("""() => {
                            const status = document.querySelector('#status');
                            const error = document.querySelector('#error');
                            const ready = (document.querySelector('canvas') || document.querySelector('#italy-demo')) && (status ? status.hidden :
                                document.querySelector('#index')?.hidden && error?.hidden);
                            return ready || (error && !error.hidden) ||
                                (status && !status.hidden && !status.textContent.startsWith('Loading'));
                        }""", timeout=60000)
                        visible_error = page.locator('#error:not([hidden]), #status:not([hidden])')
                        if visible_error.count():
                            raise AssertionError(visible_error.first.inner_text())
                        if backend == "dom":
                            page.wait_for_function("() => {const i=document.querySelector('#italy-main');return i?.complete && i.naturalWidth>0;}")
                            page.locator('input[name="italy-mode"][value="srgb"]').check()
                            page.wait_for_function("() => {const i=document.querySelector('#italy-main');return i.src.startsWith('blob:') && i.complete && i.naturalWidth>0;}")
                            page.locator('#italy-thumbnails button').first.click()
                            page.wait_for_function("() => document.querySelector('#italy-selection').textContent.includes('sRGB · sRGB preview')")
                            page.screenshot(path=str(args.output / (name + ".png")))
                            page.set_viewport_size({"width": 820, "height": 620})
                            page.screenshot(path=str(args.output / (name + "-resized.png")))
                            if errors:
                                raise AssertionError("; ".join(errors))
                            row.update(status="passed", detail="DOM images loaded, sRGB preview and source selection, resize; no browser errors", screenshot=name + ".png")
                            continue
                        canvas = page.locator("canvas").first
                        canvas.wait_for(state="visible", timeout=10000)
                        page.wait_for_timeout(1500)  # Permit asynchronous GPU failures to reach the browser error channel.
                        contexts = page.evaluate("window.__gpuiContexts")
                        expected = "webgpu" if backend == "webgpu" else "webgl2"
                        if expected not in contexts:
                            raise AssertionError(f"Forced {backend} did not create {expected}: {contexts}")
                        page.screenshot(path=str(args.output / (name + ".png")))
                        width = canvas.evaluate("c => c.getBoundingClientRect().width")
                        page.set_viewport_size({"width": 820, "height": 620})
                        page.wait_for_function("previous => document.querySelector('canvas')?.getBoundingClientRect().width !== previous", arg=width, timeout=10000)
                        page.wait_for_timeout(300)
                        page.screenshot(path=str(args.output / (name + "-resized.png")))
                        visible_error = page.locator('#error:not([hidden]), #status:not([hidden])')
                        if visible_error.count():
                            raise AssertionError(visible_error.first.inner_text())
                        if errors:
                            raise AssertionError("; ".join(errors))
                        row.update(status="passed", detail="loaded, visible canvas, forced backend, resize; no browser errors",
                                   contexts=contexts, screenshot=name + ".png")
                    except Exception as error:
                        row.update(status="failed", detail=str(error), browser_errors=errors)
                        try:
                            page.screenshot(path=str(args.output / (name + "-failure.png")))
                        except Exception:
                            pass
                    finally:
                        context.close()
                        (args.output / "report.json").write_text(json.dumps(dict(
                            scope="startup/resize smoke, not visual correctness or all controls",
                            metadata=metadata, capabilities=capabilities, checks=results), indent=2) + "\n")
                        print(row["status"].upper(), name, row["detail"], flush=True)
            finally:
                browser.close()
    finally:
        server.shutdown()
        server.server_close()
        thread.join()
    (args.output / "report.json").write_text(json.dumps(dict(
        scope="startup/resize smoke, not visual correctness or all controls",
        metadata=metadata, capabilities=capabilities, checks=results), indent=2) + "\n")
    if any(row["status"] == "failed" for row in results):
        return 1
    return 2 if any(row["status"] != "passed" for row in results) else 0


if __name__ == "__main__":
    raise SystemExit(main())
