# Release recipe

Run commands from the repository checkout with `git`, `gh`, Python 3 and Cargo installed. Authenticate `gh` and configure the repository's `CARGO_REGISTRY_TOKEN` secret before publishing.

## 1. Prepare the release PR

```sh
git fetch origin main --tags
gh release list --exclude-drafts --exclude-pre-releases

# Select the last successfully completed release.
baseline=LAST_COMPLETED_RELEASE_TAG
gh workflow run prepare-daily-release.yml --ref main -f "baseline=$baseline"
gh run list --workflow prepare-daily-release.yml --limit 1

prepare_run=PREPARATION_RUN_ID
gh run watch "$prepare_run" --exit-status
gh pr list --state open
```

Review the proposed versions, dependency pins and lockfiles. Prepare a Markdown notes file with change titles, summaries, links and the version table.

```sh
release_pr=RELEASE_PR_NUMBER
release_branch=RELEASE_PR_BRANCH
release_notes=/absolute/path/to/release-notes.md

gh pr diff "$release_pr"
gh pr checks "$release_pr"
gh pr edit "$release_pr" --body-file "$release_notes"
gh workflow run prepare-registry-release.yml --ref "$release_branch" -f mode=plan
```

Watch the plan run and inspect its report before merging:

```sh
gh run list --workflow prepare-registry-release.yml --branch "$release_branch" --limit 3
plan_run=PLAN_RUN_ID
gh run watch "$plan_run" --exit-status
gh run download "$plan_run" --dir "/tmp/release-plan-$plan_run"
```

## 2. Review and merge

After explicit merge approval, with a clean local checkout:

```sh
gh pr merge "$release_pr" --merge --delete-branch
release_sha=$(gh pr view "$release_pr" --json mergeCommit --jq .mergeCommit.oid)
git fetch origin main --tags
git show --no-patch "$release_sha"
```

## 3. Create the tag and release record

**Complete this before uploading any crates.** Generate the tag from the approved commit's UTC date and first 12 hex characters:

```sh
release_tag=$(python3 - "$release_sha" <<'PY'
from datetime import datetime, timezone
import subprocess, sys
sha = subprocess.check_output(['git', 'rev-parse', sys.argv[1] + '^{commit}'], text=True).strip()
stamp = subprocess.check_output(['git', 'show', '-s', '--format=%cI', sha], text=True).strip()
date = datetime.fromisoformat(stamp).astimezone(timezone.utc).strftime('%Y-%m-%d')
print(f'release-{date}-{sha[:12]}')
PY
)

git ls-remote origin "refs/tags/$release_tag" "refs/tags/$release_tag^{}"
gh release list --limit 10
```

For a new tag and record:

```sh
git tag -a "$release_tag" "$release_sha" -m "Release source $release_tag"
git push origin "refs/tags/$release_tag"
gh release create "$release_tag" --verify-tag --draft \
  --title "XGPUI $release_tag" --notes-file "$release_notes"

git ls-remote origin "refs/tags/$release_tag^{}"
gh release view "$release_tag"
```

Verify the tag resolves to `release_sha`. If the tag or draft already exists, verify and reuse it; do not recreate or move it.

## 4. Publish and verify crates

After explicit publication approval:

```sh
gh workflow run prepare-registry-release.yml --ref "$release_tag" \
  -f mode=publish -f "confirmation=publish $release_sha"
gh run list --workflow prepare-registry-release.yml --branch "$release_tag" --limit 1

publish_run=PUBLICATION_RUN_ID
gh run watch "$publish_run" --exit-status
gh run download "$publish_run" --dir "/tmp/release-publication-$publish_run"
```

Check the downloaded report:

```sh
python3 - "$publish_run" "$release_sha" <<'PY'
import json, pathlib, sys
report = next(pathlib.Path(f'/tmp/release-publication-{sys.argv[1]}').rglob('report.json'))
data = json.loads(report.read_text())
assert data['source_commit'] == sys.argv[2]
assert data['status'] == 'published-verified' and data['release_ready'] is True
for crate in data['packages']:
    assert crate['status'] in ('published-verified', 'already-published')
    print(crate['name'], crate['version'], crate['status'])
PY
```

Build and run an application outside the repository with exact released dependency versions and no repository patches:

```sh
consumer_manifest=/absolute/path/to/registry-only-consumer/Cargo.toml
cargo metadata --manifest-path "$consumer_manifest" --format-version 1
cargo run --locked --manifest-path "$consumer_manifest"
```

Confirm its resolved XGPUI packages come from crates.io. Add verification results and the workflow link to the preserved notes, then publish the existing draft:

```sh
gh release edit "$release_tag" --notes-file "$release_notes" --draft=false
gh release view "$release_tag"
```

**On failure:** retain the tag and draft. Retry the same run and commit, skipping verified uploads:

```sh
gh run rerun "$publish_run" --failed
gh run watch "$publish_run" --exit-status
```

Never move a tag after an upload. Keep the record in draft and hold docs deployment until verification succeeds. A source fix requires new reviewed versions and a new release source.

## 5. Deploy documentation

After successful crate publication and deployment approval:

```sh
gh workflow run pages.yml --ref main
gh run list --workflow pages.yml --limit 1

pages_run=PAGES_RUN_ID
gh run watch "$pages_run" --exit-status
curl -fI https://xgpui.dev/
curl -fI https://xgpui.dev/examples/extensions/extensions_web_bg.wasm
curl -fI https://xgpui.dev/examples/shaders/shader_examples_bg.wasm
```

Verify the deployed commit, documented versions and galleries. Add the deployment link to the release notes:

```sh
gh release edit "$release_tag" --notes-file "$release_notes"
```

## Automation requirements

Tag and release-record creation must precede uploads, and reviewed notes must be preserved. Until the workflows support this order, perform step 3 manually and restore the reviewed notes in step 4 if the finalizer replaces them.

Use the last **completed** release as the preparation baseline; exclude unfinished release tags. Select run IDs matching the intended source and event, rather than assuming the newest listed run is the correct one.
