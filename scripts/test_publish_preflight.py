import json
from pathlib import Path
import tempfile
import unittest

from publish_preflight import audit, dependencies, dependency_layers


class PublishPreflightTests(unittest.TestCase):
    def test_target_aliases_and_optional_edges_are_preserved(self):
        document = {"target": {"cfg(windows)": {"dependencies": {
            "core": {"package": "xgpui-core", "version": "=1.0.0", "optional": True}
        }}}}
        dep, = dependencies(document)
        self.assertEqual(dep["package"], "xgpui-core")
        self.assertEqual(dep["alias"], "core")
        self.assertEqual(dep["target"], "cfg(windows)")
        self.assertTrue(dep["optional"])

    def test_cycles_do_not_produce_a_false_complete_order(self):
        layers, blocked = dependency_layers({"a": {"b"}, "b": {"a"}, "c": {"a"}, "leaf": set()})
        self.assertEqual(layers, [["leaf"]])
        self.assertEqual(blocked, ["a", "b", "c"])

    def test_audit_distinguishes_dev_cycles_and_flags_bad_release_inputs(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "scripts").mkdir()
            (root / ".cargo").mkdir()
            (root / "scripts/release-packages.json").write_text(json.dumps({
                "packages": {"a": "a", "b": "b"}, "version": "1.0.0"
            }))
            (root / "upstream.json").write_text(json.dumps({"packages": {"a": "a", "b": "b"}}))
            for name in ("a", "b"):
                (root / name).mkdir()
            (root / "a/Cargo.toml").write_text('''[package]
name = "a"
version = "1.0.0"
publish = false
[dependencies]
renamed = { package = "b", version = "=0.9.0", path = "../wrong", optional = true }
''')
            (root / "b/Cargo.toml").write_text('''[package]
name = "b"
version = "1.0.0"
description = "fixture"
license = "MIT"
repository = "https://example.test"
[dev-dependencies]
a = "=1.0.0"
remote = { git = "https://example.test/dependency" }
''')
            for relative in ("Cargo.toml", ".cargo/config.toml", ".cargo/validation.toml"):
                (root / relative).write_text('[patch.crates-io]\nshim = { path = "vendor/shim" }\n')
            result = audit(root)
            self.assertFalse(result["release_ready"])
            self.assertEqual(result["dependency_first_layers"], [["b"], ["a"]])
            self.assertEqual(result["cycle_blocked_packages"], [])
            self.assertEqual(result["including_dev_cycle_blocked_packages"], ["a", "b"])
            issues = [entry["issue"] for entry in result["issues"]]
            for issue in ("publishing-disabled", "missing-description", "missing-license"):
                self.assertIn(issue, issues)
            self.assertTrue(any(issue.startswith("internal-version-mismatch:") for issue in issues))
            self.assertTrue(any(issue.startswith("internal-path-mismatch:") for issue in issues))
            self.assertTrue(any(issue.startswith("dependency-without-registry-version:") for issue in issues))
            self.assertEqual(len(result["patches_to_review_outside_repository"]), 3)


if __name__ == "__main__":
    unittest.main()
