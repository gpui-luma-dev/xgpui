# Performance Study

Pastel waves (`paint-lab --pastel-waves`): nine translucent antialiased
mesh-gradient ribbons over a five-point freeform background and nine radial
tint fields, with per-ribbon veils, variable-width rims, lip highlights, five
thread lines and hashed grain. Target: sustained 120 Hz with headroom.

Platform: Apple M4 Max, macOS, release builds, 2560 × 1562 device pixels
(1280 × 781 logical at 2×). GPU times come from Metal command buffer
`GPUStartTime`/`GPUEndTime`; "saturated" times come from back-to-back offscreen
`render_scene_to_image` with an empty-scene readback baseline subtracted.

## Baseline

| Renderer | Implementation | Cost |
| --- | --- | --- |
| `paths` | Brush fills over native path coverage; geometry rebuilt per frame | ~177 ms/frame (debug), ~6 FPS |
| `wgsl` | One instanced WGSL draw; each fragment evaluates all 9 ribbons | 6.0 ms GPU (saturated) |
| `metal` | `CustomContent` producer: compute pass per column + one fragment pass | 6.3 ms GPU per frame at 120 Hz |

Frame breakdown at 120 Hz (Metal producer, pre-optimization):

| Component | Mean | p99 |
| --- | --- | --- |
| Frame callback, total | 4.84 ms | 6.09 ms |
| `nextDrawable` wait | 3.63 ms | 5.03 ms |
| CPU work excluding wait | 1.22 ms | 2.93 ms |
| Command encoding | 0.29 ms | 0.46 ms |
| GPU, producer start → main pass end | 6.29 ms | 7.90 ms (max 8.78) |

The workload is GPU-bound: 6.3 of 8.33 ms, with the worst frames over budget.
CPU work and encoding are an order of magnitude below the limit.

## Problem 1: frame rate does not expose cost

**Problem.** FPS is quantized to the display: 60 on a 60 Hz panel, 120 on a
ProMotion panel, for every frame that completes in time. It reports no
headroom.

**Measurement pitfalls.**

- The producer command buffer and GPUI's main pass overlap on the GPU (4.2 ms
  overlap measured). Summing per-buffer spans gives 10.5 ms for a frame that
  completes at 120 Hz. The valid span is producer `GPUStartTime` → main pass
  `GPUEndTime`.
- Frame callback time includes `nextDrawable` blocking (vsync back-pressure),
  which is idle time, not work.
- Apple GPU DVFS lowers clock under light load. A shader that does less work
  can report the same milliseconds at a lower clock: the culled WGSL shader
  showed 6.3 ms in-app before and after a change that removed ~20% of its work.

**Solution.** Measure three separate quantities: CPU work net of drawable wait,
GPU span across all command buffers of a frame, and saturated GPU cost with the
clock pinned high by back-to-back offscreen rendering. Run old and new
variants alternately, three repetitions each; run-to-run noise was ±0.3 ms.

## Problem 2: per-pixel evaluation of per-column data

**Problem.** Each ribbon edge is a Catmull-Rom spline in `u` (seven knots),
plus sinusoidal drift and, for foreground ribbons, a fold strength computed
from a finite-difference second derivative (three more spline evaluations).
None of it depends on `y`, yet a single-pass shader recomputes it for every
pixel: 18 spline evaluations, 36 trig calls and 12 fold-strength splines per
fragment.

**Solution.** A native producer with two passes:

1. `pastel_columns` (compute, one thread per pixel column) evaluates every
   ribbon and writes two `float4` per ribbon per column:
   `[top, d/du top, bottom, d/du bottom]` and
   `[fold strength, y_min, y_max, 0]` (drift included). The buffer is
   `width × 9 × 2 × 16` bytes, 720 KB at 2560 columns.
2. `pastel_fragment` (one oversized triangle) reads its column's entries and
   only shades.

The producer implements `CustomContent::prepare`, records both passes into
the host's preparation batch and returns an output texture that GPUI
composites at scene order. It never submits or presents.

## Problem 3: every pixel shades every ribbon

**Problem.** Each ribbon contributes up to five layers per pixel (fill, veil,
rim, lip, threads), each with one or two antialiased edge tests (`rsqrt`), a
mesh-color interpolation and, with grain, an integer hash and a soft-light
blend. A ribbon covers a band a fraction of the target's height, but all nine
are shaded everywhere.

**Solution: exact row culling.** Per ribbon and column, compute a device-pixel
interval `[y_min, y_max]` outside which all five layers have zero coverage, and
skip the ribbon there. Because `over(dst, c, 0) = dst` exactly, the skip is
lossless provided the interval is conservative.

- Edge coverage is `clamp(0.5 + d)` with perpendicular distance
  `d = (y − y_edge) / √(1 + s²)`, `s` the device-space slope. Coverage reaches
  0 or 1 at a vertical offset of `0.5 √(1 + s²)`.
- Lip and thread bands lie between top and bottom with linearly interpolated
  value and slope, so `|s_f| ≤ max(|s_top|, |s_bottom|)`; one stretch factor
  bounds every interior band.
- The rim spans `−0.2w … 0.8w` around the top edge (`w ≤ 2.6` logical px) along
  its normal; the veil extends `reach ≤ 0.55` of the ribbon thickness below the
  top edge.
- One pixel of margin covers rounding differences between passes.

```text
y_min = min(top, bottom) − 0.5·k_max          k = √(1 + s²)
        min(…, top − (0.2w + 0.5)·k_top)      foreground ribbons
y_max = max(top, bottom) + 0.5·k_max
        max(…, top + (0.8w + 0.5)·k_top, top + reach·thickness)
skip if y < y_min − 1 or y > y_max + 1
```

The Metal producer computes the interval in the column pass; the WGSL shader
computes it inline after the edge evaluation and before the layer work, using
the maximum fold strength so the fold calculation is also skipped.

**Result.**

| | Before | After |
| --- | --- | --- |
| Metal, GPU per frame at 120 Hz (mean / p99 / max) | 6.4 / 8.0 / 10.2 ms | 4.2 / 4.5 / 4.6 ms |
| WGSL, saturated | 6.03 ms | 4.84 ms (−20%) |
| Metal, saturated | 5.11 ms | 4.59 ms (−10%) |
| Output vs. original | — | ≤ 1/255 on ≤ 79 of 4.0 M pixels |

## Problem 4: remaining cost after culling

Ablation of the culled WGSL shader (saturated, each part removed in turn):

| Component | Cost |
| --- | --- |
| Full-target draw and write, fixed | ~1.2 ms |
| Background field (5 points) and 9 radial tints | ~1.3 ms |
| Grain: hash, soft light, thread bands | ~0.9 ms |
| Remaining ribbon shading | ~1.4 ms |

Candidate fixes, measured and rejected:

| Change | Result | Cause |
| --- | --- | --- |
| Per-pixel shared Catmull-Rom basis and angle-addition drift in WGSL (36 → 4 trig calls) | −2%, within noise | Edge evaluation is not the bottleneck after culling |
| Cull lip and thread bands per line | Byte-identical, no gain | Grain cost is the hash and soft light, not the band tests |
| Cache the phase-independent background in an RGBA16Float texture (Metal) | No gain, +32 MB | Reading 8 B/pixel costs about the same as recomputing 5 + 9 terms |
| Reuse the per-frame output texture and column buffer | Not pursued | Encoding is 0.29 ms; the frame is GPU-bound |

## Problem 5: one producer per backend

**Problem.** The native producer is backend-specific; Windows renders through
D3D11 and Linux through wgpu, each with its own recording context.

**Solution.** `native.rs` defines one frame snapshot (`PastelWavesNative`)
whose `prepare` dispatches on the host context type, with producers selected
at build time by target:

| Target | Context | Producer | Shader |
| --- | --- | --- | --- |
| macOS | `MetalCustomContext` | `metal.rs` | `pastel_waves.metal` |
| Windows | `Dx11CustomContext` | `dx11.rs` | `producer.wgsl` → HLSL SM 5.0 via naga |
| Linux | `WgpuCustomContext` | `wgpu_producer.rs` | `producer.wgsl` |

- One WGSL source serves wgpu and D3D11. naga lowers storage arrays to
  `RWByteAddressBuffer`/`ByteAddressBuffer`, so the D3D11 column buffer is a
  raw buffer (`D3D11_RESOURCE_MISC_BUFFER_ALLOW_RAW_VIEWS`) with a raw UAV for
  the compute pass and a raw `BUFFEREX` SRV for the pixel pass.
- WGSL binding numbers map directly to D3D11 registers (`b0` uniforms, `u1`
  columns written, `t2` columns read); shader model 5.0 has no register spaces.
  The UAV is unbound after `Dispatch` so the buffer can be bound as an SRV.
- wgpu uses automatic bind group layouts: the compute entry binds 0 and 1, the
  render entries 0 and 2. Pipelines are created inside a validation error
  scope and cached per device.
- Ribbon, field and color constants are generated from the Rust composition
  data for all three shader languages, so the renderers share one source of
  truth.

**Verification.**

| Check | Result |
| --- | --- |
| Metal producer after the refactor | 16 captured frames byte-identical |
| wgpu producer vs. Metal producer (wgpu's Metal backend) | ≤ 1/255 on < 0.1% of channels; `just wgpu-test` |
| WGSL → HLSL translation | Unit test on every desktop target: entry points, registers, `numthreads(64, 1, 1)` |
| Windows build | Cross-target type-check and clippy |
| D3D11 at runtime | Not yet run |
| Linux at runtime | Not yet run; producer code is the wgpu path verified above |

The native producer is the default renderer on macOS. Windows and Linux default
to `wgsl` until the native producers run on that hardware.

## Output verification method

Changes were accepted only with pixel evidence. `Window::render_to_image()`
(`test-support`) reads back the real window's last frame; with a fixed phase,
repeated captures are byte-identical. Each change was compared over 16 frames
(two renderers × four phases × grain on/off) against the pre-change capture.
Float reordering in shader refactors produces 1/255 differences on a small
fraction of pixels; anything larger is treated as a defect.

See [Labs & benchmarks](extension-lab.md) for running `paint-lab`.
