# Rounded overflow control lab (#37)

Compare real GPUI controls with identical borders, radii, gradients, native text
and square child backgrounds. The left uses existing `overflow_hidden()`; the
right uses experimental rounded subtree composition. The feature is on
`issue-37/rounded-overflow`, with no change to `main`.

```sh
just rounded-overflow
just rounded-overflow check
```

Use the radius presets, solid/gradient toggle and color toggle. Click the child
header near the excluded corner and farther inside: the rounded corner must not
receive a click or hover, while the visible header remains interactive.

`check` opens a real platform window, checks excluded and visible clicks, reads back
corner pixels, changes only the radius and checks both input and output updates,
then changes the configured background. It saves `/tmp/rounded-overflow-lab.png`
and exits. `--png PATH` overrides that destination. The default capture shows the
48px outer radius with a uniform 2px border. This registered real-window check
is the regression for the original square-background leak.

For a local development build:

```sh
cargo run --offline --config .cargo/validation.toml -p xgpui-ext-lab --bin rounded-overflow-lab --features runtime-shaders,portable-shaders,wgpu -- --check
```

## Current boundary

This first native integration uses the existing bounded subtree compositor, not
the final cheap path for all rounded UI. It uses two textures per container,
supports at most four captures/frame, and each capture (including its existing
sampling allowance) must fit in 1024×1024 device pixels. Captured text uses grayscale
coverage. Circular per-corner radii are supported; this lab demonstrates uniform
border insets. Elliptical inner corners, nested captures, backdrop interaction,
foreign surfaces and inherited deferred overlays are outside the initial contract.
Nested captures involving rounded output are explicitly rejected.

The `xgpui_shader::RoundedOverflow` helper pairs prepaint and paint. Callers supply
the inner border bounds and matching inner radii. Ordinary styled elements and
`overflow_hidden()` retain legacy behavior. No `overflow_hidden_clipped()` builder
is exposed yet. The shader receives geometry per capture, so changing radii or
bounds does not compile another shader program.

Private scene metadata retains shape ownership through `Scene::replay`; CPU hitbox
metadata survives prepaint reuse. Cached views include the active input mask in
their cache key so a radius-only change cannot preserve stale hit geometry.

Verified on Apple M4 Max: real-window Metal output/input/configuration changes,
Metal and wgpu headless radius changes and replay, core mouse/scroll occlusion,
and shared shader translation to MSL/HLSL/SPIR-V. Windows/Linux and native D3D11
runtime behavior remain unverified; use applicable CI for branch checks.

See the [milestone report](../../../docs/issue-37/rounded-overflow-milestone.md)
and [core public API review](../../../docs/issue-37/api-review.md).

[Captured comparison](results/comparison.png) and [Metal check result](results/metal-check.txt).
