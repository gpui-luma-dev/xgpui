# Release procedure

Daily recipe: **prepare → review → merge → tag and draft → publish and verify
crates → publish the release record → deploy Pages**.

The tag identifies the exact approved source. A draft GitHub release records the
intended release; a published GitHub release records completed publication.
Create the tag and draft **before the first crates.io upload** so a registry
outage or partial publication does not lose the source reference or release notes.

## 1. Prepare versions and release notes

Start from current `main`. Compare with the last **successfully completed**
release, including code, packaged READMEs, dependencies and publication changes.

```sh
gh workflow run prepare-daily-release.yml --ref main
```

Preparation opens a PR with proposed versions, updates internal pins and lockfiles,
and regenerates manifests. Review the proposals individually; automatic patch
bumps are suggestions. Use the project's agreed release policy and explain any
consumer-visible API changes. Docs outside crate archives do not alone require
crate version bumps. The authoritative map is `scripts/release-packages.json`.

Prepare the release notes in the PR, including:

- Change titles, short descriptions and links to relevant commits or PRs.
- The old/new crate version table and versions expected to be verified and skipped.
- API migration notes, supported features and known validation limits.
- Space for the publication report and final deployment links.

Run the relevant tooling, package, native and documentation checks. Run a no-upload
registry plan on the PR branch and resolve version conflicts before merging.
If broad validation is intentionally disabled, preparation dispatches registry
planning instead; this does not establish full native backend coverage.

If an unfinished release already has a tag, specify the last completed baseline
explicitly. The current preparation script otherwise selects the latest ancestor
tag, which might be an incomplete release:

```sh
gh workflow run prepare-daily-release.yml --ref main \
  -f baseline=LAST_COMPLETED_RELEASE_TAG
```

## 2. Approve and merge

Obtain explicit user approval for the specific release PR before merging. Version
approval is not merge approval. After merging, record the full merge commit SHA
and rerun the registry plan for that source. Confirm that `main` has not advanced
when dispatching a plan on `main`:

```sh
gh workflow run prepare-registry-release.yml --ref main -f mode=plan
```

Keep the plan report with the release record. Unchanged published versions must
match their registry payloads; never overwrite or reuse a version with different
content. Preserve unrelated local edits when preparing or merging a release.

## 3. Create the source tag and draft release before uploading

Use `release-YYYY-MM-DD-COMMIT`, where the date is the source commit date in UTC
and `COMMIT` is its first 12 hex characters. This matches the current finalizer's
identity and remains stable when retrying on another day.

Fetch the approved commit locally, then set the full SHA and the path to the
reviewed Markdown notes. Keep a copy of those notes outside generated reports:

```sh
release_sha=FULL_APPROVED_MERGE_COMMIT_SHA
release_notes=/absolute/path/to/reviewed-release-notes.md
release_tag=$(python3 - "$release_sha" <<'PY'
from datetime import datetime, timezone
import subprocess, sys
sha = subprocess.check_output(['git', 'rev-parse', sys.argv[1] + '^{commit}'], text=True).strip()
stamp = subprocess.check_output(['git', 'show', '-s', '--format=%cI', sha], text=True).strip()
date = datetime.fromisoformat(stamp).astimezone(timezone.utc).strftime('%Y-%m-%d')
print(f'release-{date}-{sha[:12]}')
PY
)
```

Inspect existing tags/releases before creation. If this identity already exists,
verify its tag resolves to the same full commit and reuse it. Never move a release
tag after any crate has been uploaded. For a new release:

```sh
git tag -a "$release_tag" "$release_sha" -m "Release source $release_tag"
git push origin "refs/tags/$release_tag"
gh release create "$release_tag" --verify-tag --draft \
  --title "XGPUI $release_tag" --notes-file "$release_notes"
```

Do not proceed until the remote tag points to the approved source and the draft
contains the reviewed notes. Tag creation and drafting are separate from an
approval to publish crates.

## 4. Publish the exact tagged commit

Obtain explicit user approval for crate publication. Dispatch the publisher using
the tag so later changes to `main` cannot change the source:

```sh
gh workflow run prepare-registry-release.yml --ref "$release_tag" \
  -f mode=publish -f "confirmation=publish $release_sha"
```

The workflow verifies package candidates, uploads in dependency order, verifies
registry contents after each upload, and skips matching existing versions.
Credentials are limited to the publication step. Monitor the run through completion
and retain its report, checksums and logs.

A complete release requires every mapped crate to be either `published-verified`
or `already-published`, with `release_ready: true` and the expected source SHA.
Confirm a registry-only consumer resolves the intended versions without repository
patches, builds and runs. Record runtime coverage and limitations accurately.

## 5. Publish the GitHub release record

After all crates are verified, add the results and workflow link to the reviewed
notes, retain the change highlights and version table, and publish the draft:

```sh
gh release edit "$release_tag" --notes-file "$release_notes" --draft=false
```

Verify that the published record and tag still identify the exact release commit.
The tag alone does not mean that publication completed.

## 6. Deploy documentation last

Pages is **manual only**. After successful crate publication, deploy the approved
website revision so documentation does not announce unavailable crate versions:

```sh
gh workflow run pages.yml --ref main
```

Verify the deployed commit, workflow success, `https://xgpui.dev/`, documented
crate versions, the demo galleries and both Wasm bundles. Record the deployment
link in the release notes. A release PR merge does not deploy Pages.

## Recover from partial publication

Keep the original tag, draft, source SHA and reviewed notes. Do not mark the release
complete or deploy Pages while uploads remain unverified.

Rerun the same workflow run on the same commit. Verified existing versions are
skipped and missing versions resume. If a registry version differs from the
candidate, stop and investigate; published crate contents are immutable. A source
fix requires a new reviewed commit and appropriate new versions, not moving the
existing tag. Explain any abandoned or superseded partial release in its record.

If only GitHub bookkeeping failed, recover the record after checking the publication
report; do not bump versions just to recreate notes or a release entry.

## Current automation boundary

The **tag-and-draft-before-upload sequence above is the agreed procedure**. It is
not yet fully implemented by the workflows:

- Preparation currently generates a version table, not reviewed change highlights.
- The publisher's finalizer currently creates the tag/release **after** uploads.
- If a matching draft already exists, that finalizer edits it, replacing its notes
  with a generated version table; it does not publish the draft.

Until automation is updated, perform step 3 manually, preserve the reviewed notes,
and restore them with verification results in step 5 before publishing the draft.
Future automation should enforce pre-upload tag/draft creation, preserve reviewed
notes, and use only completed releases as preparation baselines.

The initial completed baseline is `release-2026-10-30`. The next completed release
is `release-2026-10-31-ed4469267251`.
