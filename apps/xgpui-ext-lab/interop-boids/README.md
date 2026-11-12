# Interop Boids

Why `xgpui-gpu-interop` exists, in one window: an existing wgpu renderer runs as an independent GPU engine inside an xgpui window, with zero copies. The UI stays responsive however heavy the engine gets.

The renderer is wgpu's own [`boids` example](https://github.com/gfx-rs/wgpu/tree/v30.0.1/examples/features/src/boids) from wgpu v30.0.1, ported with almost no changes. Each step is a compute pass that moves every boid, reading one particle buffer and writing the other, then a render pass that draws the flock. The particle buffers persist on the GPU from one step to the next. A GPUI shader paint can't do this: it has no compute pass and no state that survives between frames.

## Run

```bash
cargo run --release -p xgpui-ext-lab --bin interop-boids
```

**Try this:** choose 100,000 boids, then drag the speed slider and resize the window. Switch *Engine on* to *UI thread* and do the same. On its own thread only the flock slows down. On the UI thread every UI frame waits for the engine, so the slider and resizing lag.

| Flag | Effect |
| --- | --- |
| `--run-seconds N` | Print a summary after N seconds and exit. |
| `--particles N` | Start with N boids (default 1,500, wgpu's default). |
| `--speed X` | Start the speed slider at X, from 0.25 to 4 (default 1). |
| `--display N` | Open the window on display N (the app prints the list at startup). |
| `--ui-thread` | Start with the engine on the UI thread. |
| `--switch-at S,S,…` | Switch the engine between its own thread and the UI thread at these times. |
| `--resize-test` | Resize the window continuously, to measure the UI while the layout changes. |
| `--pause-at S` / `--play-at S` | Press Pause or Play after S seconds, for unattended runs. |

**Controls:**

- **Pause / Play.** While paused the engine stops stepping and does no GPU work. The last frame stays on screen, and the UI doesn't redraw. Play continues the same flock.
- **Engine on: Own thread / UI thread.** Moves the engine (device, surface and flock) between its own thread and the UI thread, without restarting it.
- **Boids: 500, 1,500, 15,000, 50,000, 100,000.** Starts a new flock with that many boids. The compute pass is O(n²): on an M4 Max a step takes about 2 ms of GPU time at 1,500 boids, 9 ms at 15,000, 16 ms at 50,000 and 54 ms at 100,000. A new flock chosen while paused is drawn once and stays paused.
- **Speed: 0.25× to 4×.** A logarithmic slider with 1× in the middle (and a *1×* reset button). It scales the simulation's time step (wgpu's `deltaT`), so the flock moves slower or faster while every display frame still shows one new step: slow motion stays smooth.
- **Stats.** Steps per second, the engine's CPU time per step (which includes waiting for the previous step's GPU work) and its GPU time per step, next to the UI thread's work per frame. The last line also says how frames are presented.

Every stat value sits in a fixed-width box with tabular digits, so changing numbers never move the layout.

## How it works

- `src/boids.rs`, `src/compute.wgsl`, `src/draw.wgsl`: the wgpu example. The shaders are unchanged. In the Rust code the particle count is a parameter, `render` records into the caller's encoder and target instead of a swapchain frame, it can redraw without stepping, and `set_delta_t` changes the time step. The file headers list every change.
- `src/engine.rs`: the engine. It creates its own wgpu device: Metal on macOS, Vulkan on Linux (through `WgpuSurfaceTarget::request_device`, which adds the external-memory extensions) and D3D12 on Windows. It creates a `WgpuSurfaceTarget` of three 1024×768 `Bgra8Srgb` slots. Each step it acquires a slot, records the compute and draw passes into `target.texture(&slot)`, and calls `target.present(slot, [commands])`. It keeps at most one step of GPU work in flight, so a slow GPU lowers the step rate rather than queueing work.
- `src/main.rs`: the view. It paints the surface with `EngineSurface::paint`, letterboxed to 4:3, and redraws only when the engine presents: `on_present` sends to a channel, and a task on the UI thread calls `cx.notify()`. With the engine on its own thread, the view never requests an animation frame. On the UI thread, the view steps the engine inside `render` and requests the next frame, as an engine embedded in the UI would.

**Pacing.** The engine steps once per frame the window composites. After each present it waits until the compositor has shown that frame (`SurfaceStats::composited`), then steps again. So every step is shown exactly once, at the display's rate. An engine on a timer of its own drifts against vsync. An earlier version of this lab stepped on its own 60 Hz timer, and 4–5% of its frames were never shown (13% while resizing). Each dropped frame made the flock jump two steps at once. The time step follows the measured display interval, so the flock moves at the same speed at 60 Hz and 120 Hz. If the window doesn't show a frame within 100 ms (for example while hidden), the engine steps anyway, so the flock keeps moving slowly.

**Presentation.** On its own thread the engine measures each step's GPU time (`Queue::on_submitted_work_done`) and adapts, following the crate's heavy-engine guidance:

- Steps shorter than one refresh of the display are presented with a GPU-side wait, so the next UI frame shows them.
- Steps longer than one refresh are presented observe-only (`set_gpu_wait(false)`). Below ¾ of a refresh it switches back.

The refresh interval comes from the window (`Window::refresh_interval`), and the view passes it on whenever it changes, for example when the window moves to another display. The interval at which the engine's frames get shown can't stand in for it: a slow engine is shown less often than the display refreshes. An earlier version used that interval, and at 120 Hz a 15 ms step at 50,000 boids looked like it fit within a frame. The engine kept GPU waits, and the UI dropped to 60 fps.

Both modes matter. With GPU waits at 100,000 boids, the compositor waited for each 54 ms step, and even resizing dropped to 18 fps. Observe-only kept the UI at 60 fps. With observe-only at 15,000 boids, a 9 ms step was sometimes not finished when its present woke the UI, so it took a second UI frame, and the engine fell to 38 steps/s. GPU waits kept it at 60. On the UI thread the engine is always observe-only, because there it can't measure GPU time.

**Format.** `SurfaceFormat::Bgra8Srgb` slots are `wgpu::TextureFormat::Bgra8Unorm` on all three platforms, with no sRGB view, so a shader must write sRGB-encoded values. The draw pipeline targets `target.format()`. Boids draws white on black, which is the same encoded or linear.

## Measured

macOS, Apple M4 Max, release build, `--run-seconds 8`, on a 60 Hz external display and on the built-in 120 Hz ProMotion display. Timings start 0.5 s after the window opens. *UI work* is the UI thread's time from `render` to paint in each frame. *Repeated* counts UI frames that showed no new step, and *skipped* counts UI frames that jumped more than one step.

**60 Hz** (measured before the engine took its refresh interval from the window; at 60 Hz both give the same 16.7 ms threshold):

| Run | Steps/s | Presentation | UI work p50 | UI frame p50 / p95 | Repeated / skipped |
| --- | --- | --- | --- | --- | --- |
| default (1,500 boids) | 59.5 | GPU wait | 1.9 ms | 16.67 / 17.57 ms | 0 / 0 |
| `--resize-test` | 59.7 | GPU wait | 0.5 ms | 16.66 / 17.64 ms | 0 / 0 |
| `--particles 15000` | 59.6 | GPU wait | 2.0 ms | 16.66 / 17.57 ms | 0 / 0 |
| `--particles 15000 --resize-test` | 59.7 | GPU wait | 0.4 ms | 16.59 / 19.68 ms | 0 / 0 |
| `--particles 50000` | 58.8 | GPU wait | 1.6 ms | 16.67 / 17.54 ms | 0 / 0 |
| `--particles 100000 --resize-test` | 15.2 | observe-only | 0.3 ms | 16.57 / 20.08 ms | 340 / 0 |
| `--resize-test --ui-thread` | 59.8 | observe-only | 0.8 ms | 16.67 / 17.44 ms | 0 / 0 |
| `--particles 100000 --resize-test --ui-thread` | 18.2 | observe-only | 51.6 ms | 55.31 / 59.30 ms | 0 / 0 |

**120 Hz:**

| Run | Steps/s | Presentation | UI work p50 | UI frame p50 / p95 | Repeated / skipped |
| --- | --- | --- | --- | --- | --- |
| default (1,500 boids) | 118.7 | GPU wait | 1.4 ms | 8.33 / 9.20 ms | 0 / 0 |
| `--resize-test` | 119.3 | GPU wait | 0.5 ms | 8.33 / 9.99 ms | 0 / 0 |
| `--speed 0.25` | 119.1 | GPU wait | 1.4 ms | 8.33 / 9.20 ms | 0 / 0 |
| `--speed 4` | 119.3 | GPU wait | 1.4 ms | 8.33 / 9.22 ms | 0 / 0 |
| `--particles 15000` | 118.2 | GPU wait | 0.9 ms | 8.33 / 9.25 ms | 0 / 0 |
| `--particles 15000 --resize-test` | 119.1 | GPU wait | 0.5 ms | 8.33 / 9.79 ms | 0 / 0 |
| `--particles 50000` | 56.5 | observe-only | 0.4 ms | 8.33 / 16.18 ms | 450 / 0 |
| `--particles 50000 --resize-test` | 56.7 | observe-only | 0.4 ms | 8.33 / 8.94 ms | 472 / 0 |
| `--particles 100000 --resize-test` | 15.8 | observe-only | 0.4 ms | 8.35 / 12.87 ms | 742 / 0 |
| `--resize-test --ui-thread` | 120.6 | observe-only | 0.7 ms | 8.33 / 10.40 ms | 0 / 0 |
| `--particles 100000 --resize-test --ui-thread` | 18.0 | observe-only | 54.4 ms | 56.03 / 59.43 ms | 0 / 0 |

At 120 Hz the engine takes 120 steps a second, each with half the 60 Hz time step, so the flock moves at the same speed. A step at 50,000 boids (15 ms of GPU time) is longer than a 120 Hz refresh, so the engine presents observe-only and runs at 57 steps/s, while the UI stays at 120 fps. Each step is then shown for about two UI frames, which is what *repeated* counts.

The 100,000-boid rows are the point of the lab. On its own thread the engine manages 15 steps a second, and the UI keeps redrawing at the display's rate for the resize: those redraws are the repeated frames, which show the newest step again. On the UI thread every UI frame waits about 52–54 ms for the engine, and the window resizes at 18 fps on either display.

The speed slider doesn't change these numbers: `--speed 0.25` and `--speed 4` both ran at 59.5–59.8 steps/s with no repeated or skipped frames.

`--particles 100000 --resize-test --switch-at 3,6 --pause-at 8 --play-at 9` moved the engine to the UI thread and back. The step count carried on across both moves (47, then 103), so it was the same flock. After the move back, the UI was again at 16.55 / 19.78 ms, and the 1 s pause had 0 engine steps.

## Credit

The boids example is from [wgpu](https://github.com/gfx-rs/wgpu), Copyright (c) 2025 The gfx-rs developers, licensed under either of the Apache License, Version 2.0 or the MIT license, at your option. wgpu adapted it from Austin Eng's [WebGPU samples](https://github.com/austinEng/webgpu-samples/blob/master/src/examples/computeBoids.ts).
