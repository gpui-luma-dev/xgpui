# Clip-chain feasibility probe (#37)

An isolated feasibility lab, registered in `labs.toml`. It has its own Cargo package
and does not modify the production renderer. No production GPUI
API changes. See the [source-grounded proposal](../../../docs/issue-37/feasibility.md).

The same bounded rounded-rectangle/ellipse intersection is evaluated by a CPU reference,
native Metal fragment pipeline, wgpu storage/uint-texture fragment pipelines and a
browser WebGL2 demo. Coverage is the mean of 16 **joint intersection** samples, not a
product of individually antialiased masks. Eight shapes is an experimental cap.

## Run

Use `just clip-chain-probe wgpu` for GPU checks, `just clip-chain-probe test` for CPU
checks, and `just clip-chain-probe browser` to serve the generated demo. Run the wgpu
task first to generate its fixtures. `just clip-chain-probe metal` runs the native
Metal variant on macOS after generating fixtures.

From the repository root (requires cached dependencies for `--offline`):

```sh
cargo test --offline --manifest-path apps/xgpui-ext-lab/clip-chain-probe/Cargo.toml
cargo run --offline --release --manifest-path apps/xgpui-ext-lab/clip-chain-probe/Cargo.toml -- /tmp/issue-37-clip-results
```

The run needs access to a real GPU; a sandbox without a Metal adapter cannot run it.
The lab’s standalone Cargo.lock pins the experiment's dependencies independently from the
main workspace. No workspace build or generated release manifest changes are needed.

On macOS, run the native Metal variant against those generated CPU fixtures:

```sh
swiftc -module-cache-path /tmp/issue-37-swift-cache apps/xgpui-ext-lab/clip-chain-probe/metal.swift -o /tmp/issue-37-metal-probe
/tmp/issue-37-metal-probe apps/xgpui-ext-lab/clip-chain-probe/clip.metal /tmp/issue-37-clip-results
```

The generated directory contains `demo.html`, `fixtures.js`, `names.txt`, binary shape
and reference files, and numeric results. Open the browser demo through a local server:

```sh
python3 -m http.server 8737 --bind 127.0.0.1 --directory /tmp/issue-37-clip-results
```

Visit [the local demo](http://127.0.0.1:8737/demo.html). It runs WebGL2 readback assertions
before showing the controls. With nesting disabled and radius 90, click logical point
(50,46): rounded mode rejects it; rectangle mode hits child 1. Point (135,60) hits the
visible part of child 1 in rounded mode. Pointer coordinates scale with the canvas.

## Scope and interpretation

- 12 fixtures: unclipped, rectangle, per-corner rounded, ellipse, nested, coincident,
  depth eight, oversized radii/asymmetric border insets, disjoint and three DPI scales.
- The oversized case uses CSS common-factor outer-radius normalization as a geometry
  experiment. GPUI currently clamps corners independently; the proposed styled helper
  preserves GPUI's effective radii. The other ordinary rounded fixtures fit both rules.
- Metal/wgpu assert a maximum error of one sample (0.0625) against CPU 4×4 coverage.
  The captured run is exact. WebGL2 allows one RGBA8 code value and observed zero.
- CPU also computes 32×32 coverage, reporting mean and maximum error. It shares the
  shape formula, so this tests raster agreement rather than independently proving
  shape normalization. Scale fixtures use a fixed 96×96 observation viewport.
- CPU tests include invalid geometry/depth rejection, geometric border insets,
  corner hit rejection and coincident-edge idempotence with a failing product-alpha
  negative control. Production empty clips should cull; this probe rejects them.
- Browser content is one raster/text texture plus procedural shader output. Its CPU
  predicate controls toy child clicks. This is not GPUI text/image/input integration,
  Naga-generated WebGL, WebGPU runtime validation or a native GPUI lab.
- Times include submission, synchronization and readback at 96×96. They are smoke
  diagnostics, not comparative GPU benchmarks or ordinary UI overhead measurements.
- Native D3D11 and Windows/Linux runtime behavior remain unverified. No production
  fallback, cache budget or AA quality claim is made.

Captured [results](results/) and [demo screenshot](results/demo.jpg) describe the observed
Apple M4 Max run. `results/environment.txt` records the configuration.
