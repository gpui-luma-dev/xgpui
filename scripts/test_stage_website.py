from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from stage_website import main


class StageWebsiteTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / "scripts").mkdir()
        (self.root / "docs/examples/extensions").mkdir(parents=True)
        (self.root / "docs/index.html").write_text("documentation")
        (self.root / "docs/examples/extensions/stale.wasm").write_text("stale")
        self.file_patch = patch("stage_website.__file__", str(self.root / "scripts/stage_website.py"))
        self.file_patch.start()
        self.addCleanup(self.file_patch.stop)

    def run_stage(self, *args):
        with patch("sys.argv", ["stage_website.py", *args]):
            main()

    def test_documentation_preview_excludes_legacy_bundles(self):
        self.run_stage()
        self.assertEqual((self.root / "target/site/index.html").read_text(), "documentation")
        self.assertFalse((self.root / "target/site/examples/extensions").exists())

    def test_deployment_requires_both_bundles_before_replacing_site(self):
        previous = self.root / "target/site/index.html"
        previous.parent.mkdir(parents=True)
        previous.write_text("previous")
        with self.assertRaises(SystemExit):
            self.run_stage("--require-web")
        self.assertEqual(previous.read_text(), "previous")

    def test_deployment_preserves_routes_and_removes_stale_files(self):
        for name, module in [("extensions", "extensions_web"), ("shaders", "shader_examples")]:
            bundle = self.root / "target/web" / name
            bundle.mkdir(parents=True)
            (bundle / f"{module}_bg.wasm").write_bytes(b"wasm")
            (bundle / "index.html").write_text(name)
        self.run_stage("--require-web")
        for name in ("extensions", "shaders"):
            self.assertEqual((self.root / "target/site/examples" / name / "index.html").read_text(), name)
        self.assertFalse((self.root / "target/site/examples/extensions/stale.wasm").exists())


if __name__ == "__main__":
    unittest.main()
