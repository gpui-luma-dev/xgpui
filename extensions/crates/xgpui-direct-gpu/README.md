# xgpui-direct-gpu (experimental)

Present output from an independently owned GPU engine into **xgpui**, like
presenting to a swapchain. Design and evidence: issues #197 and #198.

The engine keeps its own device, queue and thread. An `EngineSurface` owns a
fixed ring of shareable slots:

```rust,ignore
let surface = EngineSurface::new(SurfaceDesc {
    width: 640,
    height: 400,
    format: SurfaceFormat::Bgra8Srgb, // or Rgba16FloatLinearP3 in wide windows
    slots: 3,
})?;
let slot = surface.acquire()?;          // Err(WouldBlock) when the ring is full
// ...render into slot.iosurface() on the engine's GPU, submit a signal...
surface.present(slot, sync_point)?;     // only after the signal is submitted
surface.paint(window, bounds);          // in an element's paint phase
```

- A presented slot is never rewritten while a retained scene or in-flight
  composition still uses it; it returns to the ring afterwards.
- The compositor waits for the sync point on the GPU. Nothing goes through the CPU.
- `WgpuSurfaceTarget` (feature `wgpu`) wraps slots as `wgpu::Texture`s for a
  wgpu engine on Metal and presents after submission.

Status: macOS (IOSurface + `MTLSharedEvent`, zero copy). Other platforms are not
implemented yet. `Rgba16FloatLinearP3` requires a window with
`WindowColorMode::WideGamutSdr`. Not published; the API is under review.

Lab: `cargo run -p xgpui-ext-lab --bin engine-surface-lab [--sdr]`.
