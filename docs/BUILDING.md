# Build xgpui

See [Local Development](LOCAL_DEVELOPMENT.md) for Windows, Linux and macOS
commands, native app launches and website previews. This page covers the build
harness and capture layout.

Run these commands from the repository root:

```sh
cargo build --locked
```

The private root harness builds GPUI and the selected platform backend by
default. Nine framework/backend crates remain local. Seventeen supporting crates
come from the published `gpui-unofficial` release, currently pinned to `=1.27.0`.
Optional integrations and development-only support are not direct default build
inputs. Metal shaders compile at build time
by default. The harness launches nothing.

Use the explicit full-capture mode to compile every captured library supported
by the selected target:

```sh
cargo build --locked --features full-capture
# Equivalent recipe:
just build-full
```

Full-capture adds every retained local library supported by the selected target,
including the WGPU renderer on macOS. Supporting registry crates are compiled
when required by those libraries; full-capture no longer directly builds the
upstream utility and test tools. Neither mode enables every optional feature or
standalone test, benchmark, example, or binary target. Windows and Linux still
require their own native checks.

`just test` runs CI's full native-library and platform-benchmark compilation
gates before its runtime suites. The extension suites enable ICC, canvas GPUI
integration and media wgpu coverage; Linux additionally checks media without
GPU backends and Parley with system fonts. Use `just test --only native-libraries`
or `just test --only gpui-bench-build` to isolate compilation failures.
Hosted Linux extensions/labs, Windows lab applications, and the three platform
paint workflows are manual-only (`workflow_dispatch`). Run them from Actions →
the selected workflow → Run workflow when hosted verification is needed.
`Validate repository tooling` is also manual-only, including its tooling checks,
macOS/Linux/Windows native-library matrix, iOS compilation and WASM build.
PR acceptance is verified in the local test environments;
record the tested commit SHA, command and result in the PR.

Local results cover the current host; platform-specific tests still require
their native operating system. The release/version preflight checks package
versions and pins; its success does not establish that builds or tests passed.

The test runner uses Bash and a small Rust helper built from `tools/release-sync`;
warning capture and artifact indexing require no Python. The helper reuses the
release tool's dependencies. Its bootstrap build retains text diagnostics in
`cargo/bootstrap.stderr.log`; subsequent Cargo invocations retain structured JSON.

`just test` keeps warnings visible and writes a combined report to
`target/test-logs/warnings.log`, with structured records in `warnings.jsonl`.
The summary prints the unique warning count and report paths, including on failed
runs. Shared and isolated Cargo invocations retain raw JSON messages and stderr
under `target/test-logs/cargo/`. Set `TEST_ALL_LOG_DIR` to retain a run elsewhere;
the next run replaces the Cargo diagnostics and warning reports in that directory.

Compiler warnings are deduplicated by package, target and complete diagnostic
(including source spans), ignoring rendered formatting. Records retain occurrence
counts and the contributing build/suite names. Cargo stderr warnings are grouped
with their continuation text and deduplicated by that text; they may lack package
or source locations. Cargo's compiler-warning count summaries are omitted from
the combined count because the individual structured diagnostics already cover
them. Raw stderr remains available. Runtime output stays in the suite logs;
warning-prefixed stderr from programs launched by Cargo can also appear in the
stderr warning report. Warnings do not change the build/test exit status.

The benchmark compilation gate checks that every requested executable was
produced and exists; it does not launch benchmarks, including with `--help`.
Forwarded arguments to `just test` are Cargo options, applied to the shared build
and isolated Cargo invocations. They are not forwarded to prebuilt test binaries.
The runner owns Cargo's `--message-format` option for diagnostic capture.

## Requirements and options

### Cleaning build outputs

The repository Cargo configuration shares the root `target/` directory across
standalone crates too, so root `cargo clean` removes normal build outputs.
Older branches or explicit overrides can still leave nested target directories.
Stop active builds, then run `just clean-preview` to list all of them and
`just clean` to remove them. Without Just, use
`bash scripts/clean_build_outputs.sh` and add `--apply` to delete.
The cleaner skips symlinks and refuses directories containing tracked files or
not ignored by Git. It leaves source assets intact. Web bundles (`target/web/`)
and staged deployment
files (`target/site/`) live under the root target directory and are cleaned too.
Build outputs outside this checkout (such as an external `CARGO_TARGET_DIR`)
are not included. Use `--target-dir target` for standalone builds to share the
root output directory, as the lab recipes already do.

### Build prerequisites

- Rust with the 2024 edition; tested with Rust 1.99.0 on Apple Silicon macOS.
- Xcode selected as the active developer directory, with the Metal compiler
  installed, for the default macOS shader build.
- macOS 12.0 or later for the native Apple backend.
- Rust for downstream normalization and build-manifest generation. The existing
  raw import workflow retains its current importer.

```sh
# Optimized native libraries.
cargo build --release --locked

# Embed shaders for runtime compilation instead of invoking the Metal compiler.
cargo build --locked --features runtime-shaders

# Static checks on the root harness and its dependency graph.
cargo check --workspace --locked
cargo clippy --workspace --locked
cargo fmt --package xgpui-build --check
```

Root Clippy and formatting checks target the harness; imported libraries are
dependencies, not root workspace members. Existing platform/library validation
is still run separately by validate.yml.

## Compile the captured examples

```sh
cargo build --config .cargo/validation.toml \
  --manifest-path gpui/crates/gpui/Cargo.toml \
  --examples --features gpui_platform/font-kit --target-dir target
```

This builds example executables under `target/debug/examples/` without launching
them. Add `gpui_platform/runtime_shaders` to the comma-separated feature list
if you need runtime shader compilation. Standalone example builds use their
own Cargo.lock; the root lockfile applies to the root harness build.

`cargo test` at the root tests only the harness. For the imported library's
headless tests, follow [SETUP.md](SETUP.md); its dependency override and feature
flags remain required. No test or application is launched by `cargo build`.

## How imports and builds stay separate

The captured manifests each declare their own workspace. The root workspace
explicitly excludes each of those directories and builds their libraries as
path dependencies. The harness does not rewrite their manifests or source.
A separate downstream spring-animation fix uses the scheduler clock so headless
tests advance deterministically; the clean upstream branch retains the original.
The unused nested `transformed-crates` staging mirror is removed during import
and normalization. Required per-crate vendored shader inputs remain intact.

`Cargo.toml` and `tools/build-all/Cargo.toml` are generated development files;
they are not overwritten by upstream imports or downstream normalization. After an import adds or removes
packages, regenerate them and update the root lockfile on the development or
integration branch:

```sh
cargo run --locked --manifest-path tools/release-sync/Cargo.toml -- build-manifests
cargo generate-lockfile
cargo build --locked
```

CI checks `xgpui-release build-manifests --check` to detect a stale harness.
`python3 scripts/update_build.py --check` delegates to the same Rust generator.
Review its platform classification when upstream adds a backend.
Keep the root Cargo.lock committed for reproducible native builds.

The generated root manifest restores production compatibility dependencies from
`.cargo/validation.toml`. The Proptest override stays scoped to explicit standalone
test validation, avoiding an unused-patch warning in root library builds.
Compiling the macOS
utility crate requires Zed's async-process fork: its process launcher calls
`Child::adopt_raw_pid`, which the registry dependency does not provide. The fork
revision is taken from the original Zed release's workspace patch. These
overrides repair extraction omissions without editing captured source.

The local Apple backends and extension producers use `objc2`, `objc2-metal`,
and `block2`. The legacy Cocoa/metal-rs `block` compatibility patch is no longer
needed by this checkout. Published versions can still resolve the older bindings;
verify a consumer's selected dependency graph independently of local builds.

## Local naming

The raw gpui-unofficial release remains on upstream. Development uses a
normalized capture with names such as gpui and gpui-linux. The Rust tool in
tools/release-sync applies scripts/crate-names.json and preserves original
package paths in upstream.json. See [the sync process](SETUP.md).

## Published supporting dependencies

`scripts/registry-crates.json` maps former local helper names to their published
`*-gpui-unofficial` packages. Normalization preserves Rust dependency aliases,
features, target conditions, and upstream source provenance. It removes helper
source directories and local patches, and records registry identities and the
release version in `upstream.json`. Core, macros, and platform/renderer backends
remain local. Runtime Rust source is not rewritten by this migration.

After an upstream import, run normalization before generating build manifests.
Normalization also updates direct helper dependencies in app and extension
manifests. Refresh their lockfiles when changing the captured release. The
registry version follows the captured core manifest, not the latest registry
release. Keep shared types such as `Refineable` on the same registry package
throughout the graph.

Verify resolved package identities from the repository root:

```sh
cargo metadata --locked --format-version 1 > /tmp/xgpui-metadata.json
python3 scripts/check_local_dependencies.py /tmp/xgpui-metadata.json
```

This checks local framework sources, pinned registry helpers, and accidental
pulls of the upstream framework alongside the local fork.

## Downstream release identities

`scripts/release-packages.json` defines downstream package identities and individual
version overrides for ten framework/backend crates and seven extensions.
Normalization applies those versions while keeping directory names, library names
and dependency aliases stable. The helper package version follows the captured
`release/1.27.0` provenance independently. `upstream.json` records both sets.

For example, the local `gpui` crate is packaged as `xgpui`; applications retain
`use gpui::*` through a Cargo alias. Internal dependencies use exact requirements
from the release map. Supporting packages retain their upstream identities and
`=1.27.0` requirements. The selected downstream family uses core 3.0.0 and Apple/macOS 4.0.0, with
matching exact pins. The integration still requires final public API review
before release.
Interactive iOS simulator/device validation remains pending.

## Extension lab package

`apps/xgpui-ext-lab/Cargo.toml` owns the native labs, benchmarks and two browser
demo targets. `just lab-build` builds the native binaries; `just lab-test` runs
unit and compatibility tests. Select a particular workload using `--bin`, and
use its browser build script for WASM packaging. The package has its own shared
lockfile and `publish = false`; it is excluded from the library release map.

## Benchmark support

The platform benchmarks require `bench-support`. Their test targets also need
`test-support` when using `cargo check --benches`:

```sh
cargo check --locked --config .cargo/validation.toml --manifest-path gpui/crates/gpui-platform/Cargo.toml --features bench-support,test-support,runtime_shaders --benches
```

The core package includes Zed's benchmark metrics source at the captured Zed
commit. `bench-metrics-provenance.json` records that source and the local test
adaptation. It is compiled only with `bench-support`; the Linux `perf-event2`
dependency is optional and only activated by that feature. The public benchmark
measurement and allocator types remain available through `gpui`.

Benchmark collectors use process-wide tracing and allocation state. Run their
unit tests with `--test-threads=1` to isolate measurements from other test cases.
Native CI compiles all platform benchmarks and runs the metrics and benchmark
context tests on Windows, Linux and macOS.
