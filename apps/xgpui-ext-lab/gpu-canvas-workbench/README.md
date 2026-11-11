# GPU Canvas Workbench

The #197 example application. A small editable GPU workload runs in an engine on its own wgpu device: Metal on macOS, Vulkan on Linux, D3D12 on Windows. `xgpui-direct-gpu` presents it beside ordinary controls. It is not a painting application.

## Run

```bash
cargo run --release -p xgpui-ext-lab --bin gpu-canvas-workbench
```

| Flag | Effect |
| --- | --- |
| `--demo` | Paint scripted strokes continuously. |
| `--sdr` | Standard window. RGBA16F slots are shown as an sRGB preview. |
| `--run-seconds N` | Print a summary after N seconds and exit. |
| `--check` | Replay an edit script against the CPU reference without a window; exits non-zero on failure. |
| `--slots N` | Ring length, 2–8 (default 3). |

**Interaction:**

- Left-drag paints.
- Right-click inspects a 4×4 region.
- The controls set colour (linear Display P3), radius, opacity, exposure, presentation (Auto or forced sRGB preview), demo strokes and reset.

## How it works

The code is in `src/engine.rs`.

1. **Attach once.** `Engine::new` creates the engine's own device, then a `WgpuSurfaceTarget` (an RGBA16F linear-P3 ring, storage-writable). It then allocates a fixed working set:
   - the working image, a storage buffer of linear-P3 pixels;
   - a dab buffer and a parameter buffer;
   - three compute pipelines;
   - four staging buffers for inspection.

   Nothing else is allocated in steady state; `--check` verifies this.
2. **Edit.** Pointer input appends numbered dabs. Each dab is a new revision. A paint pass covers only the union of the queued dabs' bounds, up to 256 dabs per pass. Exposure is applied when publishing, so the working image is never changed by it.
3. **Publish.** When the revision has changed, `frame()` acquires a slot. A compute pass writes the working image, with exposure applied, into the slot, and `present` hands it over with a GPU-side wait.
   - The published slot is never written again. When every slot is still in use, the edit stays in the working image and is published on the next frame.
   - Frames with no edits do no GPU work.
4. **Shared placement.** The canvas and the thumbnail place the same published revision. `EngineSurface::paint` shows it in wide gamut in a wide window and as an sRGB preview otherwise. The **sRGB preview** control forces `preview_content()`.
5. **Inspect.** `inspect(x, y)` applies queued edits, copies a 4×4 region into a free staging buffer, and maps it asynchronously.
   - The result carries the revision it was requested at, even when more edits arrive before it completes.
   - Requests are bounded: when all four staging buffers are busy, the request is refused.
6. **Teardown.** Dropping the engine drops its surface. Slots return once the last scene that samples them is gone.

## Evidence

`--check` compares every working pixel with `src/oracle.rs`, an independent CPU reference, after:

- generation;
- 250 replayed dabs, including touched and untouched pixels, partial rim coverage and dabs off the canvas edge;
- exposure;
- more queued dabs than one pass holds;
- a reset.

It also checks:

- presentation-only frames;
- the inspection revision during rapid edits;
- the inspection bound;
- that the GPU-object count is the same after 200 edited frames.

Published values are checked through the same publish shader writing into a check texture; slot transport itself is covered by the `xgpui-direct-gpu` tests.

Measured on macOS (M-series, wide window, `--demo --run-seconds 6`):

| Measure | Result |
| --- | --- |
| Frame interval | p95 17.66 ms |
| Edit to publish | p50 0.59 ms |
| Publish cost | 1 MiB written by the GPU per snapshot (640×400 RGBA16F) |
| CPU bytes per dab | 32 uploaded |
| CPU readback | none, except inspection |

Linux and Windows results are posted on #198.

Not yet included: a second window, pan/zoom, pressure and failure test controls, and generation-replacement recovery. `engine-surface-lab --lose-engine-after` covers engine loss.
