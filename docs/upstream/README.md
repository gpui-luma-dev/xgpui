# Upstream release imports

How a gpui-unofficial release is brought into xgpui. Per-release investigations, comparisons, API ledgers and accounting live beside this file (for example `1.29.3-investigation.md`); the release process itself is in [../RELEASING.md](../RELEASING.md). This guide moved here from the website's Releasing page (#422).

gpui-unofficial is xgpui's upstream. `main` contains xgpui's maintained code. Sync prepares an **incoming tree proposal**, not an integrated or buildable update.

## Review a release

Imports run locally on an issue epic, as [#406](https://github.com/gpui-luma-dev/xgpui/issues/406) did for 1.29.3. Build the tool, then compare a release with xgpui's current tree:

```sh
cargo build --locked --release --manifest-path tools/release-sync/Cargo.toml --target-dir target/release-sync
tool=target/release-sync/release/xgpui-release
$tool review prepare --release RELEASE --snapshot target/incoming --report target/review.json \
  --normalizer "$tool" --map scripts/crate-names.json   # optional: --expected-sha FULL_SHA
cargo run --locked --manifest-path tools/release-sync/Cargo.toml -- context --report target/review.json \
  --output target/zed-context.md --json-output target/zed-context.json
```

On a fresh review branch, apply the differences with `$tool review code`, `review cargo` and `review metadata` (each with `--snapshot target/incoming --report target/review.json`), committing after each. `$tool review-notes --report target/review.json --context target/zed-context.md --output review.md` writes the PR description.

The proposal uses separate commits:

1. **Code:** changed source, shaders, build scripts, assets and other package files, including additions and deletions.
2. **Cargo versions/dependencies:** package versions and dependency requirements, which affect dependency resolution.
3. **Generated metadata/artifacts:** generated package READMEs, provenance and other retained capture metadata.

Release context uses the exact old/new Zed SHAs in snapshot provenance, never latest-release or previous-stable guesses. Its path filter is the union of both captures' `transform-metadata.json` crate inventories, including nested crates. It links relevant Zed commits and referenced PRs, with root Cargo dependency changes and unrelated range commits labeled separately. Missing provenance, incomplete API pagination and non-forward ranges fail context generation rather than silently reporting no changes. The Markdown/JSON context accompanies the PR; no issue or comment is created in the upstream repository.

The PR links each commit and lists code differences separately. A release with no differences needs no PR.

Sync also compares a clean snapshot of the previously recorded source release. This labels whether each proposed file changed upstream and whether xgpui has a local modification. It does **not** automatically merge or resolve those overlaps. A downstream-only code difference is reported and retained, not applied. When upstream also changed a file, its incoming version is a manual review proposal: combine or reject overlapping changes before any merge.

`main` remains untouched. Complete incoming versions are placed only on the review branch. Platform validation, integration, publishing and merging are separate work.

For example, 1.25.3 contains release metadata changes and no upstream Rust source changes relative to 1.25.2. The spring clock difference against xgpui is a local patch, not an upstream fix; it is retained with its regression assertion.

## Names and provenance

For example, `gpui-unofficial` becomes `gpui`, and `gpui-linux-gpui-unofficial` becomes `gpui-linux`. Rust dependency aliases such as `gpui_linux` stay as they were. Cargo package identities, dependency references, directory paths and local overrides change together. The unused nested `transformed-crates` staging mirror is excluded from imports and removed by normalization. Required per-crate vendored sources are preserved.

`upstream.json` retains the source repository, release and commit, plus Zed's reported provenance. `source_packages` retains original names/paths; `packages` describes the local normalized tree. `normalization` records the schema and name map. `registry_packages` records the supporting crates fetched from the matching published `gpui-unofficial` release. Retained local crates preserve upstream repository URLs, Rust source, licenses and READMEs.

Package names change for downstream consumers. Existing dependency aliases can stay, but their `package` and path fields must point at the normalized names. Local directory names remain stable. `scripts/release-packages.json` maps the maintained packages to downstream `xgpui-*` identities at `1.0.0` for the framework and `0.2.0` for extensions; `upstream.json` records those separately under `distribution`. Upstream helper versions continue to follow the captured release.

## Integrate and validate

Review and resolve the incoming proposal on a development/integration branch. Preserve local Rust edits when resolving conflicts; do not replace main's capture with a fresh generated snapshot as a substitute for merging. The first normalization is a directory/package migration; later releases use the same stable paths.

After integration, run from the repository root. The local-source patches live in the root workspace manifest, so the standalone tools' lockfiles don't record them and need no refresh when captured versions change:

```sh
cargo run --locked --manifest-path tools/release-sync/Cargo.toml -- normalize --check
cargo run --locked --manifest-path tools/release-sync/Cargo.toml -- build-manifests
cargo check
cargo build --locked

cargo test --locked --manifest-path tools/release-sync/Cargo.toml
cargo run --locked --manifest-path tools/release-sync/Cargo.toml -- build-manifests --check
```

`build-manifests` reads the package map and preserves native target selection, standalone workspace exclusions and the compatibility patches. It supersedes the removed Python `update_build.py` compatibility entry point. Review and retain the root lockfile.

Supporting crates listed in `scripts/registry-crates.json` resolve from crates.io at the captured release version. Only the retained framework/backend crates are locally overridden; see [build details](#published-supporting-dependencies).

For standalone checks, continue invoking Cargo from the repository root so it discovers `.cargo/config.toml` (the shared target directory). The captured crates are root workspace members, so the root manifest's local-source patches apply. For example, on macOS:

```sh
cargo check --config .cargo/validation.toml \
  --manifest-path gpui/crates/gpui-platform/Cargo.toml \
  --features font-kit,runtime_shaders
cargo test --config .cargo/validation.toml \
  --manifest-path gpui/crates/gpui/Cargo.toml --lib \
  --features gpui_platform/runtime_shaders
```

`.cargo/validation.toml` remains separate from generated capture overrides. It restores the Proptest and async-process Git revisions needed by this release. Cargo patches do not force a lower version over a newer compatible registry version; inspect metadata when updating versions. A separate consumer must carry its own local package overrides rather than assuming this checkout's config is inherited.

Review the website in `pages/` after each release; `just pages-check` catches broken links and a stale upstream baseline, not stale narrative.

The review branch is an unintegrated proposal with no platform validation; review and test the chosen integration separately.

## How imports and builds stay separate

The captured manifests each declare their own workspace. The root workspace explicitly excludes each of those directories and builds their libraries as path dependencies. The harness does not rewrite their manifests or source. A separate downstream spring-animation fix uses the scheduler clock so headless tests advance deterministically; the clean upstream branch retains the original. The unused nested `transformed-crates` staging mirror is removed during import and normalization. Required per-crate vendored shader inputs remain intact.

`Cargo.toml` and `tools/build-all/Cargo.toml` are generated development files; they are not overwritten by upstream imports or downstream normalization. After an import adds or removes packages, regenerate them and update the root lockfile on the development or integration branch:

```sh
cargo run --locked --manifest-path tools/release-sync/Cargo.toml -- build-manifests
cargo generate-lockfile
cargo build --locked
```

CI checks `xgpui-release build-manifests --check` to detect a stale harness. `cargo run --locked --manifest-path tools/release-sync/Cargo.toml -- build-manifests --check` delegates to the same Rust generator. Review its platform classification when upstream adds a backend. Keep the root Cargo.lock committed for reproducible native builds.

The generated root manifest restores production compatibility dependencies from `.cargo/validation.toml`. The Proptest override stays scoped to explicit standalone test validation, avoiding an unused-patch warning in root library builds. Compiling the macOS utility crate requires Zed's async-process fork: its process launcher calls `Child::adopt_raw_pid`, which the registry dependency does not provide. The fork revision is taken from the original Zed release's workspace patch. These overrides repair extraction omissions without editing captured source.

The local Apple backends and extension producers use `objc2`, `objc2-metal`, and `block2`. The legacy Cocoa/metal-rs `block` compatibility patch is no longer needed by this checkout. Published versions can still resolve the older bindings; verify a consumer's selected dependency graph independently of local builds.

## Local naming

The raw gpui-unofficial release remains on upstream. Development uses a normalized capture with names such as gpui and gpui-linux. The Rust tool in tools/release-sync applies scripts/crate-names.json and preserves original package paths in upstream.json. See [the sync process](#upstream-release-imports).

## Published supporting dependencies

`scripts/registry-crates.json` maps former local helper names to their published `*-gpui-unofficial` packages. Normalization preserves Rust dependency aliases, features, target conditions, and upstream source provenance. It removes helper source directories and local patches, and records registry identities and the release version in `upstream.json`. Core, macros, and platform/renderer backends remain local. Runtime Rust source is not rewritten by this migration.

After an upstream import, run normalization before generating build manifests. Normalization also updates direct helper dependencies in app and extension manifests. Refresh their lockfiles when changing the captured release. The registry version follows the captured core manifest, not the latest registry release. Keep shared types such as `Refineable` on the same registry package throughout the graph.

Verify resolved package identities from the repository root:

```sh
cargo metadata --locked --format-version 1 > /tmp/xgpui-metadata.json
cargo run --locked --manifest-path tools/release-sync/Cargo.toml -- check-local-dependencies --metadata /tmp/xgpui-metadata.json
```

This checks local framework sources, pinned registry helpers, and accidental pulls of the upstream framework alongside the local fork.

## Downstream release identities

`scripts/release-packages.json` defines downstream package identities and individual version overrides for ten framework/backend crates and seven extensions. Normalization applies those versions while keeping directory names, library names and dependency aliases stable. The helper package version follows the captured `release/1.29.3` provenance independently. `upstream.json` records both sets.

For example, the local `gpui` crate is packaged as `xgpui`; applications retain `use gpui::*` through a Cargo alias. Internal dependencies use exact requirements from the release map. Supporting packages retain their upstream identities and `=1.29.3` requirements. The selected downstream family uses core 4.0.0, Apple 5.0.0 and macOS 5.0.0, with matching pins. Interactive iOS simulator/device validation remains pending.
