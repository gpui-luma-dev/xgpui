# SVG lab — experimental extension sample

A native test app for [`xgpui-svg`](../../../extensions/xgpui-ext/crates/xgpui-svg/README.md).
It compares a fixed full-color texture, progressive SVG rerasterization and GPUI
paths, and includes a multi-element gallery for shared scheduling experiments.

## Run

From the repository root:

```sh
just --justfile apps/xgpui-ext-lab/svg-lab/justfile ui
just --justfile apps/xgpui-ext-lab/svg-lab/justfile test
just --justfile apps/xgpui-ext-lab/svg-lab/justfile lint
```

Equivalent Cargo invocation:

```sh
cargo run --offline --locked --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin svg-lab --target-dir target
```

Remove `--offline` if dependencies are not cached. Use release builds for useful
timings. Each app/crate has its own locked manifest.

## Comparison and loading

The built-in ribbon/leaf scene has cubic curves, two gradients, fine strokes,
rectangular clipping and a drop shadow. GPUI paths approximate the shadow with
layered strokes; gradients also differ in bounds/interpolation. This compares
capabilities, not pixel equivalence.

Wheel over any panel to zoom around the pointer (25–1600%); drag to pan all three.
Presets preserve the current pan target; **Reset view** recenters at 100%.
Clipping/shadow toggles apply to the built-in scene. The fixed texture starts at
320×240 pixels; the progressive element accounts for display density, preview
and settle policy. Pan reuses retained detail.

**Open SVG…** reads/parses/renders a base image in the background, keeping the
last good scene on failure/cancel. Imported artwork keeps its aspect ratio,
normalized to a 320-logical-pixel longest edge. Both raster panels show it; the
native panel has no arbitrary SVG-to-path conversion. **Built-in scene** restores
the comparison. Light/dark backdrop controls help inspect transparent artwork.

Select 16/32 MiB per progressive output. Requested, budgeted and displayed
resolutions and status expose limits. Parse/raster labels are individual CPU
wall-time observations, excluding GPU upload/presentation. Synchronous resvg work
cannot be interrupted. File size is limited to 32 MiB; linked sibling resources
and file watching are not provided. SVG scripting/animation is not executed.

## Included artwork and offline gallery

The [`samples`](samples/ATTRIBUTION.md) directory includes the camera, grapes,
metal and dragon artworks with creator/source/license notices, plus an original
`ribbon-leaf.svg` export. The portrait test fixture is also included. Third-party
artwork retains its own license; it is not blanket-relicensed as Apache-2.0.

```sh
just --justfile apps/xgpui-ext-lab/svg-lab/justfile gallery
```

This uses only the committed sample subset, repeated across 24 independent
image elements, with a shared 64 MiB/two-job pool. No artwork download is needed.
Scroll normally; Command-wheel zooms on macOS. Visible rows plus one overscan row
are mounted by default; `--gallery-eager` keeps every tile mounted. The gallery
loads available recognized corpus filenames from the supplied directory; an
empty/unrecognized directory fails. Shared parsed trees do not share raster outputs.

```sh
target/release/svg-lab --gallery apps/xgpui-ext-lab/svg-lab/samples --scheduler-mib 128 --scheduler-jobs 2
```

The optional historical benchmark corpus contains ten files, including artwork
with noncommercial, mixed or unresolved terms. Those additional files remain
fetch-only. The committed subset is not the same workload as the recorded
historical eight-artwork gallery. See [attribution and exclusions](samples/ATTRIBUTION.md)
and the [measurement/reproduction guide](bench/README.md).

## Extension contract and evidence

The progressive panel uses `svg_image` with a retained document, shared camera,
policy and optional diagnostics callback. It owns no manual queue or timer.
Applications can use the element without the lab UI; shared scheduling is opt-in.
See the [extension API](../../../extensions/xgpui-ext/crates/xgpui-svg/README.md).

Aggregate budgets bound estimated image reservations, not total RSS/physical
footprint. Lower budgets reduce resolution and admission can temporarily leave
tiles empty. The native measurements capture host redraw cadence and process
memory; they do not measure input-to-photon latency or all system GPU memory.

[Results and reproduction](bench/README.md) ·
[Experimental follow-ups](../../../extensions/xgpui-ext/crates/xgpui-svg/FOLLOW_UPS.md)
