# Releasing XGPUI

This is the authoritative release guide. Historical publication evidence is kept
in [the publication archive](release-reviews/publishing-history.md); release-specific
reviews live in [release-reviews/](release-reviews/).

This procedure is for the agent to execute. The user requests a release, reviews
its proposed versions and notes, and gives the approvals below. The agent runs
the commands, monitors workflows and handles failures.

## 1. Prepare — agent

When the user says **“prepare today's release”**:

- Fetch current `main` and identify the last successfully completed release.
- Compare code, packaged documentation, dependencies and publication changes.
- Run release preparation and open a PR with proposed old → new crate versions.
- Prepare release notes with change titles, short summaries, links and the version table.
- Review automatic version proposals against the versioning policy below. Include a reason for every bump in the PR.
- Run the validation and no-upload registry checks below; resolve failures and version conflicts. Record source identity, host, commands and results in the release review. Revalidate affected checks after source changes.
- For each public API addition or change in non-extension GPUI crates, document purpose, source/serialization compatibility, migration requirements and whether it could live in an extension. Present each concrete API change for final user review; implementation approval does not approve the final public API.
- Present the PR, proposed versions and validation results to the user.

**User:** review the proposal and approve or revise the version numbers. No commands or manual preparation are required.

## 2. Merge — agent, after approval

**User:** explicitly approve merging the release PR.

**Agent:** merge the approved PR, remove its branch, record the exact merged commit SHA and confirm the final registry plan applies to that source. Preserve unrelated local edits.

## 3. Create the tag and release record — agent

**Before any crates.io upload:**

- Tag the approved commit as `release-YYYY-MM-DD-COMMIT`, using its UTC commit date and first 12 hex characters.
- Support multiple releases per day: each distinct approved commit gets its own tag, for example `release-2026-10-31-ed4469267251` and `release-2026-10-31-a1b2c3d4e5f6`. Retries of the same release reuse its tag and record; do not create a new release identity for a retry.
- Create a draft GitHub release containing the reviewed notes and version table.
- Verify the remote tag points to the approved commit and present the draft to the user.

## 4. Publish and verify crates — agent, after approval

**User:** explicitly approve crate publication.

**Agent:**

- Publish the exact tagged commit in dependency order.
- Verify and skip matching versions already published; verify each new upload.
- Confirm all intended versions are verified and a registry-only consumer builds and runs.
- Add results and the workflow link to the existing release record, preserve its highlights, and publish the draft.
- Report completion to the user.

**On failure:** retain the tag and draft, diagnose the failure and retry the same commit where safe, skipping verified uploads. Never move a tag after an upload. A source fix requires a new reviewed release source and appropriate versions. Keep the record in draft and hold docs deployment until verification succeeds.

## 5. Deploy documentation — agent, after approval

**User:** approve documentation deployment.

**Agent:** manually dispatch Pages after successful crate publication, monitor it to completion, verify the live site, documented versions, galleries and Wasm bundles, and add the deployment link to the release record. Report the release complete.

## Versioning policy

Apply this policy during preparation, starting with the next release:

- **Patch:** compatible fixes, internal backend changes, packaged documentation and compatible dependency updates.
- **Minor:** new capabilities that preserve supported public APIs.
- **Major:** deliberate breaking changes to supported public APIs.
- **Before 1.0:** use a minor bump for breaking changes or new capabilities; use a patch for compatible fixes, internal changes and packaged docs. State whether a minor release contains breaking changes.
- **Dependent crates:** bump only when packaged content or dependency requirements change. Do not automatically propagate a major bump; review whether changed dependency types affect the crate's public API.
- **Internal requirements:** use full stable compatible requirements (for example,
  `3.0.0` or `0.4.0`) for XGPUI libraries. Preserve an existing minimum when it
  admits the candidate version; raise it when code needs a newer API or fix.
  Normalization and release preparation must not raise compatible minimums just
  because a dependency releases. Exact requirements remain appropriate in
  registry verification fixtures. Upstream helper pins remain unchanged.
  Release tools support full stable caret/default, tilde and exact requirements;
  other syntax requires explicit review.
- **Compatibility review:** inspect removed or renamed APIs, signatures, exhaustive enum variants, features and minimum supported Rust version. Explain their impact and the proposed version in the PR.
- **Exceptions:** obtain explicit user approval and record the compatibility impact in the release notes.

Require a version rationale for every changed crate. Automated Rust API checks should inform this review; they must not assign versions blindly. Until those checks are implemented, the agent performs the review.

## Documentation-only publishing

Website fixes and improvements may be published independently, without a crate release, release tag or crate version bump. The agent validates the docs, obtains deployment approval and manually runs Pages.

Keep the website accurate for currently published crates. Do not announce unreleased APIs as available. Record which source commit was deployed; a docs-only deployment is not a new crate-release baseline.

Documentation inside a published crate archive is different: shipping its updated contents requires a new crate version.

## Agent responsibility for automation gaps

The agent performs any steps the workflows do not yet automate. Current workflows create the tag/record after uploads and can replace notes with a version table: create the tag and draft beforehand, preserve the reviewed notes, and restore them before publishing the record.

Mark abandoned partial releases **incomplete** or **superseded**, preserving their source tags and publication results. Never use them as completed baselines.

Always use the most recent **completed** release in the approved source history as the preparation baseline, including an earlier release from the same day. Exclude unfinished release tags. Ask the user only for a missing decision, required approval or a blocker the agent cannot resolve.


## Local validation before review

Run from the repository root using the existing commands:

```sh
just test
just docs-check
git diff --check
```

Release tooling and its regression tests live in `tools/release-sync` and run
through `just test` on every host, without Python. To run only that suite:

```sh
just test --only release-sync
```

Registry checks are a separate read-only network operation:

```sh
just release-check
```

`just release-check` checks changed packages, versions and dependency pins against
release history and crates.io; it is read-only but requires registry access.
It does not compile or test the code. `just test` runs the current host's suites,
including native-library and benchmark compilation. Use `just test --list` to
inspect coverage or `just test --only native-libraries` to isolate a failure.
Prerequisite failures must be resolved or recorded as outstanding gates.

Run native validation on macOS, Windows and Linux for changes affecting those
platforms. A successful Mac run or cross-check does not establish native Windows
or Linux behavior. Use the relevant lab recipes in [Building](BUILDING.md) and
[Local development](LOCAL_DEVELOPMENT.md) for graphical acceptance; for the 1.27.0
integration, follow its [ordered lab checklist](upstream/1.27.0-integration.md).
Record browser, iOS and optional-feature checks where the release affects them.
CI results supplement this evidence; preparation workflows do not establish that
all native or graphical validation passed.

`just docs-check` regenerates and verifies the website and requires Node.js.
Review generated changes before including them. Toolchain and host prerequisites
are documented in [Setup](SETUP.md) and [Building](BUILDING.md).

## Preparation and publication tools

The version source of truth is [`scripts/release-packages.json`](../scripts/release-packages.json).
Do not maintain a second current-version table in this guide. Release notes carry
the reviewed old → new versions and a rationale for each changed crate.

The [Prepare daily release PR workflow](../.github/workflows/prepare-daily-release.yml)
runs on `main`. Supply the most recent completed ancestor release explicitly as
its baseline: its automatic tag selection does not exclude unfinished releases.
It proposes versions, refreshes lockfiles, checks release tooling and opens a PR.
Its automatic version proposals still require the compatibility review above.
For local preparation, choose new report paths outside the checkout:

```sh
cargo run --locked --manifest-path tools/release-sync/Cargo.toml -- daily prepare --baseline APPROVED_COMPLETED_RELEASE_TAG \
  --report /tmp/xgpui-release-preparation.json --notes /tmp/xgpui-release-notes.md
```

This previews the proposal. Add `--apply` to update versions, then run
`cargo run --locked --manifest-path tools/release-sync/Cargo.toml -- daily refresh-locks` and review the resulting diff.

The [Prepare registry release workflow](../.github/workflows/prepare-registry-release.yml)
checks out the selected dispatch commit and uses the release map. Local no-upload
equivalents, each with a new output directory outside the checkout:

```sh
cargo run --locked --manifest-path tools/release-sync/Cargo.toml -- registry-release --mode plan --out /tmp/xgpui-release-plan
cargo run --locked --manifest-path tools/release-sync/Cargo.toml -- registry-release --mode dry-run --out /tmp/xgpui-release-dry-run
```

- **plan** inspects candidate archives and registry versions, reporting
  `publish-needed`, `already-published` or `version-conflict`. Changed packaged
  contents under an existing version fail the plan.
- **dry-run** additionally performs registry-backed package/build verification
  where dependencies are available. Dependents of missing candidate versions
  remain blocked until prerequisites are published; they are not verified yet.
- **publish**, after approval, requires `--confirmation 'publish FULL_COMMIT_SHA'`
  and `CARGO_REGISTRY_TOKEN`. It preflights all candidates and verifies/builds and
  uploads missing versions in dependency order. Use the approved immutable source.

Plan and dry-run have no uploads and need no upload credentials, but access
crates.io and may download dependencies. Existing versions are skipped only after
comparing packaged source and normalized manifests. Generated lockfiles, VCS
metadata and publication permissions are excluded from that comparison; source,
resources, README, features and dependency requirements remain significant.

Each upload is checked through the registry API and a checksum-verified download
before dependents proceed. Reports, logs and archives are retained in workflow
artifacts, including failures. A retry checks registry contents again and skips
matching published versions. Unknown upload outcomes stop; investigate before
retrying. Never change an uploaded version's source or move its release tag.
Complete the registry-only consumer verification before declaring success.

The obsolete six-extension 0.2.0 publisher has been removed. Historical
publication evidence remains in the archive. Use `xgpui-release preflight` and
`prepare-registry` for manifest inventory and offline archive inspection;
`verify-packages` and `verify-color` run isolated archive/consumer checks.
Archive generation with `--no-verify --exclude-lockfile` is not registry-backed
build verification. Release commands require Cargo and Git; registry requests
also require curl, and GitHub context/finalization requires the gh CLI.

## Workflow limitations and release records

The registry workflow invokes `xgpui-release daily finalize` after successful
uploads. That helper may create a missing tag and replaces release notes with a
verification table. Create and verify the approved tag and draft beforehand;
retain the reviewed notes outside the release record and restore them with the
verification results before publishing the draft. Verify the record's draft or
published state explicitly. Do not treat the workflow's finalization alone as
completion of the policy above.

The [Publish documentation workflow](../.github/workflows/pages.yml) is manually
dispatched. After deployment approval, select the intended source and monitor its
build and deployment. Record the deployed commit and deployment link. Docs-only
deployments follow the documentation-only policy above.
