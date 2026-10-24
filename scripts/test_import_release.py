"""Regression tests for snapshot replacement and provenance."""

import json
import pathlib
import subprocess
import sys
import tempfile
import unittest

from import_release import capture


class ImportTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.base = pathlib.Path(self.temporary.name)
        self.source = self.base / "source"
        self.root = self.base / "destination"
        self.source.mkdir()
        self.root.mkdir()
        self.run_git("init")
        self.run_git("config", "user.name", "Import test")
        self.run_git("config", "user.email", "test@example.invalid")
        package = self.source / "crates/gpui-unofficial"
        package.mkdir(parents=True)
        (package / "Cargo.toml").write_text(
            '[package]\nname = "gpui-unofficial"\nversion = "1.25.2"\n'
        )
        (package / "LICENSE-APACHE").write_text("license fixture\n")
        (package / "old.rs").write_text("old\n")
        self.commit()

    def run_git(self, *args):
        return subprocess.check_output(["git", *args], cwd=self.source, text=True).strip()

    def commit(self):
        self.run_git("add", "--all")
        self.run_git("commit", "-m", "Fixture")

    def capture(self, expected=None):
        return capture(self.root, self.source, "HEAD", "1.25.2", expected)

    def test_replacement_preserves_development_files_and_deletes_old_sources(self):
        docs = self.root / "docs"
        docs.mkdir()
        (docs / "guide.md").write_text("local documentation\n")
        self.capture()
        package = self.source / "crates/gpui-unofficial"
        (package / "old.rs").unlink()
        (package / "new.rs").write_text("new\n")
        self.commit()
        self.capture()
        self.assertFalse((self.root / "gpui/crates/gpui-unofficial/old.rs").exists())
        self.assertTrue((self.root / "gpui/crates/gpui-unofficial/new.rs").exists())
        self.assertEqual((docs / "guide.md").read_text(), "local documentation\n")
        self.assertEqual(
            (self.root / "gpui/crates/gpui-unofficial/LICENSE-APACHE").read_text(),
            "license fixture\n",
        )

    def test_import_is_repeatable_and_records_commit(self):
        self.capture()
        before = (self.root / "upstream.json").read_bytes()
        self.capture()
        self.assertEqual(before, (self.root / "upstream.json").read_bytes())
        self.assertEqual(json.loads(before)["commit"], self.run_git("rev-parse", "HEAD"))
        self.assertIn('path = "gpui/crates/gpui-unofficial"',
                      (self.root / ".cargo/config.toml").read_text())

    def test_wrong_commit_does_not_modify_existing_import(self):
        self.capture()
        before = (self.root / "upstream.json").read_bytes()
        with self.assertRaises(ValueError):
            self.capture("0" * 40)
        self.assertEqual(before, (self.root / "upstream.json").read_bytes())

    def test_bad_snapshot_does_not_replace_existing_import(self):
        self.capture()
        before = (self.root / "upstream.json").read_bytes()
        (self.source / "crates/gpui-unofficial/Cargo.toml").unlink()
        self.commit()
        with self.assertRaises(ValueError):
            self.capture()
        self.assertEqual(before, (self.root / "upstream.json").read_bytes())

    def test_cli_imports_only_on_the_matching_version_branch(self):
        subprocess.run(["git", "init", "-b", "main"], cwd=self.root, check=True,
                       capture_output=True)
        command = [sys.executable, str(pathlib.Path(__file__).with_name("import_release.py").resolve()),
                   "1.25.2", "--source", str(self.source)]
        for branch in ["main", "release/1.25.1"]:
            subprocess.run(["git", "symbolic-ref", "HEAD", f"refs/heads/{branch}"],
                           cwd=self.root, check=True)
            result = subprocess.run(command, cwd=self.root, text=True, capture_output=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("Imports must run on the release/1.25.2 branch", result.stderr)
            self.assertFalse((self.root / "gpui").exists())
        subprocess.run(["git", "symbolic-ref", "HEAD", "refs/heads/release/1.25.2"],
                       cwd=self.root, check=True)
        result = subprocess.run(command, cwd=self.root, text=True, capture_output=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout)["ref"], "release/1.25.2")

    def test_orphan_source_release_merges_without_removing_downstream_patch(self):
        def local_git(*args):
            return subprocess.check_output(["git", *args], cwd=self.root, text=True).strip()

        local_git("init", "-b", "main")
        local_git("config", "user.name", "Import test")
        local_git("config", "user.email", "test@example.invalid")
        self.capture()
        local_git("add", ".")
        local_git("commit", "-m", "Initial release snapshot")
        baseline = local_git("rev-parse", "HEAD")
        patched = self.root / "gpui/crates/gpui-unofficial/old.rs"
        patched.write_text("downstream patch\n")
        local_git("add", ".")
        local_git("commit", "-m", "Downstream development")

        # Upstream publishes a new snapshot with independent Git ancestry.
        self.run_git("checkout", "--orphan", "release/1.25.3")
        package = self.source / "crates/gpui-unofficial"
        manifest = package / "Cargo.toml"
        manifest.write_text(manifest.read_text().replace("1.25.2", "1.25.3"))
        (package / "new.rs").write_text("incoming source\n")
        self.commit()
        self.assertEqual(len(self.run_git("rev-list", "HEAD").splitlines()), 1)

        local_git("switch", "-c", "release/1.25.3", baseline)
        capture(self.root, self.source, "HEAD", "1.25.3")
        local_git("add", ".")
        local_git("commit", "-m", "Incoming release differences")
        # The incoming branch contains only upstream changes.
        self.assertEqual(patched.read_text(), "old\n")
        local_git("switch", "main")
        local_git("merge", "--no-edit", "release/1.25.3")
        self.assertEqual(patched.read_text(), "downstream patch\n")
        self.assertEqual((patched.parent / "new.rs").read_text(), "incoming source\n")


if __name__ == "__main__":
    unittest.main()
