# Instanced portable-content companion

`portable-content-instanced` is a separate performance reference. The original
`portable-content` binary and its source remain unchanged for atomic-group work.
Both windows use the same 32×16 grid, 512 warm/cool colors, 2px gaps, 4px corners,
window size, density toggle, clipping and opacity controls.

The companion records one main-pass shader instance per visible rectangle. A
shared program lets the unclipped grid render as one 512-instance shader draw,
plus ordinary UI/background rendering. It allocates no per-cell producer images.
Clipping may cull fully hidden instances. The original lab continues to exercise
512 independent solid/copy producers and retained group presentation.

From the repository root:

```sh
cargo run --release --manifest-path apps/xgpui-ext-lab/Cargo.toml \
  --bin portable-content-instanced --features runtime-shaders
```

Open the original alongside it by changing `--bin` to `portable-content`. Press D
or click the density control for 512 ↔ 1, R for warm/cool replacement, O for opacity,
and C for clipping. Compare response to rapid toggles and resizing. Expect the
same layout/colors/coverage and complete density transitions; the instanced app
has no asynchronous producer generation to wait for.

## Repeatable headless comparison

```sh
cargo run --release --manifest-path apps/xgpui-ext-lab/Cargo.toml \
  --bin portable-content-instanced --features runtime-shaders -- \
  --benchmark --frames 60 --warmup 10 \
  --json /tmp/portable-content-instanced.json
```

This runs both rendering paths through the platform headless renderer. The
portable-group half uses the platform's atomic-group implementation (Metal, wgpu
or D3D11). The D3D11 implementation is cross-compiled; its Windows GPU runtime
acceptance remains pending.
The comparison uses a 1024×512 physical-pixel canvas at scale 1; use `--width` and
`--height` to change it. Width must be 320..=4096 and height 160..=4096. Four cases
cover warm replay of 512 cells, replay of one cell, alternating warm/cool replacement
of 512 cells, and alternating 512 ↔ 1 density. Output is JSON on stdout and,
optionally, in the requested file. Each row includes median, p95, min and max.

- `scene_build_us`: CPU construction and scene ordering.
- `prepare_ready_us`: CPU description plus queued producer work and 1ms readiness
  polling. It is a wall-clock readiness measurement, not GPU execution time.
- `cpu_encode_submit_us`: ready-scene encoding/submission without waiting for the
  GPU or reading pixels. It excludes the two preceding phases.
- `validation_render_complete_readback_us`: a second, untimed-for-encoding render,
  completion and pixel readback that drains the prior submission and bounds queue
  depth. It includes readback overhead and must not be interpreted as GPU duration.

The benchmark asserts exactly one shader batch and the expected 1/512 instance
count for every instanced sample. An interior pixel is checked on every iteration
to reject blank output or a stale palette. Final images in each case must match the
portable reference within two 8-bit channel levels. This allowance covers the
portable path's intermediate RGBA8 quantization. A failed comparison exits with
an error instead of producing a successful report.

These are two complete rendering paths, including different source-production
costs; this is not an isolated draw-call microbenchmark. The result does not measure
display pacing, FPS, input latency or GPU timestamps. Compare optimized builds on
the same machine, dimensions and sample counts, with the same validation settings;
repeat runs before treating a change as a regression. The report records source
revision, OS/architecture, build profile and workload settings. Source revision
identifies HEAD; uncommitted local changes are not fingerprinted.

## Checks

```sh
cargo test --manifest-path apps/xgpui-ext-lab/Cargo.toml \
  --bin portable-content-instanced --features runtime-shaders
```

The CPU tests verify one-batch 1/512 scenes and the original workload layout/palette.
The headless command additionally exercises both GPU paths and checks pixel parity.
