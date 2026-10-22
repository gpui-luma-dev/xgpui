"""Guard the standalone workspace boundaries used by the build harness."""

import pathlib
import tempfile
import tomllib
import unittest

from update_build import manifests


class BuildManifestTests(unittest.TestCase):
    def test_excludes_each_workspace_and_ignores_nested_staging_copy(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            for name in ("gpui-unofficial", "gpui-platform-gpui-unofficial", "gpui-macos-gpui-unofficial"):
                package = root / "gpui/crates" / name
                package.mkdir(parents=True)
                (package / "Cargo.toml").write_text(
                    f'[package]\nname = "{name}"\nversion = "1.25.2"\n[workspace]\n'
                )
            application = root / "apps/lifecycle"
            application.mkdir(parents=True)
            (application / "Cargo.toml").write_text('[package]\nname = "lifecycle"\nversion = "0.0.0"\n')
            staging = root / "gpui/crates/transformed-crates/duplicate"
            staging.mkdir(parents=True)
            (staging / "Cargo.toml").write_text("This staging copy is not a package input")
            config = root / ".cargo/validation.toml"
            config.parent.mkdir()
            config.write_text('[patch.crates-io]\nproptest = { git = "example", rev = "revision" }\n')
            before = (root / "gpui/crates/gpui-unofficial/Cargo.toml").read_bytes()
            generated = manifests(root)
            workspace = tomllib.loads(generated["Cargo.toml"])
            harness = tomllib.loads(generated["tools/build-all/Cargo.toml"])
            self.assertEqual(workspace["workspace"]["members"], ["tools/build-all", "apps/lifecycle"])
            self.assertEqual(set(workspace["workspace"]["exclude"]), {
                "gpui/crates/gpui-unofficial",
                "gpui/crates/gpui-macos-gpui-unofficial",
                "gpui/crates/gpui-platform-gpui-unofficial",
            })
            self.assertEqual(workspace["patch"]["crates-io"]["proptest"]["rev"], "revision")
            self.assertIn("gpui-unofficial", harness["dependencies"])
            macos = harness["target"]['cfg(target_os = "macos")']["dependencies"]
            self.assertIn("gpui-macos-gpui-unofficial", macos)
            self.assertEqual(before, (root / "gpui/crates/gpui-unofficial/Cargo.toml").read_bytes())

    def test_missing_capture_fails(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(ValueError, "No captured packages"):
                manifests(pathlib.Path(directory))


if __name__ == "__main__":
    unittest.main()
