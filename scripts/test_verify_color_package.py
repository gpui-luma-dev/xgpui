import io
from pathlib import Path
import tarfile
import tempfile
import unittest

from verify_color_package import inspect_archive


class ArchiveValidationTests(unittest.TestCase):
    def make_fixture(self, root, missing=None, changed=None, extra=None):
        source = root / "source"
        source.mkdir()
        contents = {
            "README.md": "# Color\n",
            "LICENSE-APACHE": "license fixture\n",
            "src/math.wgsl": "fn color() {}\n",
            "src/lib.rs": "pub fn color() {}\n",
            "Cargo.toml": '[package]\nname="xgpui-color"\nversion="0.2.0"\npublish=false\n',
        }
        for name, text in contents.items():
            path = source / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text)
        archived = {**contents, "Cargo.toml.orig": contents["Cargo.toml"]}
        if missing:
            del archived[missing]
        if changed:
            archived[changed] = "changed"
        archive = root / "color.crate"
        with tarfile.open(archive, "w:gz") as bundle:
            for name, text in archived.items():
                entry = tarfile.TarInfo("xgpui-color-0.2.0/" + name)
                data = text.encode()
                entry.size = len(data)
                bundle.addfile(entry, io.BytesIO(data))
            if extra:
                entry = tarfile.TarInfo(extra)
                bundle.addfile(entry, io.BytesIO(b""))
        return archive, source

    def test_accepts_matching_archive_and_license_shader_assets(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archive, source = self.make_fixture(root)
            extracted, files = inspect_archive(archive, source, root / "unpacked", "xgpui-color-0.2.0")
            self.assertTrue((extracted / "LICENSE-APACHE").is_file())
            self.assertIn("xgpui-color-0.2.0/src/math.wgsl", files)

    def test_rejects_missing_or_changed_shader(self):
        for options in ({"missing": "src/math.wgsl"}, {"changed": "src/math.wgsl"}):
            with self.subTest(options=options), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                archive, source = self.make_fixture(root, **options)
                with self.assertRaises(ValueError):
                    inspect_archive(archive, source, root / "unpacked", "xgpui-color-0.2.0")

    def test_rejects_archive_escape_before_extraction(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archive, source = self.make_fixture(root, extra="xgpui-color-0.2.0/../../escape")
            with self.assertRaises(ValueError):
                inspect_archive(archive, source, root / "unpacked", "xgpui-color-0.2.0")
            self.assertFalse((root / "unpacked").exists())


if __name__ == "__main__":
    unittest.main()
