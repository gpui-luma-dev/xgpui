# xgpui release imports

gpui-unofficial is xgpui's upstream repository. `main` is xgpui development.
Each incoming release gets a matching branch, for example `release/1.26.0`,
and a draft PR from that branch to `main`. Merging is manual.

gpui-unofficial publishes independent orphan release snapshots. Sync reads those
files rather than merging their Git histories. xgpui's release branches retain
connected history so incoming differences can merge with downstream patches.

## Import a release

Run **Actions → Sync GPUI release** from main with the release number and,
optionally, an expected source SHA. The workflow:

1. Selects `release/<version>`, based on the previous imported release's branch.
2. Fetches gpui-unofficial's matching release and compares its captured files,
   including additions and deletions. Crate names are normalized for xgpui.
3. Commits and pushes the incoming release differences onto that version branch.
4. Automatically opens a draft PR to `main`, or updates its existing open PR.
   Existing PRs retain their current draft/review state. No PR for an empty diff.

Release branches hold imported source, not downstream development patches.
Basing each import on the prior release keeps xgpui-only edits out of the incoming
diff. Resolve conflicts during manual integration rather than overwriting main's
patched files with a new snapshot.

For migration, the first version branch can use the existing `upstream-normalized`
branch as its baseline. If that branch is absent, sync discovers the first
normalized import commit in main's provenance history. The baseline's source SHA
must match main's recorded import. The old `upstream` and `upstream-normalized` branches are not updated by
sync. Keep version branches available as baselines for later imports.

Dispatching sync authorizes its release branch and draft PR updates. It does not
merge, publish, or run Windows validation. Unknown crates, name collisions and
baseline mismatches stop the run for review. Repeating an unchanged import does
not create another commit or PR.

## Names and provenance

For example, `gpui-unofficial` becomes `gpui`, and
`gpui-linux-gpui-unofficial` becomes `gpui-linux`. Rust dependency aliases such as
`gpui_linux` stay as they were. Cargo package identities, dependency references,
directory paths and local overrides change together. The nested transformed
staging copy is normalized too, but is not a second build input.

`upstream.json` retains the source repository, release and commit, plus Zed's
reported provenance. `source_packages` retains original names/paths; `packages`
describes the local normalized tree. `normalization` records the schema and name
map. Upstream repository URLs, Rust source, licenses and READMEs are preserved.

Package names change for downstream consumers. Existing dependency aliases can
stay, but their `package` and path fields must point at the normalized names.
These local names do not imply ownership of the corresponding crates.io names.

## Integrate and validate

Merge the release-specific branch into the development/integration branch. Preserve local
Rust edits when resolving conflicts; do not replace main's capture with a fresh
generated snapshot as a substitute for merging. The first normalization is a
directory/package migration; later releases use the same stable paths.

After integration, run from the repository root. Refresh the standalone tools'
lockfiles too when captured versions change, because Cargo records unused local
patches in them:

```sh
cargo generate-lockfile --manifest-path tools/release-sync/Cargo.toml
cargo generate-lockfile --manifest-path tools/crate-roadmap/Cargo.toml
cargo run --locked --manifest-path tools/release-sync/Cargo.toml -- normalize --check
cargo run --locked --manifest-path tools/release-sync/Cargo.toml -- build-manifests
cargo check
cargo build --locked

cargo test --locked --manifest-path tools/release-sync/Cargo.toml
cargo run --locked --manifest-path tools/release-sync/Cargo.toml -- build-manifests --check
```

`build-manifests` reads the package map and preserves native target selection,
standalone workspace exclusions and the compatibility patches. It supersedes the
legacy `scripts/update_build.py` generator, which is retained but is not the
normalized build path. Review and retain the root lockfile.

For standalone checks, continue invoking Cargo from the repository root so it
discovers `.cargo/config.toml`. For example, on macOS:

```sh
cargo check --config .cargo/validation.toml \
  --manifest-path gpui/crates/gpui-platform/Cargo.toml \
  --features font-kit,runtime_shaders
cargo test --config .cargo/validation.toml \
  --manifest-path gpui/crates/gpui/Cargo.toml --lib \
  --features gpui_platform/runtime_shaders
```

`.cargo/validation.toml` remains separate from generated capture overrides. It
restores the Proptest and async-process Git revisions needed by this release.
Cargo patches do not force a lower version over a newer compatible registry
version; inspect metadata when updating versions. A separate consumer must carry
its own local package overrides rather than assuming this checkout's config is
inherited.

Refresh and review the web documentation after each release; regeneration alone
does not validate the narrative or SVG teaching model. See
[roadmap maintenance](roadmap/BUILDING.md) and [build options](BUILDING.md).

Sync imports and normalizes source changes; it does not run platform builds or
runtime validation. Review and validation happen separately before merging.

## GitHub settings

Keep main as the default branch and preserve imported version branches after
merges. Enable **Allow GitHub Actions to create and approve pull requests** in the
repository/organization Actions settings so the workflow can create its draft PR.
If policy denies PR creation, sync fails at that step; the release branch remains
available for review. Nothing is merged automatically.
