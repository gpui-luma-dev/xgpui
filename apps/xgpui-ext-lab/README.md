# XGPUI extension lab

[Read the lab guide](../../docs/extensions/extension-lab.md) for the package
overview, target list, unpublished status and browser workflow.

An unpublished development package containing native labs, benchmarks and browser
demos. It uses one Cargo manifest and lockfile. Extension libraries stay in
`extensions/crates/`; their dependency graphs do not include this package.

The package is version `0.2.0` with `publish = false`. Publishing can be enabled
later after checking the package's bundled assets and consumer requirements.

From the repository root:

```sh
just lab-build                         # Build native labs and benchmarks.
just lab-test                          # Run unit/compatibility tests.
just media                             # Open the media lab.
just shaders                           # Open the shader examples.
just quad-bench --headless --cpu-only   # Run a bounded CPU benchmark.
just web-build                         # Build and package both browser demos.
```

Existing recipes also include `paints`, `gamut`, `color`, `text`, `svg`, and
`color-field-bench`. Target names and output executable names are unchanged.

Select one executable directly:

```sh
cargo run --release --locked --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin paint-lab --target-dir target
```

Native binaries use the default `native` feature. Browser scripts build only the
selected WASM binary with `--no-default-features --features web`; generated JS/WASM
names and the hosted `/examples/extensions/` and `/examples/shaders/`
destinations remain unchanged. Source, fixtures and benchmark reports are grouped
by target under this package.

The `native_windows` integration target is opt-in via `native-window-tests` because
it creates real platform windows. Unit and compatibility recipes do not enable it.

`portable-content` remains the independent-producer/atomic-group lab.
Its separate [instanced companion](portable-content-instanced/README.md) provides
the same window controls and a headless JSON performance comparison:

```sh
cargo run --release --manifest-path apps/xgpui-ext-lab/Cargo.toml \
  --bin portable-content-instanced --features runtime-shaders
```

## Repository-wide validation and guided UI acceptance

From the repository root, use `just test-all` for the host-supported automated
native matrix, `just test-web` for Wasm/docs plus Chromium startup/resize checks,
and `just test-ui` for sequential native lab checklists with operator-recorded
results. Append `--list` to inspect commands without running them. Each run saves
JSON/Markdown results, logs and relevant captures in `target/test-runs/` and
separates passed, failed, unsupported and untested checks. Launching/closing a
window alone is never a UI pass. These commands do not include the separate
xgpui-lab consumer or establish other-platform acceptance.

See [test matrix, filters, report semantics and browser setup](../../docs/LOCAL_DEVELOPMENT.md#run-the-repository-test-matrix).
