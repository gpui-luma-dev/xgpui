# Portable content window

Run on macOS from the repository root:

```sh
MTL_DEBUG_LAYER=1 cargo run --manifest-path apps/xgpui-ext-lab/Cargo.toml \
  --bin portable-content --features runtime-shaders
```

The app uses `ContentSource`, `ContentDraw`, stable presentation slots, and
`Window::paint_content`. It imports no backend handles. Each of 512 cells describes
a solid and a dependent image copy. Producer textures stay on the GPU; the window
samples only completed outputs. No CPU pixel readback runs in this app.

What to look for:

- The warm 512-cell grid appears automatically after startup.
- **R** or **Replace warm / cool** changes all cells; completion needs no extra click.
- **D** or the density control switches 512 → 1 → 512 without freezing.
- **O** toggles opacity once; **C** clips the destination without changing its source mapping.
- Resize repeatedly while replacing content: the UI stays responsive and the newest
  generation eventually appears. A pending resize can stretch the previous valid image.
- Cmd-Q exits. Controls remain available while GPU work runs.

Automated pixel/lifecycle acceptance (real Metal GPU required):

```sh
MTL_DEBUG_LAYER=1 cargo test --locked \
  --manifest-path gpui/crates/gpui-apple/Cargo.toml \
  --test portable_content_presentation --features runtime_shaders,test-support \
  -- --test-threads=1 --nocapture
```

Expect **12 passed, 0 failed, 1 ignored**. The ignored helper runs through the dense
subprocess test. Dense logs must show verified pixels, `producer_readbacks=0`, and
`max_in_flight <= 2`. Tests read only the final compositor validation target.
The original `metal-producer-regression` app now covers migrated WideColor and a
distinct-output stress case. Other producer families still need migration;
issue #110 remains open.


On Windows, run the same `cargo run` command without the Metal environment
variable or runtime-shaders feature. The grid uses native D3D11 portable execution
and GPU-only presentation. Apply the same 512 → 1 → 512, replace, opacity, clip
and resize checks above. [Windows acceptance commands](../../../docs/extensions/native-dx11-paints.md)
include the native pixel/lifecycle suites; Windows runtime results remain pending.


Completion redraws now follow the normal macOS display link or X11 refresh loop.
After replacing colors, stop all input: the completed grid must appear without
another click. Repeat density switches, resize/replacement and close during
pending updates. Metal must keep advancing admitted generations while newer
inputs coalesce. The gate regression is part of the shader crate's
`portable_gradient` integration test (features `portable-shaders,gpui-apple/runtime_shaders`).
