"""Regression tests for snapshot replacement and provenance."""

import json
import pathlib
import subprocess
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


if __name__ == "__main__":
    unittest.main()
