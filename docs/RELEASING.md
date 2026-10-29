# Release cycle

The [GitHub Releases page](https://github.com/gpui-luma-dev/xgpui/releases)
is the release tracker. One rolling draft contains every merged PR since the
previous published repository release, with links back to the PRs. No tracking
issue, mandatory labels, conventional commit format or release branch is needed.

## Everyday workflow

1. Merge reviewed PRs into `main`. **Update next release** refreshes the draft
   automatically. Work after the initial checkpoint belongs to the `1.1.0`
   development cycle, even while crates.io still serves the previous versions.
2. When ready, prepare one release PR: update `scripts/release-packages.json`,
   normalize manifests and refresh affected lockfiles, then pass CI and the
   package/consumer checks described in [Publishing](PUBLISHING.md). Choose
   SemVer versions based on API compatibility. Merge requires explicit approval.
3. Publish the selected crates from that reviewed commit, finish the draft's
   crate-version and validation sections, and publish the GitHub release with
   its tag targeting **that exact commit**. The next merge opens the next draft.

The draft is a working ledger, not a publication approval. It is regenerated on
each push: keep release preparation details in the release PR until the final
edit. Keep `main` quiet during the final publication window so the notes and
tag describe the same source. If work has moved ahead, remove later PR entries
from the final notes and explicitly target the reviewed release commit when
creating the tag.

## Versions and boundaries

Repository tags (`v1.0.0`, `v1.1.0`, ...) identify release cycles and source
checkpoints. `scripts/release-packages.json` remains the authority for individual
crate versions. Framework crates currently use `1.0.0`; six extensions use
`0.2.0`. Do not change crate versions just to indicate ongoing development.
During release preparation, update affected crates and dependent internal
version requirements together. Never reuse a published crate version for new
contents. An extension-only cycle can leave framework versions unchanged.

Release Drafter proposes the next minor version by default. An optional `major`
PR label proposes a major version; unlabeled PRs always appear. For a patch-only
cycle, edit the draft title and tag immediately before publication. Review the
final version before publication; labels are suggestions,
not API compatibility checks. Breaking framework API changes require a major
crate version even if the draft initially proposed a minor cycle.

The first workflow run creates a historical `v1.0.0` GitHub release at
`632c60ace70931391b867d29a03d2ac242b183c1`, the completed framework/extension
publication checkpoint. Its notes explicitly distinguish this repository
checkpoint from the commits used to build individual registry archives.
This prevents the next draft from including the entire repository history.
Later drafts use the previous published release as their boundary.

## Automation scope

The workflow only establishes the initial checkpoint and maintains a draft.
It never uploads crates, merges PRs, or publishes subsequent GitHub releases.
It needs the standard workflow token with `contents: write` and
`pull-requests: read`; no extra release secret is needed. Manual recovery is
**Actions → Update next release → Run workflow** on `main`.

Crate publication still uses the existing verification safeguards. The old
extension publisher is pinned to the approved `0.2.0` source and is **not** a
general next-version publishing command. A future publication must update the
publisher's reviewed scope, versions and source pin in its release PR.

Direct commits do not have PR entries. Put releasable work through PRs, and
record any exceptional direct changes in the final notes.
