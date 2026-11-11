# XGPUI extension lab

[Read the lab guide](https://xgpui.dev/extension-lab.html) for the package
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
just lab-media                         # Open the media lab.
just lab-shader                        # Open the shader examples.
just lab-quad-bench --headless --cpu-only   # Run a bounded CPU benchmark.
just web-build                         # Build and package both browser demos.
```

Each extension has a `lab-*` recipe: `lab-canvas`, `lab-media`, `lab-shader`,
`lab-paint`, `lab-gamut`, `lab-color`, `lab-text` (also `lab-parley`) and `lab-svg`.
Use `just lab-media --help`, or the corresponding lab's `--help`, to see all app
options. Every recipe forwards arguments, including quoted file paths. The old
short names and `*-lab` names remain aliases. Native launch recipes enable runtime shaders,
portable shaders and wgpu; platform-specific capabilities still apply.
Target names and output executable names are unchanged. `lab-color` includes
Swatches/Chromaticity tabs and accepts `--chromaticity`; its old chromaticity
recipes now run locally without a sibling checkout. See [Color lab](color-lab/README.md).

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
