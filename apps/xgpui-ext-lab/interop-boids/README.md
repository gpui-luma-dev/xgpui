# Interop Boids

Why `xgpui-gpu-interop` exists, in one window: an existing wgpu renderer runs as an independent GPU engine inside an xgpui window, with zero copies.

The renderer is wgpu's own [`boids` example](https://github.com/gfx-rs/wgpu/tree/v30.0.1/examples/features/src/boids) from wgpu v30.0.1, ported with almost no changes. Each step is a compute pass that moves every boid, reading one particle buffer and writing the other, then a render pass that draws the flock. The particle buffers persist on the GPU from one step to the next. A GPUI shader paint can't do this: it has no compute pass and no state that survives between frames.

## Run

```bash
cargo run --release -p xgpui-ext-lab --bin interop-boids
```

| Flag | Effect |
| --- | --- |
| `--run-seconds N` | Print a summary after N seconds and exit. |
| `--particles N` | Start with N boids (default 1,500, wgpu's default). |
| `--speed X` | Start the speed slider at X, from 0.25 to 4 (default 1). |
| `--resize-test` | Resize the window continuously, to measure smoothness while the layout changes. |
| `--pause-at S` / `--play-at S` | Press Pause or Play after S seconds, for unattended runs. |

**Controls:**

- **Pause / Play.** While paused the engine stops stepping and does no GPU work. The last frame stays on screen, and the UI doesn't redraw. Play continues the same flock.
- **Boids: 500, 1,500, 5,000, 15,000.** Starts a new flock with that many boids. The compute pass is O(n²), so 15,000 is real GPU work. A new flock chosen while paused is drawn once and stays paused.
- **Speed: 0.25× to 4×.** A logarithmic slider with 1× in the middle (and a *1×* reset button). It scales the simulation's time step (wgpu's `deltaT`), so the flock moves slower or faster while every display frame still shows one new step: slow motion stays smooth.
- **Stats.** The engine's steps per second and its CPU time to record and present a step, next to the UI's frame interval and the latency from present to composition.

## How it works

- `src/boids.rs`, `src/compute.wgsl`, `src/draw.wgsl`: the wgpu example. The shaders are unchanged. In the Rust code the particle count is a parameter, `render` records into the caller's encoder and target instead of a swapchain frame, and it can redraw without stepping. The file headers list every change.
- `src/engine.rs`: the engine thread. It creates its own wgpu device: Metal on macOS, Vulkan on Linux (through `WgpuSurfaceTarget::request_device`, which adds the external-memory extensions) and D3D12 on Windows. It creates a `WgpuSurfaceTarget` of three 1024×768 `Bgra8Srgb` slots. Each step it acquires a slot, records the compute and draw passes into `target.texture(&slot)`, and calls `target.present(slot, [commands])`. It keeps at most one step of GPU work in flight, so a slow GPU lowers the step rate rather than queueing work.

  **Pacing.** The engine steps once per frame the window composites. After each present it waits until the compositor has shown that frame (`SurfaceStats::composited`), then steps again. So every step is shown exactly once, at the display's rate. An engine on a timer of its own drifts against vsync. An earlier version of this lab stepped on its own 60 Hz timer, and 4–5% of its frames were never shown (13% while resizing). Each dropped frame made the flock jump two steps at once. The time step follows the measured display interval, so the flock moves at the same speed at 60 Hz and 120 Hz. The cost is one display frame of latency from present to screen, which doesn't matter for a simulation nobody steers. If the window doesn't show a frame within 100 ms (for example while hidden), the engine steps anyway, so the flock keeps moving slowly.
- `src/main.rs`: the view. It paints the surface with `EngineSurface::paint`, letterboxed to 4:3, and redraws only when the engine presents: `on_present` sends to a channel, and a task on the UI thread calls `cx.notify()`. It never requests an animation frame.

**Format.** `SurfaceFormat::Bgra8Srgb` slots are `wgpu::TextureFormat::Bgra8Unorm` on all three platforms, with no sRGB view, so a shader must write sRGB-encoded values. The draw pipeline targets `target.format()`. Boids draws white on black, which is the same encoded or linear.

## Measured

macOS, Apple M4 Max, 60 Hz, release build, `--run-seconds 10`. Timings start 0.5 s after the window opens. *Repeated* counts UI frames that showed no new step, and *skipped* counts UI frames that jumped more than one step.

| Run | Steps/s | UI frame p50 / p95 / max | Repeated | Skipped |
| --- | --- | --- | --- | --- |
| default (1,500 boids) | 59.9 | 16.67 / 17.59 / 18.34 ms | 0 | 0 |
| `--resize-test` | 60.0 | 16.66 / 16.96 / 28.94 ms | 0 | 0 |
| `--speed 0.25` | 59.8 | 16.66 / 17.54 / 17.65 ms | 0 | 0 |
| `--speed 4` | 59.8 | 16.66 / 17.55 / 17.68 ms | 0 | 0 |
| `--particles 15000` | 59.9 | 16.67 / 17.55 / 17.69 ms | 0 | 0 |
| `--particles 15000 --resize-test` | 60.0 | 16.66 / 17.14 / 23.27 ms | 0 | 0 |

Each run had 1 unshown frame: the first one, at startup.

`--run-seconds 7 --pause-at 3 --play-at 5`: over the 2 s pause there were 0 engine steps and 1 UI frame (the one that redrew the button), and Play resumed the same flock.

## Credit

The boids example is from [wgpu](https://github.com/gfx-rs/wgpu), Copyright (c) 2025 The gfx-rs developers, licensed under either of the Apache License, Version 2.0 or the MIT license, at your option. wgpu adapted it from Austin Eng's [WebGPU samples](https://github.com/austinEng/webgpu-samples/blob/master/src/examples/computeBoids.ts).
