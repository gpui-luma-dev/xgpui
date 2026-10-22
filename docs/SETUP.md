# xgpui release imports

`main` is the development branch. Synchronization keeps two persistent branches:

- `upstream`: the exact gpui-unofficial capture, with its original names.
- `upstream-normalized`: the same capture with local crate names, used for merges.

Do not add development patches to either generated branch. Preserve both branches
and use merge commits when integrating their history; resolve conflicts on a
development/integration branch based on main.

## Import a release

Run **Actions → Sync GPUI release** from main, supplying the release number and,
optionally, an expected source SHA. The workflow:

1. Uses the existing importer to replace `gpui/` on upstream and record provenance.
2. Restores those capture files onto upstream-normalized, replacing the previous
   generated snapshot rather than overlaying it.
3. Runs the Rust normalizer with the explicit `scripts/crate-names.json` mapping.
4. Commits and pushes changes to the generated branches when necessary.
5. Provides a comparison link against main. Creating/updating a PR requires the
   dispatch's `open_pr` opt-in and organization permission.

Dispatching this workflow authorizes its branch updates. It never publishes
crates, merges a PR or launches an application. No workflow is dispatched merely
by changing these files locally.

Re-running an unchanged capture is idempotent. Unknown crates or naming collisions
stop normalization for review; add a reviewed mapping before retrying. Do not
derive new names silently. When updating the normalizer, retain its schema and
mapping version in provenance and review the resulting generated diff.

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

Merge upstream-normalized into the development/integration branch. Preserve local
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

The workflow can still validate the original raw upstream paths through its
`source_ref` input. Native macOS checks do not establish browser, Linux, Windows
or mobile runtime compatibility.

## GitHub settings

Keep main as the default branch and preserve upstream/upstream-normalized after
merges. If organization policy blocks Actions-created PRs, leave
`SYNC_AUTO_PR=false`; the workflow supplies the normalized comparison link.
Even when the organization permits PR creation, the dispatch's `open_pr` input
defaults to false. Review and authorize each operation separately.
