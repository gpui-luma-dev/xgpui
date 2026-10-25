# Instanced WGSL examples

A local native app for issue #21, with companion WASM builds. The native app is
the primary demo: select all six examples inside one window.

Run here with `just ui`, following the other lab apps. Bare `just` lists recipes.

```sh
just ui                    # Local native app; choose examples in the window
just minimal               # Custom fragment shader
just glow                  # 100 animated badges
just particles             # Adjustable particle workload
just composition           # Clipping, opacity and ordering
just shadows               # Native vs programmable shadows
just borders               # Soft animated border on a light panel
just --set profile release ui  # Optimized native app
```

From the repository root:

```sh
just --justfile apps/shader-examples/justfile ui
```

The in-app selector switches between:

1. **Minimal:** custom fragment function with a typed rounded-box instance.
2. **Glow:** 100 animated badges sharing one program.
3. **Particles:** 100, 1,000, 5,000 or 10,000 particles with pause and opacity controls.
4. **Composition:** transparent overlapping instances, a native gold quad between
   shader draws, an inset cyan clip frame, a clipping toggle and inherited opacity.

5. **Shadows:** compare native single-color shadows with gradient-color shadows;
   cycle blur, spread, offset, falloff, and inset/outset mode. Both cards have
   clickable children; card and parent opacity controls verify composition.
6. **Borders:** an animated Generate pill on a light panel demonstrates blurred
   orbiting highlights. Pause/Resume freezes and resumes the 2.5-second rotation.

The existing `apps/extensions-web` gallery is not modified.

## Companion WASM build

```sh
# From the repository root; wasm-bindgen-cli must be 0.2.129.
bash apps/shader-examples/build.sh
python3 apps/shader-examples/serve.py
```

Open http://localhost:8083/ and choose an example and backend. WebGPU needs a
supporting browser/adapter and localhost or HTTPS. WebGL2 is explicitly available;
errors appear on the page and in the console. No worker, shared memory or atomic
WASM build is used. For performance comparisons, build with `--release`.
The build writes this app's ignored `dist/` and the docs companion at
`docs/examples/shaders/`.

All effects use the same typed shader API on Metal, D3D11 and wgpu. A compatible
run is eligible for one instanced draw; composition intentionally splits at the
native quad. The displayed redraw interval includes frame scheduling and CPU
work, not GPU time or measured draw counts. Use a GPU capture to inspect submission.

Interactive acceptance starts in the local app: switch through all six examples,
resize, pause/resume, toggle opacity and adjust particle counts. Repeat the same
checks in the WASM companion on forced WebGPU and WebGL2. In composition, the gold quad
must sit above red and below green/blue. Toggle clipping to reveal the full shapes
without moving them; the cyan frame stays fixed. Confirm no
console errors. Native or translation checks alone do not prove browser behavior.

See [the v2 contract](../../docs/extensions/instanced-wgsl-v2.md) for authoring,
ABI validation, transport limits and color semantics.

Validated locally on 2026-10-25 in the in-app Chromium browser: minimal shader,
glow, particles and composition rendered across forced WebGPU and WebGL2;
pause, count and opacity controls worked. The optimized 5,000-particle page
showed approximately 16.7 ms redraw intervals on both backends in this session.
This is a browser scheduling observation, not GPU timing or a 120 FPS claim.
WebGL2 startup can report the existing dual-source-text fallback and a resize
poll timeout; the tested examples continued rendering without console errors.
The shader crate's 23 tests passed, including Metal/wgpu pixel checks. The native
example app compiled for macOS and cross-compiled for Windows; D3D11 driver
execution remains to be checked on Windows.

Native app acceptance on 2026-10-25: launched the macOS Metal app, switched
between Minimal, Glow and Particles using the in-app selector, and verified the
particle count and pause controls. Use `--release` before comparing particle
performance; the default development build prioritizes iteration speed.

The Shadows example was checked on native Metal and both browser WebGPU and
WebGL2. The helper update was verified first on native Metal, then on both
browser backends: child clicks, card opacity and inset/outset controls worked
without browser console errors. Metal/wgpu tests also verify paint order and
ancestor clipping. This is
an opt-in `painted().shadow_paint(layer)` helper backed by `ShadowPainter`; native GPUI `.shadow(...)` keeps its current API.
See [richer shadow authoring](../../docs/extensions/instanced-wgsl-v2.md#richer-shadows)
for gradient stops, paint order, automatic effect bounds and the analytic blur contract.

Borders category acceptance: the native Metal demo was checked before rebuilding
WASM. Generate clicks and pause were then checked on forced WebGPU and WebGL2
(`backend=webgl`); both rendered without console errors. The helper is entirely
in `xgpui-shader`; no further GPUI core/API changes were made.
