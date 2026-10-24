# xgpui release imports

gpui-unofficial is xgpui's upstream. `main` contains xgpui's maintained code.
Sync prepares an **incoming tree proposal**, not an integrated or buildable update.

## Review a release

Run **Actions → Sync GPUI release** on main with a release number and optional
source SHA. Sync normalizes the release's crate names, compares that tree with
xgpui's current tree, and opens a **new draft PR on a fresh version-named branch**.
Existing PRs are never edited, force-pushed or merged.

The proposal uses separate commits:

1. **Code:** changed source, shaders, build scripts, assets and other package
   files, including additions and deletions.
2. **Cargo versions/dependencies:** package versions and dependency requirements,
   including mirrored manifests. These affect dependency resolution.
3. **Generated metadata/artifacts:** generated package READMEs, provenance and
   the remaining duplicate `transformed-crates` files.

Release context uses the exact old/new Zed SHAs in snapshot provenance, never
latest-release or previous-stable guesses. Its path filter is the union of both
captures' `transform-metadata.json` crate inventories, including nested crates.
It links relevant Zed commits and referenced PRs, with root Cargo dependency
changes and unrelated range commits labeled separately. Missing provenance,
incomplete API pagination and non-forward ranges fail context generation rather
than silently reporting no changes. Markdown/JSON context accompanies the PR and
workflow artifact; no issue or comment is created in the upstream repository.

The PR links each commit and lists code differences separately. The detailed JSON
comparison is uploaded as a workflow artifact. A release with no differences
creates no PR.

Sync also compares a clean snapshot of the previously recorded source release.
This labels whether each proposed file changed upstream and whether xgpui has a
local modification. It does **not** automatically merge or resolve those overlaps.
A downstream-only code difference is reported and retained, not applied. When
upstream also changed a file, its incoming version is a manual review proposal:
combine or reject overlapping changes before any merge.

`main` remains untouched. Complete incoming versions are placed only on the review
branch. Platform validation, integration, publishing and merging are separate work.

For example, 1.25.3 contains release metadata changes and no upstream Rust source
changes relative to 1.25.2. The spring clock difference against xgpui is a local
patch, not an upstream fix; it is retained with its regression assertion.

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

Review and resolve the incoming proposal on a development/integration branch. Preserve local
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

Sync performs no platform validation. Its branch is an unintegrated proposal;
review and test the chosen integration separately.

## GitHub settings

Keep main as the default branch. Enable **Allow GitHub Actions to create and
approve pull requests** in repository/organization Actions settings. Sync creates
only new draft PRs; it never approves or merges them. The old persistent `upstream`
and `upstream-normalized` branches are not used or updated.
