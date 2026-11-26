# Releasing XGPUI

This is the authoritative release guide. Historical publication evidence is kept
in [the publication archive](release-reviews/publishing-history.md); release-specific
reviews live in [release-reviews/](release-reviews/).

A release starts as a **release issue** that shows the whole plan in one place:
what changed, every crate's next version and why, the release notes and the
remaining steps. Everything that changes the repository then lands through
reviewed PRs. The owner makes the decisions and manually starts the workflows
that publish the crates and deploy the website.

| Step | Who | Result |
| --- | --- | --- |
| 1. Release issue | agent | Change review, versions, notes and checklist; no repository changes |
| 2. Decisions | owner | Version questions answered on the issue |
| 3. Release PRs | agent, owner merges | Versions and pins, website docs, records, merged to `main` |
| 4. Tag and draft record | agent | Release tag on the merged commit and a draft GitHub release |
| 5. Publish crates | owner starts CI | Crates uploaded from the tag, with verification reports |
| 6. Finish the record | agent | Verification table added and the GitHub release published |
| 7. Deploy the website | owner | Pages workflow run manually |

The issue's checklist tracks these steps; the agent ticks items as their PRs
merge. [#338](https://github.com/gpui-luma-dev/xgpui/issues/338) (XGPUI 4.1.0)
is the first release made this way. Use it as the reference for the next one: its
description shows the expected layout, and its **Lessons learned** section is
folded into the rules below.

## 1. Release issue — agent

When the owner asks for a release, open an issue titled
`Release: <headline versions> — <main changes>`. Change nothing in the
repository in this step.

- **Baseline:** the most recent **completed** release tag (see
  [Release records](#release-records)). Exclude unfinished or superseded tags.
- **Candidate:** current `main`, recorded by commit. If anything merges to `main`
  before the tag is created, re-run the review and update the issue.
- **Changes:** list every merged PR since the baseline and group the packaged
  changes by crate. Distinguish source changes from README, manifest and test-only
  changes; only packaged content needs a version.
- **Versions:** run the read-only [`release-check`](#tools) against the baseline as
  an input, then give every crate a next version and a reason under the
  [versioning policy](#versioning-policy). Show breaking changes and the
  re-exports that carry them into dependents.
- **Re-exports and pins, by hand:** `release-check` doesn't see a crate whose own
  code is unchanged but whose pinned dependency takes a breaking bump. On #338 it
  reported `xgpui-gamut` unchanged, yet gamut re-exports `xgpui-color`, so color's
  breaking release forced gamut 0.5.0. List every internal requirement the new
  versions no longer satisfy, and every `pub use` of another XGPUI crate.
- **Dependency pins:** list each internal requirement that must rise, and why.
- **Packaged READMEs:** compare every crate README's install snippet with the new
  versions. READMEs ship in the package and drift; #338 found four snippets naming
  versions from earlier releases.
- **Downstream check (recommended):** build xgpui-luma against the candidate with
  a local, uncommitted `.cargo/config.toml` that patches every XGPUI crate to this
  checkout, then run its lab tests. Record what breaks and what had to change. On
  #338 this found two native paragraph bugs (fixed in #336) and eight API breaks
  before anything was published.
- **Release notes (draft):** highlights, migration notes per breaking crate, the
  core public API changes with the PRs that approved them, and verification so far.
- **Publish order:** every crate to publish, in dependency order with its new version,
  computed from Cargo metadata.
- **Checklist:** the remaining steps in this guide, plus any release-specific PRs.
  The issue ends with a **Lessons learned** section, filled in after the release
  (see [step 7](#7-deploy-the-website--owner)).

Questions the policy can't settle (for example, whether a technically breaking
change in a core crate takes a major or a minor version) go in a **Decision
needed** section with the options and a recommendation.

## 2. Decisions — owner

Answer the issue's open decisions. The agent records each answer in the issue
and ticks it off.

## 3. Release PRs — agent, owner merges

Work under `issue-<N>/epic`, one PR per concern, each into the epic. GitHub
deletes the epic branch when it merges, so a follow-up after that recreates
`issue-<N>/epic` from `main`.

- **Versions and pins:** crate manifests, dependency requirements (including the
  labs' manifests for breaking versions), [`scripts/release-packages.json`](../scripts/release-packages.json),
  packaged README install snippets and `Cargo.lock`. Before opening it,
  `build-manifests --check` must pass and `release-check` must report every
  changed crate `ready-for-new-version` with no pin problems.
- **Website docs:** `pages/` updated for the coming versions: What's new, crate
  pages, guides for new APIs, and the homepage's Start here subtitles. Leave the
  changes uncommitted until the owner has hosted them locally (`just pages-serve`)
  and approved them. What's new can't name the release tag, which is created on
  the commit that contains it; link the release issue and the
  [GitHub releases](https://github.com/gpui-luma-dev/xgpui/releases) list instead.
  Merged, **not deployed**.
- **Records:** a review file in `docs/release-reviews/` when the release needs
  more evidence than the issue holds, and any guide changes.

Then open the epic's PR into `main`. CI must pass, with one exception. A macOS job
cancelled after its queue timeout, with no runner and no steps, is an
infrastructure failure rather than a test result; it happened on #334, #337 and
#343. Re-queue it, run its steps locally on the same commit, or let the owner waive
it, and say which in the PR. The owner tests Windows and Linux by hand only once
changes are on `main`.

The owner merges into `main`. The repository doesn't allow auto-merge, and the
agent isn't woken when CI succeeds, so the owner tells the agent to merge once CI
is green. The agent re-checks every result before merging.

## 4. Tag and draft record — agent

On the merged commit, **before any upload**:

- Create the remote tag `release-YYYY-MM-DD-COMMIT` (the commit's UTC date and its
  first 12 hex characters) and verify that it resolves to the full commit SHA.
  Each distinct commit gets its own tag; a retry of the same release reuses it.
- Create a **draft** GitHub release on that tag with the issue's title, notes and
  old → new version table. Keep a copy of the title and body for the final record.
- Add the tag and the draft's link to the issue.

## 5. Publish crates — owner

In GitHub Actions, run **Publish crates** on `main`. Enter the exact reviewed
release tag and `publish FULL_COMMIT_SHA` using the full SHA verified in step 4.
The workflow checks that the tag resolves to that commit in `main`'s history,
tests the release tooling, then runs preflight, packaging checks and publication
from the tagged source. The repository must have access to the organization's
`CARGO_REGISTRY_TOKEN` Actions secret. Only the publish step receives it.

The optional local path uses the same stages from a clean checkout of the tag,
with `CARGO_REGISTRY_TOKEN` set:

```sh
git switch --detach release-YYYY-MM-DD-COMMIT
just release-preflight   # registry plan; no uploads
just release-check       # package and build check; no uploads
just release-publish     # uploads in dependency order
```

Each stage requires the previous stage's report for the same tag. The workflow
uploads JSON reports as the `crates-io-release-<run>-<attempt>` artifact. Local
runs keep reports in `../xgpui-release-artifacts/<commit>/`. Publication skips versions already
on crates.io with identical contents, verifies each upload before its dependents
proceed, and stops on an unknown upload outcome.

**On failure:** keep the tag and draft. Retry the same tag where it's safe; verified
uploads are skipped. Never change an uploaded version's contents or move its tag.
A source fix needs a repair PR, a new commit, a new tag and a new draft. Mark the
old draft superseded and keep its tag.

## 6. Finish the record — agent

After the owner reports the publish:

- Read the publish report from the Actions artifact or the local release
  artifacts directory. Every crate must be `published-verified` or
  `already-published` with a registry checksum, and the source SHA must match the
  tag.
- Check the release from a fresh consumer outside the checkout: exact versions from
  crates.io, no repository patches or credentials, building code that uses the new
  APIs. State the host and what it doesn't cover.
- Restore the reviewed notes in the draft, append the
  [verification table](#release-records) and the consumer result, then publish the
  draft and recheck the tag SHA.
- Tick the issue's checklist.

## 7. Deploy the website — owner

Run the **Publish documentation** workflow
([`pages.yml`](../.github/workflows/pages.yml)) manually on `main`, then tell the
agent. On #338 "deployed" was reported before any run existed, so before closing
the issue the agent confirms all of these:

- a Publish documentation run on the release commit succeeded;
- the latest `github-pages` deployment is that commit;
- the live pages carry the new content (What's new, the homepage and the changed
  guides);
- both gallery Wasm bundles are served.

Then the agent adds the deployment link to the release record and the issue, and
writes the issue's **Lessons learned** section: what worked, what caused friction,
and suggested changes for the owner to decide. Anything durable goes back into
this guide in a docs PR. Close the issue.

## Versioning policy

- **Patch:** compatible fixes, internal backend changes, packaged documentation and compatible dependency updates.
- **Minor:** new capabilities that preserve supported public APIs.
- **Major:** deliberate breaking changes to supported public APIs.
- **Before 1.0:** use a minor bump for breaking changes or new capabilities; use a patch for compatible fixes, internal changes and packaged docs. State whether a minor release contains breaking changes.
- **Dependent crates:** bump only when packaged content or dependency requirements change. Do not automatically propagate a major bump; review whether changed dependency types affect the crate's public API. A crate that re-exports a dependency inherits that dependency's breaking changes.
- **Internal requirements:** use full stable compatible requirements (for example,
  `3.0.0` or `0.4.0`) for XGPUI libraries. Preserve an existing minimum when it
  admits the candidate version; raise it when code needs a newer API or fix.
  Release preparation must not raise compatible minimums just because a
  dependency releases. Exact requirements remain appropriate in registry
  verification fixtures. Upstream helper pins remain unchanged.
  Release tools support full stable caret/default, tilde and exact requirements;
  other syntax requires explicit review.
- **Compatibility review:** inspect removed or renamed APIs, signatures, `#[non_exhaustive]` and exhaustive enum variants, features and minimum supported Rust version. Explain their impact and the proposed version in the issue.
- **Exceptions:** need the owner's decision on the issue, recorded with the compatibility impact in the release notes.

Every changed crate needs a version rationale. Automated checks inform the review;
they don't assign versions.

Public API additions and changes in non-extension GPUI crates are reviewed and
approved individually in their own PRs (see `AGENTS.md`). The release issue lists
them with links to those approvals.

## Documentation-only publishing

Website fixes and improvements can ship without a crate release, tag or version
bump: merge the `pages/` change, then the owner runs the Pages workflow. Keep the
website accurate for currently published crates and don't announce unreleased
APIs as available. A docs-only deployment is not a new release baseline.

Documentation inside a published crate archive is different: shipping its updated
contents requires a new crate version.

## Release records

The GitHub release is the durable record, and `release-check` reads past releases
to find the baseline. Keep the human notes and the old → new table, and append a
separate three-column table containing every mapped crate and its actual result:

```markdown
| Crate | Version | Result |
| --- | --- | --- |
| `crate-name` | VERSION | published-verified |
| `unchanged-crate` | VERSION | already-published |
```

The history checker recognizes exactly `published-verified` and
`already-published` in the third column. Prose or an old/new version table alone
is not enough. Mark abandoned partial releases **incomplete** or **superseded**,
keeping their tags and publication results; never use them as a baseline.

The [maintenance/Parley release](https://github.com/gpui-luma-dev/xgpui/releases/tag/release-2026-11-08-cb66d2b2238b)
shows a complete record: 15 verified new versions, 2 verified skips, a
registry-only consumer, and the full notes with the verification table.

## Tools

Release tooling and its regression tests live in `tools/release-sync` and run
through `just test` on every host. The version source of truth is
[`scripts/release-packages.json`](../scripts/release-packages.json); don't keep a
second current-version table elsewhere.

```sh
# Read-only: changed packages, versions and pins against a baseline and crates.io.
cargo run --locked --manifest-path tools/release-sync/Cargo.toml -- release-check --epoch LAST_COMPLETED_RELEASE_TAG --report ../xgpui-release-artifacts/release-check.json

# The generated root manifest is current.
cargo run --locked --manifest-path tools/release-sync/Cargo.toml -- build-manifests --check

# The release-tooling suite only.
just test --only release-sync
```

`release-check` needs registry access but uploads nothing, and it doesn't compile
or test code. Pass the baseline explicitly: the history parser recognizes only the
verification table above, and a missed baseline can falsely flag unchanged crates.

The owner's publish commands (`just release-preflight`, `release-check`,
`release-publish`) wrap `registry-release` in its `plan`, `dry-run` and `publish`
modes:

- **plan** inspects candidate archives and registry versions, reporting
  `publish-needed`, `already-published` or `version-conflict`. Changed packaged
  contents under an existing version fail the plan.
- **dry-run** also performs registry-backed package and build verification where
  dependencies are available. Dependents of unpublished candidates are verified
  during publishing.
- **publish** requires the `publish FULL_COMMIT_SHA` confirmation, which the `just`
  recipe supplies from the tag, and `CARGO_REGISTRY_TOKEN`.

Existing versions are skipped only after comparing packaged source and normalized
manifests. Generated lockfiles, VCS metadata and publication permissions are
excluded from that comparison; source, resources, README, features and dependency
requirements remain significant.

### Known gaps

These are known holes in the tooling. Cover them by hand until they're fixed:

- `release-check` doesn't flag a crate whose pinned dependency takes a breaking
  bump, or one that re-exports such a dependency.
- No check compares packaged README install snippets with
  `scripts/release-packages.json`.
- `just pages-check` doesn't check relative links in `docs/*.md`.
- `scripts/fixtures/native-package-consumer` pins old exact versions. Step 6's
  fresh consumer replaces it for now.

## Validation

Before the release PRs merge:

```sh
just test
just pages-check
git diff --check
```

`just test` runs the current host's suites. Use `just test --list` to inspect
coverage, or `just test --only native-libraries` to isolate a failure. CI covers
macOS, Linux and Windows for PRs into `main`. After a push to `main`, the
smoke workflow checks generated manifests and the static site on the integrated
commit. For an explicitly approved direct push that bypasses PR checks, dispatch
the applicable full validation workflows on `main`. The owner's manual Windows
and Linux checks happen on `main`. Use the labs (`just labs`) for graphical checks.
`just pages-check` checks the static website in `pages/`, which is edited by hand. Toolchain and host
prerequisites are in [Local development](https://xgpui.dev/local-development.html#build-prerequisites).

## Website deployment

`.github/workflows/pages.yml` runs on manual dispatch only. It checks `pages/`, builds both demo collections, checks that the bundles exist and uploads `pages/` as is. Internal documentation and tooling are never uploaded.

Select **Settings → Pages → Source: GitHub Actions**. After deployment, repeat the browser checks at `https://xgpui.dev/` and refresh nested URLs directly to catch stale assets.

### Custom domain

Production traffic for `xgpui.dev` was migrated in [issue #61](https://github.com/gpui-luma-dev/xgpui/issues/61) and [PR #64](https://github.com/gpui-luma-dev/xgpui/pull/64) (verified 2026-10-28).

- The repository Pages custom domain is `xgpui.dev`, with **Enforce HTTPS**. The certificate covers `xgpui.dev` and `www.xgpui.dev`.
- Organization ownership is verified by the TXT record at `_github-pages-challenge-gpui-luma-dev.xgpui.dev`. Retain it.
- Apex A records: `185.199.108.153`, `185.199.109.153`, `185.199.110.153`, `185.199.111.153`. `www` is a CNAME to `gpui-luma-dev.github.io`. Preserve unrelated DNS records.
- No `CNAME` file is needed: Actions publishing uses the repository setting.
- `http://xgpui.dev/`, `https://www.xgpui.dev/` and the former project URL `https://gpui-luma-dev.github.io/xgpui/…` redirect to the HTTPS apex, keeping path and query. Keep site links relative so localhost previews also work.

```sh
dig A xgpui.dev +short
dig CNAME www.xgpui.dev +short
dig TXT _github-pages-challenge-gpui-luma-dev.xgpui.dev +short
curl -IL https://www.xgpui.dev/
curl -IL 'https://gpui-luma-dev.github.io/xgpui/examples/extensions/?demo=color&backend=webgl'
```

For rollback, redeploy a known-good commit through the workflow. If removing the domain mapping, change Pages DNS before removing the GitHub custom-domain association, and keep the ownership record. GitHub's [custom domain](https://docs.github.com/en/pages/configuring-a-custom-domain-for-your-github-pages-site/managing-a-custom-domain-for-your-github-pages-site) and [troubleshooting](https://docs.github.com/en/pages/configuring-a-custom-domain-for-your-github-pages-site/troubleshooting-custom-domains-and-github-pages) guides have the details.
