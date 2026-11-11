# GPU engines and color in xgpui apps

This guide is for app developers who render with their own GPU code (a canvas, a simulation, a viewer, a game-style renderer) and need its output to look right next to ordinary xgpui UI, with wide gamut when the display supports it.

Two rules prevent most problems:

1. **Declare what your pixels mean; never convert them for the monitor yourself.** xgpui converts declared color once, on the GPU, into the window's composition space. The operating system then maps that to the actual display.
2. **Don't fork xgpui, or swap its rendering backend, to reach the GPU.** Keep your engine on its own GPU device and present its frames through an engine surface (`xgpui-direct-gpu`). xgpui handles the platform-specific sharing on macOS, Linux and Windows.

## Why these rules exist

A familiar failure: an app that needed its own wgpu renderer forked its UI framework to swap the backend, then managed color by hand only in its canvas:

- **The fork drifted** and stopped building on some platforms.
- **The canvas converted to the monitor's ICC profile in its own shader,** but only on the platform where it could read the profile. On the others it failed.
- **It rendered into an 8-bit intermediate,** so wide colors were clipped.
- **It assumed every image was sRGB.**
- **The UI and the canvas went through different color paths.**
- **Where the OS also color-managed the window, colors were converted twice.**

Each of these breaks color on a different platform or display. Declaring color and presenting through xgpui avoids all of them.

## Choose the path for your GPU work

| Your GPU work | Use | Notes |
| --- | --- | --- |
| Shader effects, paints and fills drawn as part of the UI frame | `xgpui-shader` paints, custom content | Runs inside xgpui's frame, on xgpui's device. |
| Decoded images, video frames | `xgpui-media` | Supply pixels plus an explicit interpretation; see [Pixels and metadata](pixels-and-metadata.md). |
| A persistent engine: its own state, compute, many passes, its own frame rate or thread | `xgpui-direct-gpu` engine surfaces | The engine keeps its own device, queue and thread. Frames are shown with no copies, so you don't need a fork. |

## Color: declare, don't convert

### Window

Ask for wide presentation only on windows that need it, then check what the window actually negotiated:

```rust,ignore
let status = window.set_color_mode(WindowColorMode::WideGamutSdr)?;
let wide = window.color_status().effective() == WindowColorMode::WideGamutSdr;
```

Ordinary UI colors look the same in wide and standard windows. Wide gamut only changes how content declared as wide is presented. macOS composes in extended linear Display P3, Windows in scRGB. Linux has no wide composition yet (#146), so wide content is shown as a mapped sRGB preview there.

### Engine output

Pick a `SurfaceFormat`. It fixes both the storage and the meaning, so there's no untyped "whatever is in this texture":

| `SurfaceFormat` | Write these values | Alpha | Shown as |
| --- | --- | --- | --- |
| `Bgra8Srgb` | sRGB-**encoded** channels (what an 8-bit sRGB image holds) | straight | sRGB, in every window |
| `Rgba16FloatLinearP3` | **linear-light** Display P3 (D65), 0–1 | associated (premultiplied) | wide gamut in a wide window, and an sRGB preview in other windows and on Linux |

- **Render in linear light.** Write linear values to `Rgba16FloatLinearP3`, and sRGB-encoded values to `Bgra8Srgb`. Mixing the two up is the usual cause of gradients that are too dark or too light.
- **Don't add a monitor transform.** Don't read the display's ICC profile, and don't convert to it. Converting to the monitor yourself causes colors to be converted twice where the OS also manages color, and makes them platform-specific everywhere else.
- **Use 16-bit float for wide or high-precision work.** An 8-bit target clips wide colors and bands in dark gradients.
- **Let `EngineSurface::paint` choose.** It shows the frame wide or as a preview to suit each window. Use `content()` or `preview_content()` only when you need to choose explicitly.

### Images and colors from files

Don't assume sRGB. Supply each image's primaries, transfer and alpha meaning, or parse its embedded ICC profile once. See [Pixels and metadata](pixels-and-metadata.md) and [ICC profiles](icc-profiles.md). Missing metadata is the application's policy decision; an unsupported profile must not silently become sRGB.

## Engine checklist

```rust,ignore
// One engine device of its own. On Linux, request_device enables memory
// and signal sharing with the compositor and a low-priority queue; on
// macOS and Windows, request a device with required_features().
#[cfg(target_os = "linux")]
let (device, queue) = WgpuSurfaceTarget::request_device(&adapter)?;
#[cfg(not(target_os = "linux"))]
let (device, queue) = adapter
    .request_device(&wgpu::DeviceDescriptor {
        required_features: WgpuSurfaceTarget::required_features(),
        ..Default::default()
    })
    .await?;
let mut target = WgpuSurfaceTarget::new(SurfaceDesc {
    width, height,
    format: SurfaceFormat::Rgba16FloatLinearP3,
    slots: 3 + extra_windows, // see "Ring size" below
}, &device, &queue)?;

// Render loop, usually on the engine's own thread.
let slot = match target.surface().acquire() {
    Ok(slot) => slot,
    Err(AcquireError::WouldBlock) => continue,     // ring full: try next frame
    Err(AcquireError::Invalidated) => rebuild()?,  // device lost
};
let texture = target.texture(&slot)?;               // render or compute into it
target.present(slot, [encoder.finish()])?;          // submits, then publishes

// UI: place the latest frame and redraw when a new one arrives.
target.surface().paint(window, bounds, radii);
```

- **Ring size.** Use 3 slots, plus one for each additional window showing the surface. A window that isn't drawing, such as a minimized one, keeps its last frame in its retained scene, and that pins the slot until it draws again.
- **Redraw on new frames, not every display frame.** Wake the UI from `EngineSurface::on_present`. Note that GPUI throttles animation frames in unfocused windows to `WindowOptions::inactive_frame_interval` (#211).
- **Heavy engines.** If a frame takes a large share of GPU time, call `target.set_heavy(Some(HeavyEngine::default()))`. Then split each frame's work into `suggested_chunks()` submissions, and wait `pace()` between frames. This keeps the UI responsive when the GPU is saturated; see `extensions/crates/xgpui-direct-gpu/README.md`.
- **Device loss.** On the engine's device-lost callback, call `invalidate()`. The last frame keeps showing. Then rebuild on a new device and surface, reconstructing your state from data the app keeps on the CPU, such as an edit log. GPU state is not carried across.
- **Unsupported setups.** If the compositor can't use the surface, for example because the engine is on a different GPU, the first frame fails with `Unsupported`. `EngineSurface::unsupported()` reports why, and the surface offers no further frames. Show the reason; don't retry every frame.

The GPU Canvas Workbench (`apps/xgpui-ext-lab/gpu-canvas-workbench`, with its own README) is a complete example of all of this.

## Verify instead of eyeballing

- **Numbers.** Compare your engine's output with an independent CPU computation of the same pixels. The workbench's `--check` does this for every pixel. `xgpui-direct-gpu`'s color tests compare compositor output for near-black, out-of-sRGB, fractional-alpha and opacity patches against a reference built from the primaries matrix and transfer function.
- **A real display.** Run `gpu-canvas-workbench --compare` on a wide display. The same frame appears in wide gamut on the left and as the sRGB preview on the right. The P3 patches should look more saturated on the left, and the greys should match. Screenshots are usually converted to sRGB, so judge this by eye on the screen, not from a screenshot.
- **Load.** `engine-surface-lab --stress-surfaces N --stress-size WxH --work N [--heavy]` shows whether your engine's GPU load starves the UI.

## Common mistakes

| Symptom | Likely cause | Fix |
| --- | --- | --- |
| Colors too saturated or washed out on one OS | The app converted to the monitor profile, and the OS converted again | Remove the monitor transform; declare and let xgpui and the OS map |
| Correct on one platform, wrong on others | Platform-specific color code, such as reading display profiles | Remove it; xgpui presents with the same meaning everywhere |
| Gradients too dark or too light | Encoded values written to a linear surface, or the reverse | Write linear values to `Rgba16FloatLinearP3`, encoded values to `Bgra8Srgb` |
| Dark fringes or halos on soft edges | Straight alpha written where associated alpha is expected, or the reverse | Premultiply for `Rgba16FloatLinearP3`; use straight alpha for `Bgra8Srgb` |
| Wide colors clipped, banding in dark areas | 8-bit intermediate | Render in 16-bit float |
| Imported images look off | Every image assumed to be sRGB | Supply interpretation or ICC; see [Pixels and metadata](pixels-and-metadata.md) |
| UI stutters while the engine works | Engine frames saturate the GPU, and composition waits for them | Use heavy-engine mode |
| Publishing stalls with several windows | Ring too small; hidden windows keep their last frame | Use 3 slots plus one per extra window |

## Limits

- Linux shows wide content as an sRGB preview; there's no native wide composition yet (#146).
- HDR/EDR output is out of scope; `WideGamutSdr` is wide SDR.
- Windows engines can't lower their GPU priority through wgpu. Heavy-engine mode compensates.
- Submitted GPU work that never completes is a GPU hang. The driver's timeout and device-loss handling deal with it, not xgpui.
- Engine surfaces are experimental and unpublished; the API may change.
