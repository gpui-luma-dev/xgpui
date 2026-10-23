# shader-lab

Local native GPU correctness and performance app for
[#7](https://github.com/gpui-luma-dev/xgpui/issues/7). The first slice renders a
solid-color offscreen attachment and the patskovn-based Hermite mesh producer
through the retained-content integration. Mesh is the default effect. This mesh
increment requires no further GPUI or renderer API changes.

## Run

From the repository root, with the normal macOS/Xcode build prerequisites:

```sh
cargo run --locked --config .cargo/validation.toml \
  --manifest-path apps/shader-lab/Cargo.toml \
  --target-dir target
```

From this app directory, `just` lists all recipes:

```sh
just ui                         # Paused morph
just animate                    # Four-second transition
just slow                       # Forty-second transition
just window 1440 1080            # Custom window size
just capture                    # /tmp/shader-lab.{png,json}
just submit                     # Cached mesh submission timing
just baseline                   # Native solid baseline
just animated-submit            # Changing mesh snapshots
just minimal                    # 2x2 grid, two samples per edge
just quality                    # 3x3 grid, 32 samples, odd extent
just solid --animate            # Headless changing solid
just check                      # Format, tests, Clippy
just metal-test                 # Tests with Metal API validation
just args                       # Every CLI flag and default
just --set profile release submit # Optimized measurement
```

Recipes build before running and forward extra flags, for example
`just headless --grid 3 --subdivisions 32 --frames 30 --json /tmp/mesh.json`.
All file arguments resolve relative to the repository root. From the root, use
`just --justfile apps/shader-lab/justfile <recipe>`.
The UI shows one mesh; effect, baseline and instance-count workloads are headless.

The default window is 1280x960 logical pixels, with 960x640 example panels and
room for the controls. `--width` and `--height` override the window size.

The UI focuses on a mesh morph between a warm, upper-left keyframe A and a
cool, lower-right keyframe B. White circles and connecting lines show the actual
control points. Play/Pause runs A → B once; Replay starts again. The progress
slider scrubs and pauses. **Slow animations** stretches the same transition from
4 seconds to 40 seconds without jumping. Start playback with `--animate`; add
`--slow-animations` for slow playback. Performance/baseline options remain in the
headless CLI.

## Correctness and timing

```sh
# Real Metal pixel assertions, followed by timed frames and an optional PNG.
cargo run --locked --config .cargo/validation.toml \
  --manifest-path apps/shader-lab/Cargo.toml \
  --target-dir target -- --headless --frames 60 --warmup 10 \
  --png /tmp/shader-lab.png --json /tmp/shader-lab.json

# CPU encoding/submission measurement without per-frame GPU readback.
target/debug/shader-lab --headless --submit-only --width 640 --height 480 --count 16 --frames 120 --warmup 20
# Native-solid UI baseline (not a mesh-equivalent rendering).
target/debug/shader-lab --headless --submit-only --width 640 --height 480 --count 16 --frames 120 --warmup 20 --baseline
# Force new immutable mesh snapshots each frame (pipelines stay shared).
target/debug/shader-lab --headless --submit-only --width 640 --height 480 --count 16 --frames 120 --warmup 20 --animate

# Explicit quality and topology workloads; --effect solid retains the old mode.
target/debug/shader-lab --headless --grid 2 --subdivisions 2 --frames 10
target/debug/shader-lab --headless --grid 3 --subdivisions 32 --animate --width 641 --height 479 --frames 30

# Automated GPU integration tests.
cargo test --locked --config .cargo/validation.toml \
  --manifest-path apps/shader-lab/Cargo.toml --target-dir target
```

Headless runs first assert pixel values for inherited clipping, local rounded
coverage, straight texture alpha, element opacity, overlapping native primitives,
scene replay, shared first-use output, static caching, resize, removal, and retry
after an invalid producer aborts preparation. Fully clipped records are rejected.
The same Metal preparation/composition path serves the UI and headless renderer.

Mesh checks compare GPU pixels with an independent CPU Hermite basis evaluator
for regular 2x2, rectangular 3x2, and deformed 3x3 grids. They reject coverage
holes and exercise snapshot preservation, shared pipeline reuse, resize, minimum
subdivisions, duplicate first-use records, and retry after aborted preparation.

JSON reports include build profile, workload, device name when a custom producer
is used, physical size, scale, warmup, frame count, median/p95/max times, and
current cached texture bytes. The latter excludes GPUI targets, driver memory,
and earlier in-flight outputs. Equal-size instances share one producer texture.
Mesh reports also include grid size, samples per edge, triangles/output, estimated
geometry bytes/generation, output generations, and pipeline compilations. Counts
include warmup; normal animated related snapshots should compile pipelines once.

Default rendering timing includes GPU completion wait and CPU readback.
`--submit-only` measures CPU encoding/submission and possible queue backpressure;
a final GPU drain is excluded. Neither mode measures GPU-only time, display
cadence, or FPS. Headless changing colors use a deterministic frame-index sweep;
native playback uses elapsed time and starts paused unless explicitly requested. Headless mesh
animation uses deterministic geometry/color snapshots at nominal 1/60-second
steps so runs remain reproducible.

## Initial local smoke measurements

Apple M4 Max, macOS arm64, debug build, 640x480 physical pixels, scale 1,
16 instances, 20 warmup frames and 120 measured frames, submit-only mode:

| Workload | Median CPU encode/submit (ms) | p95 (ms) |
| --- | ---: | ---: |
| Native solid baseline | 0.024 | 0.041 |
| Cached solid output (before mesh increment) | 0.035 | 0.045 |
| Changing solid output | 0.050 | 0.063 |
| Cached 3x3 mesh, 18 samples/edge | 0.032 | 0.038 |
| Animated 3x3 mesh, 18 samples/edge | 0.072 | 0.172 |

These are initial development smoke measurements, not release performance budgets
or GPU costs. Use repeated optimized runs before drawing performance conclusions.
Static equal-size instances generated one 76,800-byte producer texture. The table
includes solid integration and mesh smoke runs. The mesh uses 2,312 triangles
and an estimated 265,136 geometry-buffer bytes per output generation. The static
mesh generated one output; the 140 animated snapshots generated 140 outputs
while compiling pipelines once.

## Scope and compatibility

Native Metal on macOS and D3D11 on Windows. The extension also offers an opt-in
wgpu backend, exercised separately in paint-lab. Windows runtime validation is
still pending; see [native D3D11 status](../../extensions/xgpui-ext/docs/native-dx11-paints.md). No
high-gamut framebuffer, arbitrary subtree effects, grain, or media engine is
included. Mesh colors use encoded-sRGB Hermite interpolation, not Oklab.
Existing view APIs are preserved; public `Primitive` and
`PrimitiveBatch` enums gain a custom variant, breaking downstream exhaustive
matches. Root manifests and existing GPU-facing primitive layouts are unchanged.

Still needed in later milestones: GPU timestamps, freeform control-point dragging,
animated resource pooling, optional grain, transparent-window/device-loss checks,
and broader multi-window stress tests. The current UI edits preset parameters;
the library accepts arbitrary validated control-point grids.
The root build checks only the platform libraries supported by the selected
host; it does not validate Windows/Linux execution.
