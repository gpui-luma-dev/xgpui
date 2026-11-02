import contextlib
import io
import json
import os
from pathlib import Path
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

import run_tests as runner
import browser_smoke


class RunnerTests(unittest.TestCase):
    def test_host_exclusions_and_critical_gpu_coverage(self):
        for host in ["Darwin", "Windows", "Linux"]:
            plan = runner.native_plan("dev", Path("/tmp/report"), host)
            checks = {c.name: c for c in plan}
            self.assertIsNone(checks[{"Darwin": "gpui-apple", "Windows": "gpui-windows", "Linux": "gpui-linux"}[host]].support)
            self.assertIsNotNone(checks["gpui-web"].support)
            for name in ["gpui", "gpui-wgpu", "xgpui-media", "lab-tests", "dense-instanced-parity", "paint-conic"]:
                self.assertIn(name, checks)
            self.assertEqual(len(checks), len(plan), "report IDs must be unique")
            self.assertNotIn("--locked", checks["xgpui-media"].command)
            self.assertIn("--locked", checks["lab-tests"].command)

    def test_failed_and_unrun_checks_cannot_produce_success(self):
        with tempfile.TemporaryDirectory() as directory:
            report = dict(mode="test", host="test", revision="test", profile="dev", selection="all",
                          checks=[dict(name="a", status="passed", detail="ok"),
                                  dict(name="b", status="untested", detail="not run")])
            runner.save_report(Path(directory), report)
            self.assertEqual(report["result"], "incomplete")
            report["checks"][1]["status"] = "failed"
            runner.save_report(Path(directory), report)
            self.assertEqual(report["result"], "failed")
            report["checks"] = [dict(name="foreign", status="unsupported", detail="other host")]
            runner.save_report(Path(directory), report)
            self.assertEqual(report["result"], "incomplete")
            self.assertEqual(json.loads((Path(directory) / "report.json").read_text())["result"], "incomplete")

    def test_manual_zero_exit_does_not_count_without_terminal_acceptance(self):
        with tempfile.TemporaryDirectory() as directory, patch("sys.stdin.isatty", return_value=False):
            status, _, _ = runner.execute(runner.Check("ui", [sys.executable, "-c", "pass"], kind="manual"),
                                           Path(directory) / "ui.log", 1,
                                           prompt=lambda _: self.fail("must not prompt outside a terminal"))
            self.assertEqual(status, "untested")

    def test_manual_result_requires_operator_and_successful_process_exit(self):
        class Process:
            def __init__(self, code):
                self.returncode = code
            def poll(self):
                return self.returncode
        with tempfile.TemporaryDirectory() as directory, patch("sys.stdin.isatty", return_value=True):
            for code, answer, expected in [(0, "pass inspected controls", "passed"),
                                            (1, "pass", "failed"),
                                            (0, "", "untested"),
                                            (0, "unsupported no wide surface", "unsupported")]:
                with patch.object(runner.subprocess, "Popen", return_value=Process(code)):
                    with contextlib.redirect_stdout(io.StringIO()):
                        status, _, _ = runner.execute(runner.Check("ui", [sys.executable], kind="manual"),
                            Path(directory) / "ui.log", 1, prompt=lambda _: answer)
                    self.assertEqual(status, expected)

    def test_command_exit_two_is_failure_unless_browser_reports_missing_prerequisite(self):
        with tempfile.TemporaryDirectory() as directory:
            for name, expected in [("cargo", "failed"), ("browser-smoke", "untested")]:
                status, _, _ = runner.execute(runner.Check(name, [sys.executable, "-c", "raise SystemExit(2)"]),
                                               Path(directory) / (name + ".log"), 3)
                self.assertEqual(status, expected)

    @unittest.skipIf(os.name == "nt", "POSIX process-group test")
    def test_timeout_terminates_child_process_tree(self):
        with tempfile.TemporaryDirectory() as directory:
            marker = Path(directory) / "leaked-child"
            child = "import time,pathlib;time.sleep(1);pathlib.Path(" + repr(str(marker)) + ").touch()"
            parent = "import subprocess,sys,time;subprocess.Popen([sys.executable,'-c'," + repr(child) + "]);time.sleep(20)"
            status, detail, _ = runner.execute(runner.Check("timeout", [sys.executable, "-c", parent]),
                                               Path(directory) / "timeout.log", .2)
            self.assertEqual(status, "failed")
            self.assertIn("deadline", detail)
            time.sleep(1.1)
            self.assertFalse(marker.exists(), "grandchild escaped the timeout")

    def test_failed_web_build_prevents_stale_browser_validation(self):
        checks = [runner.Check("web-build", [sys.executable, "-c", "raise SystemExit(1)"]),
                  runner.Check("web-stage", [sys.executable, "-c", "pass"], requires=["web-build"]),
                  runner.Check("browser-smoke", [sys.executable, "-c", "pass"], requires=["web-stage"])]
        with tempfile.TemporaryDirectory() as directory, patch.object(runner, "web_plan", return_value=checks):
            output = Path(directory) / "run"
            with contextlib.redirect_stdout(io.StringIO()):
                code = runner.main(["web", "--output", str(output)])
            self.assertEqual(code, 1)
            report = json.loads((output / "report.json").read_text())
            self.assertEqual([r["status"] for r in report["checks"]], ["failed", "untested", "untested"])

    def test_gallery_discovery_covers_both_collections_and_forces_backends(self):
        with tempfile.TemporaryDirectory() as directory:
            site = Path(directory)
            for gallery, parameter in [("extensions", "demo"), ("shaders", "example")]:
                index = site / "examples" / gallery / "index.html"
                index.parent.mkdir(parents=True)
                index.write_text(f'<a href="?{parameter}=one">one</a><a href="?{parameter}=one&backend=webgl">one</a>')
            cases = browser_smoke.routes(site)
            self.assertEqual(len(cases), 4)
            self.assertEqual({r[2] for r in cases}, {"webgpu", "webgl"})


if __name__ == "__main__":
    unittest.main()
