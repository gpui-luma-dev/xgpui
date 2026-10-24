# Build xgpui

Run these commands from the repository root:

```sh
cargo build --locked
```

The root workspace contains a private build harness that depends on all captured
libraries supported by the selected target. On macOS, this compiles 23 captured
crates, including GPUI, the Apple/Metal and macOS backends, the wgpu renderer,
macros, HTTP clients, and supporting utilities. Metal shaders are compiled at
build time by default. The harness has no runtime behavior and launches nothing.

The complete release contains 26 crates. The Linux, Windows, and browser
backends are selected only for their respective targets, rather than counted
as native macOS builds. Those targets are not validated by the macOS build.
Optional features and standalone test, benchmark, example, or binary targets
are not all enabled by a root library build.

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

## Standalone consumer labs

The [lifecycle lab](../apps/lifecycle-lab/README.md) exercises window closure,
entity ownership, subscriptions, and action routing through public GPUI APIs.
It has a separate workspace and lockfile; it is not part of the root build.

```sh
cargo test --locked --config .cargo/validation.toml \
  --manifest-path apps/lifecycle-lab/Cargo.toml --lib --target-dir target
```

Its focused macOS CI job builds the native executable, runs headless tests, and
checks formatting and Clippy. Native manual checks remain separate.
See [API coverage](API_COVERAGE.md) for the other consumer labs and evidence gaps.

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
The nested `transformed-crates` staging copy is not built a second time.

`Cargo.toml` and `tools/build-all/Cargo.toml` are generated development files;
they are not overwritten by upstream imports or downstream normalization. After an import adds or removes
packages, regenerate them and update the root lockfile on the development or
integration branch:

```sh
cargo run --locked --manifest-path tools/release-sync/Cargo.toml -- build-manifests
cargo generate-lockfile
cargo build --locked
```

CI checks `python3 scripts/update_build.py --check` to detect a stale harness.
Review the platform classification in that script when upstream adds a backend.
Keep the root Cargo.lock committed for reproducible native builds.

The generated root manifest restores the compatibility dependencies listed in
`.cargo/validation.toml`. Besides Zed's Proptest revision, compiling the macOS
utility crate requires Zed's async-process fork: its process launcher calls
`Child::adopt_raw_pid`, which the registry dependency does not provide. The fork
revision is taken from the original Zed release's workspace patch. These
overrides repair extraction omissions without editing captured source.

## Local naming

The raw gpui-unofficial release remains on upstream. Development uses a
normalized capture with names such as gpui and gpui-linux. The Rust tool in
tools/release-sync applies scripts/crate-names.json and preserves original
package paths in upstream.json. See [the sync process](SETUP.md).
