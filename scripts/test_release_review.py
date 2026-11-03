"""Tree review proposals are deliberately separate from integration."""
import json
import pathlib
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from import_release import git
from release_review import POLICIES, apply, classify, compare, prepare


class ReviewTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.base = pathlib.Path(self.temporary.name)
        self.root, self.before, self.incoming = [self.base / n for n in ("xgpui", "old", "new")]
        for tree in (self.root, self.before, self.incoming):
            package = tree / "gpui/crates/gpui"
            package.mkdir(parents=True)
            (package / "code.rs").write_text("old source\n")
            (package / "Cargo.toml").write_text("version 1.25.2\n")
            (package / "README.md").write_text("generated old version\n")
            (tree / "upstream.json").write_text(json.dumps({"commit": "old", "packages": {"gpui": "gpui/crates/gpui"}}))
        git("init", "--quiet", "-b", "main", cwd=self.root)
        git("config", "user.name", "Test", cwd=self.root)
        git("config", "user.email", "test@example.invalid", cwd=self.root)
        git("add", ".", cwd=self.root)
        git("commit", "--quiet", "-m", "Initial", cwd=self.root)
        self.code = "gpui/crates/gpui/code.rs"

    def plan(self):
        return compare(self.root, self.before, self.incoming, "1.25.3")

    def test_downstream_only_difference_is_explicit_manual_review(self):
        (self.root / self.code).write_text("our patch\n")
        plan = self.plan()
        self.assertEqual(len(plan["entries"]), 1)
        entry = plan["entries"][0]
        self.assertEqual(entry["kind"], "code")
        self.assertFalse(entry["upstream_changed"])
        self.assertTrue(entry["downstream_overlap"])
        self.assertTrue(entry["retain_local"])
        # Preparing the report never replaces our patched working tree.
        self.assertEqual((self.root / self.code).read_text(), "our patch\n")
        head = git("rev-parse", "HEAD", cwd=self.root)
        apply(self.root, self.incoming, plan, "code")
        self.assertEqual((self.root / self.code).read_text(), "our patch\n")
        self.assertEqual(git("rev-parse", "main", cwd=self.root), head)

    def test_downstream_only_manifests_and_metadata_are_retained(self):
        paths = ("gpui/crates/gpui/Cargo.toml", "gpui/crates/gpui/README.md")
        for name in paths:
            (self.root / name).write_text("downstream identity and documentation\n")
        plan = self.plan()
        self.assertEqual({e["kind"] for e in plan["entries"]}, {"cargo", "metadata"})
        self.assertTrue(all(e["retain_local"] for e in plan["entries"]))
        for kind in ("code", "cargo", "metadata"):
            apply(self.root, self.incoming, plan, kind)
        for name in paths:
            self.assertEqual((self.root / name).read_text(), "downstream identity and documentation\n")

    def test_prepare_stages_current_policies_in_both_captures(self):
        policies = {
            POLICIES[0]: {"helper": "helper-gpui-unofficial"},
            POLICIES[1]: {"version": "2.0.0", "packages": {"gpui": "xgpui"}},
        }
        for name, data in policies.items():
            path = self.root / name
            path.parent.mkdir(exist_ok=True)
            path.write_text(json.dumps(data))
        snapshot = self.base / "snapshot"
        observed = []
        run = subprocess.run

        def normalize(command, **kwargs):
            if command[0] != str(self.base / "normalizer"):
                return run(command, **kwargs)
            tree = pathlib.Path(command[command.index("--root") + 1])
            for name in POLICIES:
                self.assertEqual((tree / name).read_bytes(), (self.root / name).read_bytes())
            observed.append(tree)

        def capture_fixture(tree, source, revision, release, expected_sha):
            # The captures can contain an older policy; it must be replaced.
            for name in POLICIES:
                path = tree / name
                path.parent.mkdir(exist_ok=True)
                path.write_text("stale upstream policy")
            (tree / "gpui/crates").mkdir(parents=True, exist_ok=True)
            (tree / "upstream.json").write_text(json.dumps({
                "commit": "a" * 40, "zed_commit": "b" * 40, "packages": {"gpui": "gpui/crates/gpui"}}))
            (tree / "gpui/crates/transform-metadata.json").write_text(json.dumps({
                "zed_commit": "b" * 40, "crates": ["gpui"]}))

        provenance = self.root / "upstream.json"
        data = json.loads(provenance.read_text())
        data["ref"] = "release/1.25.2"
        provenance.write_text(json.dumps(data))
        with patch("release_review.capture", side_effect=capture_fixture), \
                patch("release_review.subprocess.run", side_effect=normalize):
            report = prepare(self.root, snapshot, self.base / "report.json", self.base, "HEAD",
                             "1.26.2", self.base / "normalizer", self.base / "map.json")
        self.assertEqual(len(observed), 4)  # normalize and check, for each snapshot
        self.assertEqual(len(set(observed)), 2)
        self.assertEqual(report["zed_context"]["paths"], ["crates/gpui"])
        self.assertFalse(any(e["path"].startswith("scripts/") for e in report["entries"]))

    def test_missing_policy_fails_before_creating_snapshot(self):
        snapshot = self.base / "snapshot"
        with self.assertRaises(FileNotFoundError):
            prepare(self.root, snapshot, self.base / "report.json", self.base, "HEAD",
                    "1.26.2", self.base / "normalizer", self.base / "map.json")
        self.assertFalse(snapshot.exists())

    def test_source_cargo_and_generated_metadata_apply_separately(self):
        (self.incoming / self.code).write_text("new upstream source\n")
        manifest = self.incoming / "gpui/crates/gpui/Cargo.toml"
        manifest.write_text("version 1.25.3\n")
        (self.incoming / "gpui/crates/gpui/README.md").write_text("generated new version\n")
        mirror = self.incoming / "gpui/crates/transformed-crates/gpui/code.rs"
        mirror.parent.mkdir(parents=True)
        mirror.write_text("duplicate generation artifact\n")
        plan = self.plan()
        apply(self.root, self.incoming, plan, "code")
        self.assertEqual((self.root / self.code).read_text(), "new upstream source\n")
        self.assertEqual((self.root / manifest.relative_to(self.incoming)).read_text(), "version 1.25.2\n")
        self.assertFalse((self.root / mirror.relative_to(self.incoming)).exists())
        apply(self.root, self.incoming, plan, "metadata")
        self.assertEqual((self.root / manifest.relative_to(self.incoming)).read_text(), "version 1.25.2\n")
        apply(self.root, self.incoming, plan, "cargo")
        self.assertEqual((self.root / manifest.relative_to(self.incoming)).read_text(), "version 1.25.3\n")
        self.assertFalse((self.root / mirror.relative_to(self.incoming)).exists())
        self.assertEqual(self.plan()["entries"], [])

    def test_legacy_staging_is_deleted_even_when_incoming_has_a_copy(self):
        relative = "gpui/crates/transformed-crates/gpui/code.rs"
        for tree in (self.root, self.incoming):
            path = tree / relative
            path.parent.mkdir(parents=True)
            path.write_text("legacy mirror")
        git("add", ".", cwd=self.root)
        git("commit", "--quiet", "-m", "Legacy staging", cwd=self.root)
        plan = self.plan()
        self.assertEqual([(e["path"], e["change"]) for e in plan["entries"]], [(relative, "deleted")])
        apply(self.root, self.incoming, plan, "code")
        apply(self.root, self.incoming, plan, "metadata")
        self.assertFalse((self.root / relative).exists())
        self.assertEqual(self.plan()["entries"], [])

    def test_mirrored_cargo_requirements_are_not_labeled_metadata(self):
        self.assertEqual(classify("gpui/crates/transformed-crates/gpui/Cargo.toml", set()), "cargo")
        self.assertEqual(classify(".cargo/config.toml", set()), "cargo")

    def test_additions_and_deletions_are_proposed_not_lost(self):
        (self.incoming / self.code).unlink()
        added = self.incoming / "gpui/crates/gpui/new.metal"
        added.write_text("shader source\n")
        plan = self.plan()
        self.assertEqual({e["change"] for e in plan["entries"]}, {"added", "deleted"})
        apply(self.root, self.incoming, plan, "code")
        self.assertFalse((self.root / self.code).exists())
        self.assertTrue((self.root / added.relative_to(self.incoming)).exists())

    def test_stale_plan_refuses_to_overwrite_new_work(self):
        (self.incoming / self.code).write_text("incoming\n")
        plan = self.plan()
        (self.root / self.code).write_text("work after review\n")
        with self.assertRaises(ValueError):
            apply(self.root, self.incoming, plan, "code")
        self.assertEqual((self.root / self.code).read_text(), "work after review\n")

    def test_non_import_files_are_not_touched(self):
        local = self.root / "local.rs"
        local.write_text("local app\n")
        self.assertEqual(self.plan()["entries"], [])
        self.assertEqual(local.read_text(), "local app\n")


if __name__ == "__main__":
    unittest.main()
