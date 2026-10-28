# xgpui-ext-lab

**Version 0.2.0 · Unpublished development package**

`xgpui-ext-lab` is the shared home for XGPUI's native extension labs,
benchmarks and browser demos. It contains runnable examples for exploring
rendering behavior and comparing implementations. The workloads share one Cargo
manifest and lockfile, with separate executable targets.

It lives at `apps/xgpui-ext-lab/`. The reusable extension libraries live separately
under `extensions/crates/` and do not depend on this package.

## What is in it?

| Workload | Target | Root recipe |
| --- | --- | --- |
| Image loading, mipmaps, adjustments, tiled canvases and paths | `media-lab` | `just media` |
| Brushes, gradients, meshes and layer effects | `paint-lab` | `just paints` |
| Native wide-gamut presentation | `gamut-lab` | `just gamut` |
| Color conversions and gradient comparisons | `color-lab` | `just color` |
| Native and Parley text comparisons | `text-lab` | `just text` |
| SVG rendering, scaling and scheduling | `svg-lab` | `just svg` |
| Quad rendering benchmark | `quad-bench` | `just quad-bench` |
| Color-field rendering benchmark | `color-field-bench` | `just color-field-bench` |
| Native and browser shader examples | `shader-examples` | `just shaders` |
| Browser extension gallery | `extensions-web` | `just web-build` |

The package also keeps the associated sample assets, fixtures, benchmark scripts
and recorded reports beside the workloads that use them. Measurements describe
the recorded workload and machine; use the benchmark commands to measure your own
setup.

## Why is it not published?

The manifest sets `publish = false`. This is a development and validation package,
not a library that applications need to depend on. It follows the local framework
and extension checkouts, uses repository fixtures, and can change as experiments
and diagnostic workloads evolve.

Keeping it unpublished lets the extension libraries have their own release
contracts without committing to stable lab commands, benchmark output formats or
sample layouts. Normal applications use the framework and extension libraries;
they do not pull in the labs or their development dependencies.

This does not prevent a future release on crates.io. Before publishing, we would
choose the supported tools, package the required assets, verify registry-only
dependencies and define the compatibility expectations for the commands and
outputs. The lab is deliberately absent from the library release map today.

## Build and test

Run from the repository root:

```sh
just lab-build
just lab-test
```

`lab-build` builds the native lab and benchmark executables in release mode without
opening a window. `lab-test` runs unit and compatibility tests. Tests that create
real platform windows remain opt-in through the `native-window-tests` feature.

Choose one workload with its root recipe, or select its binary directly:

```sh
cargo run --release --locked --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin paint-lab --target-dir target
```

For a bounded benchmark without a window:

```sh
just quad-bench --headless --cpu-only
```

See [Local Development](../LOCAL_DEVELOPMENT.md) for toolchains, native platform
requirements and the distinction between framework builds and lab builds.

## Native lab launch commands

These applications run on your desktop, not inside the browser. Run the commands
from the repository root after following the platform setup in
[Local Development](../LOCAL_DEVELOPMENT.md).

### Pastel waves

Open the native paint lab's animated ribbon experiment:

```sh
just paints --pastel-waves
```

### Full paint lab

Open the desktop brush, gradient, mesh and layer-effect comparisons:

```sh
just paints
```

### Native wide gamut

Compare native wide-SDR presentation with ordinary sRGB controls on a supported
display:

```sh
just gamut
```

### Native text comparison

Compare native and Parley shaping, editing and virtualized paragraphs:

```sh
just text
```

### Desktop SVG lab

Open the native SVG rendering, zoom and scheduling comparisons:

```sh
just svg
```

## Browser demos and local docs

The browser demos are targets of the same package. Their scripts select the
`web` feature without the default native targets and generate the JavaScript and
WASM bundles used by this website:

```sh
just web-build
just docs-check
just docs-serve
```

`web-build` requires the pinned `wasm-bindgen` CLI, currently `0.2.129`, and the
`wasm32-unknown-unknown` Rust target. It updates both `docs/examples/extensions/`
and `docs/examples/shaders/`. Generating the documentation alone does not rebuild
WASM. Reload the page after rebuilding.

[Open the browser demo gallery](../examples/extensions/) or
[open the shader examples](../examples/shaders/). Their hosted URLs are unchanged
by the package consolidation. The gallery also lists native-only workloads and
how to launch them locally.

## How is this different from xgpui-lab?

[`xgpui-lab`](https://github.com/gpui-luma-dev/xgpui-lab) is the separate desktop
application used to exercise XGPUI together with the Luma SDK. It has its own
repository and application shell.

`xgpui-ext-lab` is the development package inside this repository: focused native
labs, repeatable benchmarks and the source for this website's WASM demos. The two
names refer to different applications and development roles.

[Browse the package source](https://github.com/gpui-luma-dev/xgpui/tree/main/apps/xgpui-ext-lab).
