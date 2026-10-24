"""Pinned release context covers the captured source inventory, not gpui* only."""
import json
import pathlib
import tempfile
import unittest

from release_context import REPOSITORY, collect, context_inputs, markdown

BASE, HEAD = "a" * 40, "b" * 40


class ContextTests(unittest.TestCase):
    def test_exact_shas_and_union_include_nested_and_supporting_crates(self):
        with tempfile.TemporaryDirectory() as temporary:
            roots = [pathlib.Path(temporary) / name for name in ("old", "new")]
            for root, sha, crates in [(roots[0], BASE, ["gpui", "refineable/derive_refineable"]),
                                      (roots[1], HEAD, ["gpui", "http_client_tls", "tooling/perf"])]:
                (root / "gpui/crates").mkdir(parents=True)
                (root / "upstream.json").write_text(json.dumps({"zed_commit": sha, "packages": {c: c for c in crates}}))
                (root / "gpui/crates/transform-metadata.json").write_text(json.dumps({"zed_commit": sha, "crates": crates}))
            inputs = context_inputs(*roots)
            self.assertEqual(inputs["base_commit"], BASE)
            self.assertEqual(inputs["head_commit"], HEAD)
            self.assertEqual(inputs["paths"], ["crates/gpui", "crates/http_client_tls", "crates/refineable/derive_refineable", "crates/tooling/perf"])
            metadata = roots[1] / "gpui/crates/transform-metadata.json"
            metadata.write_text(json.dumps({"zed_commit": BASE, "crates": ["gpui"]}))
            with self.assertRaises(ValueError):
                context_inputs(*roots)

    def test_paginated_range_and_files_are_scoped_without_losing_shared_dependencies(self):
        inputs = {"repository": REPOSITORY, "base_commit": BASE, "head_commit": HEAD,
                  "paths": ["crates/gpui", "crates/http_client_tls", "crates/tooling/perf"]}
        commits = [{"sha": str(i) * 40, "commit": {"message": "Change (#123)\nDetails"}} for i in (1, 2, 3)]
        calls = []

        def api(endpoint):
            calls.append(endpoint)
            if "/compare/" in endpoint:
                return [{"status": "ahead", "total_commits": 3, "commits": commits[:2]},
                        {"commits": commits[2:]}]
            if commits[0]["sha"] in endpoint:
                return [{"files": [{"filename": "crates/http_client_tls/src/lib.rs"}]},
                        {"files": [{"filename": "crates/tooling/perf/src/lib.rs"}]}]
            if commits[1]["sha"] in endpoint:
                return [{"files": [{"filename": "Cargo.toml"}, {"filename": "Cargo.lock"}]}]
            return [{"files": [{"filename": "crates/gpui_unrelated/source.rs"}]}]

        context = collect(inputs, api)
        self.assertIn(BASE + "..." + HEAD, calls[0])
        self.assertEqual(len(context["commits"][0]["captured_paths"]), 2)
        self.assertEqual(context["commits"][1]["workspace_paths"], ["Cargo.lock", "Cargo.toml"])
        self.assertEqual(context["commits"][2]["captured_paths"], [])
        text = markdown(context)
        self.assertIn("/pull/123", text)
        self.assertIn("Shared workspace dependency context", text)
        self.assertIn("Other commits in the exact range", text)

    def test_removed_or_renamed_source_is_included(self):
        inputs = {"repository": REPOSITORY, "base_commit": BASE, "head_commit": HEAD, "paths": ["crates/http_client_tls"]}
        def api(endpoint):
            if "/compare/" in endpoint:
                return [{"status": "ahead", "total_commits": 1, "commits": [{"sha": HEAD, "commit": {"message": "Move crate"}}]}]
            return [{"files": [{"filename": "crates/new_tls/lib.rs", "previous_filename": "crates/http_client_tls/lib.rs"}]}]
        self.assertEqual(collect(inputs, api)["commits"][0]["captured_paths"], ["crates/http_client_tls/lib.rs"])

    def test_incomplete_or_divergent_context_is_not_a_successful_empty_report(self):
        inputs = {"repository": REPOSITORY, "base_commit": BASE, "head_commit": HEAD, "paths": ["crates/gpui"]}
        for page in [{"status": "ahead", "total_commits": 1, "commits": []},
                     {"status": "diverged", "total_commits": 0, "commits": []}]:
            with self.assertRaises(ValueError):
                collect(inputs, lambda _: [page])


if __name__ == "__main__":
    unittest.main()
