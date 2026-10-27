# Build xgpui

See [Local Development](LOCAL_DEVELOPMENT.md) for Windows, Linux and macOS
commands, native app launches and website previews. This page covers the build
harness and capture layout.

Run these commands from the repository root:

```sh
cargo build --locked
```

The private root harness builds GPUI and the selected platform backend by
default. On macOS this resolves 15 captured libraries, Windows 14, and Linux 15
with the generated native features. Optional integrations and development-only
support are not direct default build inputs. Metal shaders compile at build time
by default. The harness launches nothing.

Use the explicit full-capture mode to compile every captured library supported
by the selected target:

```sh
cargo build --locked --features full-capture
# Equivalent recipe:
just build-full
```

On macOS full-capture resolves 23 captured libraries, including HTTP integrations,
utility/test support and the WGPU renderer. All 26 captured crates remain available
across targets. Neither mode enables every optional feature or standalone test,
benchmark, example, or binary target. Cross-target dependency resolution is not
platform build validation; Windows, Linux and macOS require their own checks.
Browser checks are secondary.

## Requirements and options

- Rust with the 2024 edition; tested with Rust 1.99.0 on Apple Silicon macOS.
- Xcode selected as the active developer directory, with the Metal compiler
  installed, for the default macOS shader build.
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

A local MIT-licensed `block` 0.1.6 compatibility copy under `vendor/block` fixes
an invalid foreign static in the legacy Cocoa/Metal dependency. Its patch notes
record the source, ABI-preserving changes, and removal criteria. The normal local
Cargo configuration also applies it to standalone builds; consumer lockfiles
record the local source. Account for this patch when preparing published packages.

## Local naming

The raw gpui-unofficial release remains on upstream. Development uses a
normalized capture with names such as gpui and gpui-linux. The Rust tool in
tools/release-sync applies scripts/crate-names.json and preserves original
package paths in upstream.json. See [the sync process](SETUP.md).
