#!/usr/bin/env python3
"""Read-only Zed release context from pinned capture provenance and crate paths."""
import argparse
import json
import pathlib
import re
import subprocess

REPOSITORY = "zed-industries/zed"
WORKSPACE_FILES = {"Cargo.toml", "Cargo.lock"}


def context_inputs(before, incoming):
    captures = []
    paths = set()
    for root in (before, incoming):
        provenance = json.loads((root / "upstream.json").read_text())
        metadata = json.loads((root / "gpui/crates/transform-metadata.json").read_text())
        sha = provenance.get("zed_commit", "")
        if not re.fullmatch(r"[0-9a-f]{40}", sha) or metadata.get("zed_commit") != sha:
            raise ValueError("Missing or inconsistent exact Zed SHA in capture provenance")
        crates = metadata.get("crates", [])
        if not crates or len(set(crates)) != len(provenance["packages"]):
            raise ValueError("Zed crate inventory does not cover the captured packages")
        for crate in crates:
            if not isinstance(crate, str) or not crate or crate.startswith("/") or ".." in pathlib.PurePosixPath(crate).parts:
                raise ValueError("Unsafe Zed crate path")
            paths.add("crates/" + crate)
        captures.append(sha)
    return {"repository": REPOSITORY, "base_commit": captures[0], "head_commit": captures[1],
            "paths": sorted(paths)}


def api_pages(endpoint):
    return json.loads(subprocess.check_output(
        ["gh", "api", "--paginate", "--slurp", endpoint], text=True))


def collect(inputs, api=api_pages):
    base, head = inputs["base_commit"], inputs["head_commit"]
    if inputs["repository"] != REPOSITORY or not all(re.fullmatch(r"[0-9a-f]{40}", sha) for sha in (base, head)):
        raise ValueError("Context requires pinned Zed commit SHAs")
    pages = api(f"repos/{REPOSITORY}/compare/{base}...{head}?per_page=100")
    if not pages or pages[0]["status"] not in ("ahead", "identical"):
        raise ValueError("Zed commits do not form a forward release range")
    commits = {commit["sha"]: commit for page in pages for commit in page["commits"]}
    if len(commits) != pages[0]["total_commits"]:
        raise ValueError("Incomplete Zed commit pagination")
    entries = []
    for sha, commit in commits.items():
        files = [entry for page in api(f"repos/{REPOSITORY}/commits/{sha}?per_page=100") for entry in page["files"]]
        if len(files) >= 3000:
            raise ValueError("Zed commit may exceed API file limit; inspect with Git instead")
        # Include both paths for renames, so removed/renamed crates stay covered.
        names = {name for entry in files for name in (entry["filename"], entry.get("previous_filename")) if name}
        captured = sorted(name for name in names if any(name == path or name.startswith(path + "/") for path in inputs["paths"]))
        shared = sorted(names & WORKSPACE_FILES)
        subject = commit["commit"]["message"].splitlines()[0]
        entries.append({"sha": sha, "subject": subject, "captured_paths": captured, "workspace_paths": shared})
    return {**inputs, "commits": entries}


def markdown(context):
    url = "https://github.com/" + REPOSITORY
    base, head = context["base_commit"], context["head_commit"]
    lines = ["## Zed release context", "",
             f"Exact provenance range: [`{base}` → `{head}`]({url}/compare/{base}...{head}).", "",
             f"Scope: **{len(context['paths'])} captured crate paths**, union of both release inventories; shared root Cargo files are reported separately.", "",
             "This is upstream context, not a claim that every Zed change is present in the captured crates.", ""]
    for title, selected in (
        ("Captured-crate commits", [c for c in context["commits"] if c["captured_paths"]]),
        ("Shared workspace dependency context", [c for c in context["commits"] if c["workspace_paths"]]),
        ("Other commits in the exact range", [c for c in context["commits"] if not c["captured_paths"] and not c["workspace_paths"]]),
    ):
        lines += ["### " + title, ""]
        if not selected:
            lines += ["None.", ""]
        for commit in selected:
            subject = re.sub(r"#(\d+)", lambda m: f"[#{m[1]}]({url}/pull/{m[1]})", commit["subject"]).replace("@", "&#64;")
            lines.append(f"- [`{commit['sha'][:10]}`]({url}/commit/{commit['sha']}) — {subject}")
            paths = commit["captured_paths"] if title == "Captured-crate commits" else commit["workspace_paths"]
            if paths:
                lines.append("  - Relevant paths: " + ", ".join(f"`{p}`" for p in paths))
        lines.append("")
    lines += ["<details><summary>All captured Zed crate paths</summary>", ""]
    lines.extend("- `" + path + "`" for path in context["paths"])
    lines += ["", "</details>", ""]
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--report", type=pathlib.Path, required=True)
    parser.add_argument("--output", type=pathlib.Path, required=True)
    parser.add_argument("--json-output", type=pathlib.Path, required=True)
    args = parser.parse_args()
    inputs = json.loads(args.report.read_text())["zed_context"]
    context = collect(inputs)
    args.output.write_text(markdown(context))
    args.json_output.write_text(json.dumps(context, indent=2) + "\n")


if __name__ == "__main__":
    main()
