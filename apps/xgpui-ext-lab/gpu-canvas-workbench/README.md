# GPU Canvas Workbench

The #197 example application. A small editable GPU workload runs in an engine on its own wgpu device: Metal on macOS, Vulkan on Linux, D3D12 on Windows. `xgpui-surface` presents it beside ordinary controls. It is not a painting application.

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
| `--second-window` | Open a second window that places the same published revision. |
| `--minimize-after S` | Minimize the main window after S seconds; the second window keeps stepping the engine. |
| `--resize-second` | Resize the second window every 0.5 s. This only changes placement; nothing is republished. |
| `--compare` | Split the canvas: the left half in wide gamut, the right half showing the same region as the sRGB preview. On a wide display the P3 patches look more saturated on the left, and the greys match. *Compare wide \| sRGB* toggles it. |
| `--pan-zoom` | Animate zoom and pan of the main canvas. This only changes placement; nothing is republished. |
| `--ui-thread-engine` | Run the engine on the UI thread instead of its own thread, for comparison. |
| `--lose-device-after S` | Destroy the engine's device after S seconds, then rebuild on a new device by replaying the edit log. |
| `--throttle-inactive` | Keep GPUI's default 30 fps animation throttle for unfocused windows. |

**Interaction:**

- Left-drag paints.
- Scroll pans the canvas; ⌘/Ctrl-scroll zooms around the cursor, 1–8×. *Fit view* resets. Pan and zoom only move and scale the placement, clipped to the canvas, so they never republish, and dabs land under the cursor at any zoom.
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

## Threading

By default the engine runs on its own thread (`src/driver.rs`):

- The UI sends edits, exposure, reset, inspection and device-loss commands over a channel, and places the surface's latest published frame. It never waits for the engine.
- The engine thread applies all queued commands, then edits and publishes in **one submission** per frame (paint and publish passes together). It recovers from device loss by itself, so a rebuild never blocks the UI.
- It blocks on its command channel when idle, and waits in 1 ms slices while work is in flight.
- Edit-to-publish latency counts from the moment the UI made the edit. The status also reports CPU time spent recording and inside `present`.

`--ui-thread-engine` runs the same engine on the UI thread for comparison.

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

Published values are checked through the same publish shader writing into a check texture; slot transport itself is covered by the `xgpui-surface` tests.

Measured on macOS (M-series, wide window, `--demo --run-seconds 6`):

| Measure | Result |
| --- | --- |
| Frame interval | p95 17.66 ms |
| Edit to publish | p50 0.59 ms |
| Publish cost | 1 MiB written by the GPU per snapshot (640×400 RGBA16F) |
| CPU bytes per dab | 32 uploaded |
| CPU readback | none, except inspection |

Linux and Windows results are posted on #198.

**Windows.** The engine is stepped by whichever window renders, at most once per display frame, so edits keep progressing while the main window is hidden. *Open second window* and *Minimize* are also available as controls.

Measured on macOS:

- **Resizing:** one snapshot served 3 window sizes, with no republishing.
- **Minimizing:** the second window carried on at 60 fps, after a single stall of about 570 ms during the minimize animation.
- **Two animating windows:** both run at 60 fps (298 new frames each in 5 s).

GPUI throttles the animation frames of an unfocused window to `WindowOptions::inactive_frame_interval`, 33.3 ms by default, as a power-saving policy on every platform. The workbench turns that off so its measurements reflect compositing; `--throttle-inactive` restores GPUI's default, under which the unfocused window runs at about 30 fps.

**Recovery.** The engine keeps every dab since the last reset as a CPU edit log. *Lose device* (or `--lose-device-after S`) destroys the engine's device and invalidates its surface; the last frame stays on screen. Half a second later the workbench rebuilds the engine on a new device and surface, and replays the log to reconstruct the working image. It never copies GPU state across. Inspections that were in flight on the old device are dropped. Each inspection carries its generation, and windows follow the new surface.

On macOS, 1,697 dabs replayed in 8.3 ms. `--check` verifies the rebuilt image pixel for pixel against the CPU reference.

On macOS, `--pan-zoom` made 236 view changes in 4 s with a single snapshot, and the frame interval p95 stayed at 17.5 ms. Pressure is covered by *Hold snapshot* and `--check`. `engine-surface-lab --lose-engine-after` covers engine loss.
