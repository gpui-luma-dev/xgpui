# xgpui-engine-surface (experimental)

Present output from an independently owned GPU engine into **xgpui**, the way an engine presents to a swapchain. Design and evidence are in issues #197 and #198.

The engine keeps its own device, queue and thread. An `EngineSurface` owns a fixed ring of shareable slots:

```rust,ignore
let mut target = WgpuSurfaceTarget::new(SurfaceDesc {
    width: 640,
    height: 400,
    format: SurfaceFormat::Rgba16FloatLinearP3, // or Bgra8Srgb
    slots: 3,
}, &device, &queue)?;
let slot = target.surface().acquire()?;       // Err(WouldBlock) when the ring is full
let texture = target.texture(&slot)?;         // render, or compute into it (RGBA16F)
target.present(slot, [encoder.finish()])?;    // submits, then publishes the frame
target.surface().paint(window, bounds, radii); // in an element's paint phase
```

## Platforms

| Platform | Slots | Synchronisation | Copies |
| --- | --- | --- | --- |
| macOS (Metal) | IOSurface | `MTLSharedEvent` | none |
| Linux (Vulkan) | opaque-fd external memory | exported timeline semaphore | none |
| Windows (D3D12 engine, D3D11 compositor) | shared NT-handle textures | shared fence | none |

Engines today: on Linux and Windows an engine uses wgpu, through feature `wgpu` and `WgpuSurfaceTarget`. On Linux, create its device with `WgpuSurfaceTarget::request_device`. On macOS a raw Metal engine can also present directly, using `SyncPoint::new` or the `unsafe SyncPoint::submitted`. The shared memory and signals are native to each platform, so adapters for raw Vulkan or D3D12 engines could be added later.

## Guarantees
- **Slots are immutable once published.** A presented slot is never rewritten while a retained scene or in-flight composition uses it. It returns to the ring afterwards, even if no further frames are drawn. Size the ring at 3 plus one for each additional window showing the surface: a window that isn't drawing, such as a minimized one, keeps its last frame in its retained scene, and that pins the slot until it draws again.
- **Colour.** `Bgra8Srgb` works in every window. `Rgba16FloatLinearP3` is shown in wide gamut in a `WideGamutSdr` window and as a mapped sRGB preview elsewhere: `paint` chooses for each window, and `preview_content` / `content` choose explicitly. RGBA16F slots allow storage writes. Colour is verified against a CPU reference on all three platforms.
- **A misbehaving engine can't stall composition.** The compositor waits on the GPU only for frames whose signal is known to be submitted. Every `WgpuSurfaceTarget` frame qualifies, and so does a macOS frame presented with the `unsafe SyncPoint::submitted`. Any other frame is shown only once its signal has been seen reached, and until then the last ready frame stays on screen. Not covered: submitted work that never finishes (a GPU hang), which ends with the driver's timeout and device-loss handling.
- **Unsupported compositors.** A compositor that can't use the surface, for example because the engine is on a different GPU, fails the first preparation with `Unsupported`. The surface latches that reason, reported by `EngineSurface::unsupported()`, and offers no further frames.
- **Recovery.** `EngineSurface::invalidate()` on engine device loss: the last frame keeps showing, and a new surface on a new device recovers. Replacement of the compositor's own device re-imports slots.

## Feedback and heavy engines

`SurfaceStats` reports `composited`, `unshown` (frames dropped before they were ever shown) and `composite_latency_ms` (the time from `present` to first composition).

An engine whose frames cost a lot of GPU time should turn on **heavy-engine mode**:

```rust,ignore
target.set_heavy(Some(HeavyEngine::default())); // observe-only + pacing + chunk advice
loop {
    let chunks = target.suggested_chunks();     // submit all but the last with the queue
    render_frame_in(chunks, &mut target)?;      // the last chunk goes with present()
    std::thread::sleep(target.pace());          // backs off while the UI lags
}
```

Heavy-engine mode combines three measures, because the stress tests showed that each alone falls short:

1. **Observe-only presentation** (`set_gpu_wait(false)`). The compositor never waits for an unfinished engine frame.
2. **Short submissions** of about `chunk_ms` of GPU time, so the GPU can schedule the compositor between them. At most `max_chunks` per frame (default 64).
3. **Throttling on compositor lag.**

On Linux the engine queue is also created at low priority where the driver supports it. The Windows D3D12 queue priority cannot be lowered through wgpu.

Measured with 4 × 4K engines at a GPU-saturating load on an Intel Iris Xe: the UI went from 5–6 frames in 6 s with the defaults to p50 16.7 ms, with a p95 of 20.5 ms on Windows and 33.6 ms on Linux. On an Apple M-series running 8 × 4K at a heavier load, it went from starved to p50 16.7 ms and p95 51 ms.

## Labs
- `cargo run -p xgpui-ext-lab --bin engine-surface-lab -- --threaded`. Stress testing: `--stress-surfaces N --stress-size WxH --work N [--heavy]`.
- `cargo run -p xgpui-ext-lab --bin gpu-canvas-workbench`: the example application, with `--check` for automated verification.

Not published; the API is under review.
