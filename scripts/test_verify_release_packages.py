import copy
import io
from pathlib import Path
import tarfile
import tempfile
import tomllib
import unittest

from verify_release_packages import dump_toml, inspect_archive, rewire_candidates


class ReleasePackageValidationTests(unittest.TestCase):
    def test_manifest_round_trip_preserves_cfg_features_and_example_tables(self):
        document = tomllib.loads('''[package]
name = "fixture"
version = "1.0.0"
publish = false
[features]
default = ["dep:optional"]
[target.'cfg(target_os = "macos")'.dependencies]
optional = { version = "1", optional = true, features = ["a"] }
[[example]]
name = "first"
path = "examples/first.rs"
[[example]]
name = "second"
path = "examples/second.rs"
''')
        self.assertEqual(tomllib.loads(dump_toml(document)), document)

    def test_candidate_rewiring_keeps_versions_features_and_external_sources(self):
        document = {"target": {"cfg(windows)": {"dependencies": {
            "alias": {"package": "candidate", "version": "=1.0.0", "optional": True, "features": ["wide"]},
            "external": {"version": "2", "git": "https://example.test/source"},
        }}}}
        original = copy.deepcopy(document)
        rewrites = rewire_candidates(document, {"candidate": Path("/tmp/archive-candidate")})
        deps = document["target"]["cfg(windows)"]["dependencies"]
        self.assertEqual(deps["external"], original["target"]["cfg(windows)"]["dependencies"]["external"])
        self.assertEqual(deps["alias"]["version"], "=1.0.0")
        self.assertEqual(deps["alias"]["features"], ["wide"])
        self.assertTrue(deps["alias"]["optional"])
        self.assertEqual(deps["alias"]["path"], "/tmp/archive-candidate")
        self.assertEqual(rewrites[0]["alias"], "alias")

    def test_nested_package_inputs_are_excluded_but_runtime_assets_are_required(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            staged = root / "source"
            contents = {
                "Cargo.toml": '[package]\nname="fixture"\nversion="1.0.0"\n',
                "README.md": "# fixture\n", "LICENSE-APACHE": "license\n",
                "src/lib.rs": 'pub const SHADER: &str = include_str!("math.wgsl");',
                "src/math.wgsl": "fn math() {}",
                "examples/app/Cargo.toml": '[package]\nname="nested"\nversion="1.0.0"\n',
                "examples/app/LICENSE-APACHE": "nested license",
                "examples/app/src/main.rs": "fn main() {}",
            }
            for name, text in contents.items():
                path = staged / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(text)
            for omit_shader in (False, True):
                archive = root / f"fixture-{omit_shader}.crate"
                with tarfile.open(archive, "w:gz") as bundle:
                    for name, text in {**contents, "Cargo.toml.orig": contents["Cargo.toml"]}.items():
                        if name.startswith("examples/") or (omit_shader and name.endswith(".wgsl")):
                            continue
                        entry = tarfile.TarInfo("fixture-1.0.0/" + name)
                        data = text.encode()
                        entry.size = len(data)
                        bundle.addfile(entry, io.BytesIO(data))
                if omit_shader:
                    with self.assertRaisesRegex(ValueError, "math.wgsl"):
                        inspect_archive(archive, staged, root / "missing", "fixture-1.0.0")
                else:
                    extracted, files = inspect_archive(archive, staged, root / "valid", "fixture-1.0.0")
                    self.assertTrue((extracted / "src/math.wgsl").exists())
                    self.assertFalse(any("examples/app" in name for name in files))


if __name__ == "__main__":
    unittest.main()
