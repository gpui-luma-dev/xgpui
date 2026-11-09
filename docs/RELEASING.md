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
- Reuse already prepared version numbers and existing release PRs; do not run an automatic bump over them. Propose only the remaining old → new versions, with a reason for each.
- Reuse the existing release prose in docs for the GitHub release notes, retaining change titles, short summaries, links and an old → new version table. Keep alpha limits explicit. Do not rewrite docs merely to create the GitHub record.
- For a first publication, check the crate is in the release map, enable crates.io publication, and verify package metadata, README and the actual `LICENSE-APACHE` file. A manifest license declaration alone is insufficient. Inspect the candidate archive before tagging; a package file listing is a quick check, not full archive/build verification. Raise internal dependency minimums when the new crate uses newly added APIs.
- Review automatic version proposals against the versioning policy below. Include a reason for every bump in the PR.
- Run the validation and no-upload registry checks below; resolve failures and version conflicts. Record source identity, host, commands and results in the release review. Revalidate affected checks after source changes.
- For each public API addition or change in non-extension GPUI crates, document purpose, source/serialization compatibility, migration requirements and whether it could live in an extension. Present each concrete API change for final user review; implementation approval does not approve the final public API.
- Present the PR, versions, validation scope and exact publish/skip counts. Distinguish a version audit, archive payload plan and actual package/build verification.

**User:** review the proposal and approve or revise the version numbers. No commands or manual preparation are required.

## 2. Merge — agent, after approval

**User:** explicitly approve merging the release PR.

**Agent:** verify relevant CI tests, lint and builds pass, then merge the approved PR and remove its branch, or confirm the merge if the user performed it. Record the full merge commit SHA from GitHub. Preserve unrelated local edits; a local checkout/cleanup failure does not mean the remote merge failed.

## 3. Create the tag and release record — agent

**Before any crates.io upload:**

- Create the remote Git tag explicitly, then verify its full SHA before creating the draft. Creating a draft release with a tag name does not necessarily create the Git ref; workflow dispatch requires the real ref.
- Tag the approved commit as `release-YYYY-MM-DD-COMMIT`, using its UTC commit date and first 12 hex characters.
- Support multiple releases per day: each distinct approved commit gets its own tag, for example `release-2026-10-31-ed4469267251` and `release-2026-10-31-a1b2c3d4e5f6`. Retries of the same release reuse its tag and record; do not create a new release identity for a retry.
- Create a draft GitHub release containing the reviewed notes and version table. Preserve a copy of its title and body in the named release-artifacts directory before running publication; the current finalizer overwrites them.
- Verify the remote tag resolves to the approved full SHA and the record is a draft. Explain if the UTC tag date differs from the user's local date. Run the final no-upload plan against this tag, download its report and check the exact source SHA, publish/skip counts and payload conflicts. A successful plan has uploads disabled and may still report `release_ready: false`; only completed publication verification must report true. Present the draft and results to the user.

## 4. Publish and verify crates — agent, after approval

**User:** explicitly approve publication of the stated new versions and any still-pending, individually presented public API changes. Ask plainly: “Publish these N new crate versions? These other versions will be verified and skipped.” Identify any pending API item and its compatibility impact plainly; merging implementation does not itself approve its final public API.

**Agent:**

- Dispatch publication against the tag, confirming `publish FULL_COMMIT_SHA`; do not select a moving `main` branch. Publish in dependency order and monitor through completion.
- Verify and skip matching versions already published; verify each new upload.
- Inspect the report: the source SHA must match, `release_ready` must be true, and every candidate must be `published-verified` or `already-published` with a registry checksum. Report the actual new/skip counts.
- Use a fresh consumer outside the checkout, with exact versions for the full release map and no repository patches or credentials. Verify its Cargo metadata resolves those versions from crates.io, then build/run representative APIs, including new extension APIs. Use the actual Rust library name, which may differ from the package name (`xgpui` exposes `gpui`), and exercise new behavior rather than only importing types. State the host and limits; this is not interactive or cross-platform acceptance.
- Restore the reviewed title and notes after the workflow finalizer runs. Add the publication/consumer results and workflow link. Keep the version history and add the machine-readable verification table described below. Explicitly publish the existing draft and recheck the tag SHA and published state.
- Report crate publication and release-record completion; say separately that Pages is still undeployed.

**On failure:** retain the tag and draft, diagnose the failure and retry the same commit where safe, skipping verified uploads. Never move a tag after an upload. A source fix requires a repair PR with explicit merge approval, then a new commit-derived tag/draft and another tagged plan. Preserve the old tag and mark its draft superseded. Before any upload, unchanged candidate versions can be reused if still available; after an upload, do not reuse that version for changed contents. Keep the record in draft and hold docs deployment until verification succeeds.

## 5. Deploy documentation — agent, after approval

**User:** approve documentation deployment.

**Agent:** check that the intended docs source describes the published state, replacing stale “in preparation”/“unpublished” wording while retaining alpha warnings. Any source edits go through a PR and explicit merge approval. Then manually dispatch Pages, monitor it to completion, verify the deployed commit, live site, documented versions, galleries and both Wasm bundles, and add the deployment link to the release record. Report docs deployment separately from crate publication.

## Versioning policy

Apply this policy during preparation:

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

The agent performs any steps the workflows do not yet automate. The finalizer can create missing tags/records after uploads and replaces the title and notes with generated text. Create the tag/draft beforehand, preserve the reviewed title/body outside the checkout, and restore them with verification evidence before publishing the record. Updating this guide does not change workflow behavior.

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
just release-check --epoch LAST_COMPLETED_RELEASE_TAG
```

`just release-check` checks changed packages, versions and dependency pins against
release history and crates.io; it is read-only but requires registry access.
Pass the completed baseline explicitly: the history parser currently recognizes only the verification table format below and can miss releases recorded only with prose or old/new version tables. A missed baseline can falsely flag unchanged crates. Confirm the actual tag and packaged diff before proposing bumps.

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

Work in the normal repository checkout. Keep builds and downloaded workflow
reports under `target/`. Use a named, persistent release-artifacts directory
outside the checkout for isolated consumers and tools that require external
outputs; do not create `/tmp` checkouts or scatter release files there. The agent
creates that directory and sets `XGPUI_RELEASE_ARTIFACTS` to its absolute path
before using the examples below. Keep edits local until the user requests a
branch push or PR.

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
  --report "$XGPUI_RELEASE_ARTIFACTS/preparation.json" --notes "$XGPUI_RELEASE_ARTIFACTS/notes.md"
```

This previews automatic patch suggestions; it is not needed when versions are already prepared. Apply only the user-approved version plan, preserving compatible requirements. After version changes, run
`cargo run --locked --manifest-path tools/release-sync/Cargo.toml -- daily refresh-locks` and review the resulting diff. Reject unplanned external dependency changes.

The [Prepare registry release workflow](../.github/workflows/prepare-registry-release.yml)
checks out the selected dispatch commit and uses the release map. Local no-upload
equivalents, each with a new output directory outside the checkout:

```sh
cargo run --locked --manifest-path tools/release-sync/Cargo.toml -- registry-release --mode plan --out "$XGPUI_RELEASE_ARTIFACTS/plan"
cargo run --locked --manifest-path tools/release-sync/Cargo.toml -- registry-release --mode dry-run --out "$XGPUI_RELEASE_ARTIFACTS/dry-run"
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
uploads. It validates the report and tag identity, but overwrites the release
title/body and does not publish an existing draft. Agent restoration and explicit
draft publication remain required.

Preserve the human release notes and old/new table. Append a separate three-column
table containing every mapped crate and its actual report result:

```markdown
| Crate | Version | Result |
| --- | --- | --- |
| `crate-name` | VERSION | published-verified |
| `unchanged-crate` | VERSION | already-published |
```

The history checker recognizes exactly `published-verified` and
`already-published` in the third column. A prose statement or a previous/candidate
version table alone is not sufficient. Keep this verification table alongside
the highlights so the next release can identify the completed baseline.

The [maintenance/Parley release](https://github.com/gpui-luma-dev/xgpui/releases/tag/release-2026-11-08-cb66d2b2238b)
is an example of this sequence: pre-upload tag and draft, 15 verified new versions,
2 verified skips, a registry-only consumer, and restored full notes with the
verification table. Pages remained a separate action.

The [Publish documentation workflow](../.github/workflows/pages.yml) is manually
dispatched. After deployment approval, select the intended source and monitor its
build and deployment. Record the deployed commit and deployment link. Docs-only
deployments follow the documentation-only policy above.
