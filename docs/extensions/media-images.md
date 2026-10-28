# xgpui-media

Images and GPU-backed video frames for GPUI. Use it when your app needs large
photos, image adjustments, or a view of frames produced by another part of your
application. The API is experimental.

Images keep a set of smaller versions called **[mipmaps ↗](https://en.wikipedia.org/wiki/Mipmap)**. The GPU picks a suitable
level when drawing a small preview, which helps fine detail stay steady as you
zoom or scroll. The native loader prepares images in the background and limits
how much work can enter the renderer at once.

## Linear adjustments and next pipeline contracts

[Media pipeline contracts](media-pipeline.md) documents Phases 1–5 of issue #26:
explicit linear color/alpha semantics, CPU matrix/vignette references, progressive
publication states and bounded tile planning. The new `ImageAdjustments` placement API renders matrix/vignette effects in
linear [sRGB ↗](https://en.wikipedia.org/wiki/SRGB) on retained [RGBA8 ↗](https://en.wikipedia.org/wiki/RGBA_color_model) images, without changing `ImageFilter`. Run
`just media --adjustments` or use the browser Image filters demo. RGBA16F uploads, progressive/tiled rendering, bounded preview blur and native
video adjustments are available; the pipeline guide describes their limits. Existing image and native surface instructions below still apply.

## Where this helps in a real app

| Example | What you get |
| --- | --- |
| A photo library with thumbnails and a detail view | Reuse one loaded image at several sizes. Mipmaps reduce shimmer in small previews. |
| An asset browser that loads large images while you scroll | Background decoding and upload limits help keep loading from competing with every UI frame. |
| A photo editor, theme preview, or disabled image state | Adjust brightness, contrast, saturation, hue or tint while keeping the original image. |
| A camera or screen-capture preview | Display frames supplied by a producer. The latest-frame channel replaces stale pending frames instead of building a playback backlog. |

### Beyond photos and video

Any UI that displays detailed raster content at different sizes can benefit:

- **Document previews:** render a PDF page once, then reuse it in thumbnails and
  a larger preview.
- **Maps and diagrams:** display rasterized layers or tiles while zooming;
  mipmaps help reduce shimmer.
- **Scientific tools:** show generated heatmaps, simulation output or microscope
  images, with brightness and contrast controls.
- **Remote desktops and emulators:** present frames produced elsewhere, keeping
  the latest available frame rather than accumulating stale ones.
- **Design tools:** reuse cached raster previews of complex artwork while
  navigating a canvas.

The pixels can come from a renderer, simulation or remote source. Document
rendering, map generation and remote-frame decoding still happen outside
`xgpui-media`. For already-decoded pixels, `mipmapped_rgba8` builds a mip chain;
a host adapter handles the GPU upload. Native GPU frames use the video adapters
described below.

For small icons and ordinary UI images, GPUI's existing `img` is still a useful
default. You opt into `xgpui-media` where these features help.

## Load once, display many times

Keep a `MediaLoader`, the pending `ImageJob`, and the resulting `MediaImage` in
your view. Start a load when the user chooses an image, rather than starting it
again on every render.

```rust,ignore
use xgpui_media::MediaLoader;

// Create once and retain in your view.
let loader = MediaLoader::default();
let job = loader.load("photos/cover.webp")?;
// Store job in self.pending so later renders can poll it.
```

Poll the job from your view or a GPUI task. Taking a completed image does not wait
for decoding. These snippets assume your view has `pending` and `photo` fields:

```rust,ignore
if let Some(job) = self.pending.as_mut()
    && let Some(image) = job.try_take()
{
    self.photo = Some(image);
    self.pending = None;
}
```

Check `job.status()` for `LoadStatus::Failed(error)` and show a useful error state.
The host view must arrange notifications or polling while decoding. Dropping the
job cancels publication of its result.

Once you have an image, build a normal styled element:

```rust,ignore
use gpui::{prelude::*, *};
use xgpui_media::image_view;

image_view(image.clone())
    .w(px(480.)).h(px(320.))
    .rounded(px(12.))
```

Clones share the source texture, so a thumbnail and a larger preview can reuse the
same image. `image_view` stretches to the size you give it; match the source aspect
ratio when you want to avoid distortion. PNG, JPEG and WebP first frames are
supported. Loading from a URL and animated-image playback are not included.

## Adjust an image without changing the original

For example, make an unselected thumbnail grayscale:

```rust,ignore
use xgpui_media::ImageFilter;

image_view(image.clone())
    .filter(ImageFilter::new().saturation(0.))
    .w(px(160.)).h(px(100.))
```

Or give a preview a small brightness and contrast adjustment:

```rust,ignore
let adjustments = ImageFilter::new()
    .brightness(1.05)
    .contrast(1.15);

image_view(image.clone())
    .filter(adjustments)
    .w(px(480.)).h(px(320.))
```

The GPU applies these adjustments while drawing. Changing a filter reuses the
loaded source; it does not decode or upload the image again. Filters compose in
call order and preserve alpha. They work on [SDR ↗](https://en.wikipedia.org/wiki/Standard-dynamic-range_video) sRGB colors. Blur, LUT grading,
embedded color profiles, EXIF orientation and [HDR ↗](https://en.wikipedia.org/wiki/High_dynamic_range) handling are not implemented.

## Set a loading budget for a scrolling gallery

Share a loader across related images. If loading competes with scrolling, you can
limit how many new sources the renderer adopts in each scene:

```rust,ignore
use xgpui_media::{FrameUploadLimits, MediaLimits, UploadLimits};

let loader = MediaLoader::with_upload_policy(
    MediaLimits::default(),
    UploadLimits::default(),
    FrameUploadLimits { max_uploads: 1, ..Default::default() },
)?;
```

This example permits one new source per scene. Cached images still draw normally.
The default frame byte limit is 64 MiB; an image's full mip chain must fit that
limit. An oversized source reports an error, so resize it or raise the byte limit.
A busy loader rejects new decode requests; keep a small application queue and
retry as slots become available. This is not a promise that every frame will meet
its timing budget on every machine.

## Show frames from a camera or capture source

The producer supplies a `VideoFrame`; the view displays it. This is useful for a
live preview where showing the newest available frame matters more than retaining
old frames.

```rust,ignore
use xgpui_media::{VideoStream, video};

let (sender, stream) = VideoStream::channel();
// Keep stream in the view; give sender to your frame producer.
let preview = video(stream.clone()).w(px(640.)).h(px(360.));
```

```rust,ignore
// Producer: frame is a VideoFrame supplied by your capture/decoder adapter.
sender.publish(frame)?;

// UI: notify your view after changing playback state.
stream.set_playing(false);
```

The channel keeps one pending frame and one displayed frame. It does not open a
movie file, decode video, play audio, seek, or synchronize playback. Native frame
imports have ownership and synchronization requirements; see the adapter details
below before integrating a producer. System playback adapters are tracked in
[issue #40](https://github.com/gpui-luma-dev/xgpui/issues/40).

## Related: drawing a route or progress stroke

Trim and dash effects live in `xgpui-svg`, alongside the media work. A map route
can use dashes; a progress indicator can reveal a fraction of a curve:

```rust,ignore
use gpui::px;
use xgpui_svg::path_effects::PathEffect;

let effect = PathEffect::new()
    .trim(0.0..0.65)?
    .dash(&[px(12.), px(6.)], px(0.))?;
let stroke = measured_path.stroke(&effect, px(3.))?;
// Paint the resulting GPUI path in a canvas: window.paint_path(stroke, color).
```

Here `measured_path` is a retained `MeasuredPath` built from your contours. Measure
once, then change the effect as progress updates. See the
[contour effects guide](svg-rendering.md) for constructing and painting a path.

## Try the examples

- [Image filters](../examples/extensions/?demo=media-filters): compare the original
  with brightness, contrast, saturation, hue and tint adjustments.
- [Mipmapping](../examples/extensions/?demo=mipmaps): shrink the same image through
  an ordinary [atlas ↗](https://en.wikipedia.org/wiki/Texture_atlas) and a dedicated mipmapped texture.
- [Trim / dash](../examples/extensions/?demo=path-effects): change trim, dash pattern,
  phase and stroke width on open and closed curves.

These browser demos use small generated fixtures. Their first upload is
synchronous; they do not demonstrate the native background loader or native video
imports. WebGPU and WebGL2 have both been checked locally.

For the native app, run from the repository root:

```sh
just --justfile apps/xgpui-ext-lab/media-lab/justfile ui
just --justfile apps/xgpui-ext-lab/media-lab/justfile video
```

Both commands use release mode and generate their own sample content.

## Current platform status

| Area | Status |
| --- | --- |
| Images and color filters | Metal/wgpu tests pass on macOS; D3D11 cross-compiles. |
| Browser sampling, filters and contour effects | Checked on WebGPU and WebGL2 in Chromium on macOS. |
| macOS native frames | [BGRA ↗](https://en.wikipedia.org/wiki/RGBA_color_model) and 8-bit [NV12 ↗](https://en.wikipedia.org/wiki/YCbCr) with explicit [BT.601 ↗](https://en.wikipedia.org/wiki/Rec._601)/[BT.709 ↗](https://en.wikipedia.org/wiki/Rec._709) selection are tested. |
| Windows DXGI and Linux DMA-BUF frames | Implemented and cross-compiled; native runtime checks are pending. |

**60 Hz is the performance baseline.** Three sustained-scroll runs on an M4 Max
completed 252 large-image replacements without errors. Loading redraw p95 was
17.57–17.66 ms, close to cached rendering. These are UI redraw measurements on that
machine, not GPU timings or results for lower-powered hardware. Raw reports and
repeatable commands are available below; 120 Hz checks are additional coverage.

## Further details

The following sections contain the resource contracts, platform-specific APIs and
validation records for integration and review.

<details>
<summary>Loading and renderer details</summary>

## Loading, ownership and bounds

- PNG, JPEG and WebP first-frame decoding and CPU mip generation run on bounded
  native workers. Mips use area integration in linear sRGB with associated alpha,
  then return to the existing [straight-alpha ↗](https://en.wikipedia.org/wiki/Alpha_compositing) encoded-sRGB compositor contract.
  Odd dimensions and single-row/column sources retain their edge contributions.
- `MediaLimits` bounds active jobs, encoded input bytes, per-image mip payload,
  and aggregate admitted payload. Admission rejects excess work immediately;
  there is no unbounded queue. Share one loader across related views.
- Each job conservatively reserves `max_image_bytes` before decoding. The
  reservation follows image clones until the last snapshot drops. Unused reserved
  capacity is not reclaimed early. Defaults: two workers, 64 MiB/image and encoded
  input, 256 MiB aggregate admitted payload. Dimensions are at most 16384 on each
  axis and must fit the actual backend device limit.
- These are **source payload bounds**, not a total process/VRAM guarantee.
  Encoded input, codec scratch, retained CPU pixels, GPU pixels and temporary
  padded staging buffers have separate costs. One GPU source is cached per image;
  switching device/queue/backend replaces that cache. In-flight/retained scenes
  may retain older GPU resources until normal scene/command release.
- Dropping/cancelling a job suppresses publication. Cancellation is checked before
  decode and during mip generation; an active codec decode finishes before its
  worker slot and reservation can be released. `try_take` transfers an image once.
- `LoadStatus::Ready` means CPU-ready. The first paint starts background GPU
  staging and returns a cached transparent 1x1 placeholder. Once staging completes,
  renderer preparation adopts the prepared resources and orders the upload before
  sampling. Retained renders and layout-size changes reuse the destination texture.
- `UploadLimits` independently bounds active staging workers and accounted
  staging reservations. Defaults: two upload workers and 128 MiB staging.
  Metal/wgpu admission leases survive submission until GPU completion; D3D11
  currently releases its reservation when submission is observed.
  A busy image reports `Queued` and retries on later paints without creating an
  unbounded task queue. A source larger than the entire staging budget reports
  `Failed` rather than remaining queued forever. Admission is not FIFO.
- Staging accounting uses each mip's row size rounded up to 256 bytes. This matches
  Metal/wgpu buffer layouts; D3D11 driver allocation costs may differ. The staging
  reservation covers preparation and adoption. On Metal/wgpu both the image and
  GPU completion guard must release it; observing submission drops the image's
  reference, and the guard keeps it alive until completion. Dropping a pending
  image or abandoning an unsubmitted recording releases its references safely.
  A running worker releases its resources on return.
- Metal/wgpu completion callbacks return exclusive buffers to a loader's
  queue-bound pool; a worker cannot refill one while the GPU may still copy it.
  Idle buffers are reusable only at the same total padded size. Idle mismatches
  are evicted when needed. `staging_pool_stats()` reports allocated/cached bytes,
  allocations and reuse counts; the current pool's byte capacity equals
  `max_staging_bytes` and includes borrowed as well as idle buffers.
- GPU destination textures and driver overhead are not part of staging limits.
  Queue/device replacement can temporarily retain old in-flight resources along
  with the new pool. Shared admission still bounds outstanding staging leases;
  the pool's idle cache has its own capacity. These limits are not a hard total
  process/VRAM cap. Hidden retained images can hold staging references until
  painted, polled or dropped. There is no automatic visibility eviction.
- Queue/device changes invalidate the cache and discard previous-generation worker
  results. Abandoned wgpu/D3D11 recordings can adopt the same staging resources
  again; there is no new pixel copy. `retry_upload()` invalidates an upload or
  retries a failure. Notify the host view after an explicit retry.

## Backend contract and current limits

Native Metal and DirectX 11; enable feature `wgpu` for the portable renderer.
The backend contexts expose additive `create_image_output(&ImageTextureData)`
methods. Existing producer outputs still require the exact requested display
extent. Image outputs explicitly opt into source-sized textures. Existing scene
composition applies ordering, clipping, rounded coverage and inherited opacity.

Metal workers allocate private textures, fill exclusive shared staging buffers
and record uncommitted blits inside an autorelease pool. An upload command's
completion callback returns its buffer and releases admission ownership. wgpu
workers fill exclusive mapped buffers; the host records copies and attaches
resource/admission guards to the recording. Queue completion releases those guards.
Abandoned wgpu recordings release guards without submission, leaving the staged
source available for a retry. Mapping a reused wgpu buffer may wait for driver
progress on the worker (bounded to five seconds), never on the UI preparation path.

D3D11 workers still initialize a one-use CPU staging texture and allocate the
destination; the renderer records a deferred `CopyResource`. D3D11 does not yet
pool these textures or claim GPU-completion accounting. Workers never submit or
execute commands or access its immediate context. Native Windows validation is
required before adding completion-driven D3D11 reuse.

For wgpu/D3D11, `image_upload_context().stage(data)` remains the unpooled API and
`adopt_image_upload(staged)` records the handoff. wgpu also offers queue-bound
`staging_pool` / `stage_with_pool` and completion-guarded adoption. Metal provides
`stage_image_upload_with_pool`. Existing synchronous convenience APIs remain
available. `xgpui-media` uses the pools for full Metal/wgpu sources and allocates
only a tiny placeholder during preparation.

`image.upload_status()` reports `Idle → Queued/Preparing → Staged → Uploading →
Ready`, or `Failed(error)`. `Ready` means host-submitted, with queue ordering before
sampling; it does not assert GPU completion. `image_view` and `MediaImage::paint`
request redraws until ready or failed. A caller that inserts raw `CustomDraw`
records must drive repaint/status polling itself, including headless tests.
Empty, fully clipped and zero-opacity media paint calls do not start uploads or
request loading redraws. CPU job loading still requires host notification/polling
as described above.
Use `loader.upload_stats()` to inspect active jobs and outstanding staging reservations.

### Per-frame admission

`MediaLoader::with_upload_policy(media_limits, upload_limits, frame_limits)` adds
`FrameUploadLimits` independently of decode and staging admission. Defaults are
at most two newly adopted sources and 64 MiB of unpadded mip payload per scene
frame. All images sharing a loader share this budget. Cached source draws and the
1x1 placeholder do not spend it; duplicate placements reuse the first adoption.
When exhausted, prepared images stay `Staged`, display their placeholder and try
again next frame. The byte/count counters reset on the next scene preparation.

GPUI's additive `CustomContent::prepare_in_frame` passes a shared
`PreparationFrame`; its default forwards to the previous request-aware method.
The window geometry adapter preserves both logical coordinates and this context.
Direct low-level `prepare` calls get an independent one-call frame; custom
wrappers should forward `prepare_in_frame` to preserve scene-wide admission.

A source must fit the configured frame byte limit in full. Larger sources report
`Failed` before worker allocation, rather than being queued forever or silently
exceeding the cap. Raise the limit or use a smaller source. Chunked mip uploads,
per-frame time limits, priority/FIFO fairness and GPU timestamp measurement remain
follow-ups. The release benchmark below measures CPU work and staging reuse. Pooling is a bounded cache, not a fixed two-buffer ring. There is no
zero-stutter or frame-rate guarantee.

SDR sRGB input only. Embedded [ICC ↗](https://en.wikipedia.org/wiki/ICC_profile), EXIF orientation and HDR interpretation are not
implemented. Mip generation is alpha-aware; the existing compositor still samples
straight encoded channels between texels/mip levels. This is not a fully linear,
[premultiplied ↗](https://en.wikipedia.org/wiki/Alpha_compositing) image-compositing pipeline.

## Upload status example

```rust,ignore
use xgpui_media::{MediaLoader, MediaLimits, UploadLimits, FrameUploadLimits, UploadStatus};

let loader = MediaLoader::with_upload_policy(
    MediaLimits::default(),
    UploadLimits { max_jobs: 2, max_staging_bytes: 128 * 1024 * 1024 },
    FrameUploadLimits { max_uploads: 2, max_bytes: 64 * 1024 * 1024 },
)?;
// After taking a CPU-ready MediaImage from its ImageJob and painting it:
match image.upload_status() {
    UploadStatus::Ready => { /* commands submitted; source can be sampled */ }
    UploadStatus::Failed(error) => { /* show error; offer image.retry_upload() */ }
    _ => { /* placeholder / staging / host handoff pending */ }
}
let staging = loader.upload_stats();
let cache = loader.staging_pool_stats();
```

## Image color filters

```rust,ignore
use xgpui_media::{image_view, ImageFilter};
let filter = ImageFilter::new()
    .brightness(1.05)
    .contrast(1.15)
    .saturation(1.3)
    .hue_rotate(15.)
    .tint([1., 0.7, 0.3, 0.1]);
let element = image_view(image.clone()).filter(filter).w(px(640.)).h(px(400.));
// Low-level painting:
image.paint_filtered(window, bounds, radii, filter);
// Independent replayable custom content using the same retained source:
let placement = image.filtered(filter);
```

Each filter chain compiles to three affine RGB rows. Metal, wgpu and native D3D11
apply the rows after texture sampling, before native coverage and inherited
opacity. No extra render target, pass, decode or source upload is introduced.
Different placements may use different filters while sharing the same image cache.
The default is identity; existing image and custom-content callers keep their
current output. This API applies to SDR media, not wide-color native backgrounds.

Adjustments operate on **encoded sRGB** channels; mip generation still operates in
linear light. Operations compose in call order. Brightness multiplies RGB; contrast
uses a 0.5 pivot; saturation uses Rec.709 luminance coefficients; hue rotation is in
degrees; tint mixes toward its RGB by its fourth component and preserves source
alpha. RGB clips to the SDR target range after the combined transform, rather than
between operations. These are color adjustments, not ICC color management.

Factors clamp to 0..8, nonfinite factors use identity, and nonfinite hue uses zero.
Tint components clamp to 0..1 (nonfinite becomes zero). A composition that overflows
is ignored. Blur and LUT grading remain pending.

The media lab has five filter controls and **Reset filters**. Only the right panel
is filtered, so the left remains a source reference. Watch the upload counter while
changing filters: it should stay at one. Append `--vivid` to `ui` or a headless run
to start with combined brightness, contrast and saturation adjustments:

```sh
just --justfile apps/xgpui-ext-lab/media-lab/justfile ui --vivid
cargo run --release --offline --locked --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin media-lab --target-dir target -- --headless --vivid --png /tmp/media-filter.png
```

The lab also has a **Path effects** view; launch it with `just --justfile apps/xgpui-ext-lab/media-lab/justfile paths`. See the SVG rendering guide for contour semantics and limits.

## GPUI public API changes: additions and compatibility

[GPUI API additions](gpui-public-api.md#added-source-sized-media-textures)
cover `ImageTextureData`, image output constructors and trilinear compositor
sampling. Existing ordinary custom outputs still require display-sized extents;
no public scene struct layout or existing method signature changes in this slice.

</details>

<details>
<summary>Native video adapters and ownership</summary>

## Video snapshots — first native slice

`VideoStream`, `VideoSender`, `VideoFrame`, `video` and `video_frame` separate frame
production from presentation. The portable channel retains **one pending frame and
one displayed frame**, replacing obsolete pending work instead of building a FIFO.
Native GPU in-flight references have separate lifetimes; this is not a total memory
budget or a playback clock. Source timestamps are metadata. The channel itself
has no decoding, audio sync or seeking. System playback adapters are separate
follow-up work outside the GPU surface scope.

```rust,ignore
use xgpui_media::{VideoStream, VideoFrame, video};
let (sender, stream) = VideoStream::channel();
// Retain stream in your view and move sender to your frame producer.
let viewport = video(stream.clone()).w(px(640.)).h(px(360.)).rounded(px(16.));
// Producer: sender.publish(frame)?;
// UI controls: stream.set_playing(false); cx.notify();
// Resume selects the newest pending frame, rather than playing old frames.
```

The live element requests animation frames while visible, playing and open. Paused,
closed, empty, fully clipped and zero-opacity viewports request none. The host must
notify its view after changing presentation state; paused/closed elements do not
subscribe to background wakeups. Closing/dropping the sender retains the final
frame. Dropping the last stream consumer closes admission and releases channel-held
snapshots. `snapshot()` observes pending content when playing; `stats()` reports
published, superseded-before-observation, observed, pending, closed and playing
state. Observed counts are host snapshots, not GPU completion or presentation
fences. Multiple stream clones share pause/display state. For consistent duplicate
placements, capture once in your render and use `video_frame(snapshot.clone())` in
each viewport. `video_frame` itself does not request redraws.

### macOS BGRA / IOSurface import

```rust,ignore
use xgpui_media::{CoreVideoFrame, VideoFrame};
// pixel_buffer comes from an external producer/decoder. It must be IOSurface and
// Metal compatible, with completed writes and opaque or straight-alpha BGRA8.
// SAFETY: the producer cannot write/recycle it while any snapshot clone exists.
let native = unsafe { CoreVideoFrame::from_bgra(pixel_buffer)? };
let frame = VideoFrame::core_video(native, source_timestamp);
sender.publish(frame)?;
```

Native Metal creates a BGRA texture view of the retained pixel buffer: no CPU pixel
readback, image staging upload or display-sized intermediate is used. Each snapshot
caches up to four queue-bound imports and reuses the view on resize/replay. Import
failures and unsupported formats return errors. The renderer retains producer
snapshots, pixel buffers and legacy CoreVideo views until sampling completion,
including when a scene is replaced before the GPU finishes. Construction is unsafe
because external pixel-buffer mutation and producer synchronization cannot be
proved by a retained reference alone. Premultiplied alpha, orientation transforms,
HDR metadata/color management and transfer conversion remain pending. The new
element also supports explicit SDR NV12 conversion as described below.

### macOS NV12 — direct two-plane sampling

```rust,ignore
use xgpui_media::{CoreVideoFrame, Nv12Matrix, VideoFrame};
// SAFETY: initialized, synchronized, immutable IOSurface/Metal-compatible NV12;
// matching SDR matrix and centered chroma. The producer cannot recycle this frame.
let native = unsafe { CoreVideoFrame::from_nv12(pixel_buffer, Nv12Matrix::Bt709)? };
sender.publish(VideoFrame::core_video(native, source_timestamp))?;
```

`from_nv12` accepts opaque 8-bit CoreVideo `420f` (full range) and `420v` (video
range), with even dimensions and matching Y / interleaved CbCr planes. The pixel
format selects range; `Nv12Matrix::Bt601` or `Bt709` explicitly selects coefficients.
No matrix inference from attachments is performed. Use the matrix matching the
producer and centered chroma. Other chroma locations, 10-bit/P010, BT.2020 and HDR
are outside this API. Matrix conversion preserves the source encoded RGB transfer;
it does not perform [transfer-function ↗](https://en.wikipedia.org/wiki/Transfer_functions_in_imaging), [gamut ↗](https://en.wikipedia.org/wiki/Gamut), primaries or HDR color management.

CoreVideo imports R8 luma and RG8 chroma texture views. The same compositor
fragment samples both planes and converts YCbCr to encoded SDR RGB before the
normal image color transform, opacity, clipping and rounded coverage. Conversion
adds no image staging upload, CPU readback, offscreen conversion target or extra
render pass. Source planes, CoreVideo wrappers and the immutable producer remain
retained through GPU completion. Resize and duplicate placements reuse the same
queue import. The public `MetalCustomOutput` fields and generic producer format
contract are unchanged; native NV12 uses a separate checked internal output.

Release captures with Metal API validation check BGRA and all four NV12 matrix /
range combinations. Every mode checks all four color bars, clipping, rounded
coverage, opacity and source reuse after resize. Fixed quantized midtone vectors
and neutral black/white endpoints check conversion coefficients/ranges separately
from the fixture generator. Native lifetime tests cover BGRA and all four NV12
combinations through sampling completion. The release UI has been exercised for
format cycling, pause/resume, replacement bursts, opacity, corners, stop and restart.

The existing `paint_surface` path now also accepts BGRA, while its NV12 full-range
behavior remains covered by a regression. Unsupported formats return an error
instead of triggering the previous format assertion; error paths end active Metal
encoders cleanly. Legacy `PaintSurface` fields and `paint_surface` signatures are
unchanged. Use the new styled element for rounded corners and inherited opacity.

`VideoFrame::new` is an advanced adapter for existing custom producers that return
source-sized backend outputs. It does not implement DXGI or DMA-BUF import. The
video lab currently requires macOS Metal. A Windows native constructor is described
below. Linux has an initial Vulkan DMA-BUF snapshot adapter described below;
full synchronization/import negotiation remains pending. Windows cross-compilation covers the portable API and initial
BGRA adapter; the interactive synthetic source remains macOS-only.

### Native video lab

```sh
just --justfile apps/xgpui-ext-lab/media-lab/justfile video
just --justfile apps/xgpui-ext-lab/media-lab/justfile video-capture
```

The source generates fresh 320×180 IOSurfaces at about 30 fps on a background
thread. It writes synthetic CPU pixels; only presentation avoids the texture
upload. Both viewports use one immutable snapshot per render. Pause holds that
snapshot while production continues. Burst produces 120 frames; pending capacity
stays at one and replacement counts rise. Resume selects the latest snapshot.
Stop closes the producer while retaining the final image; restart starts a new
source. **Format** cycles BGRA8 and NV12 full/video range with BT.601/BT.709,
restarting the generated source in that format. Opacity and corner controls exercise the styled compositor. Leaving this
view stops its producer thread. Paused diagnostics refresh on host interactions.

The headless capture checks BGRA and all four NV12 combinations for RGB values, content-mask clipping, rounded coverage, inherited opacity
and retained import reuse. The media suite covers bounded replacement/pause/close;
Metal tests cover BGRA and NV12 legacy surfaces, unsupported formats and lifetime
through GPU completion:

```sh
cargo test --release --offline --locked --manifest-path extensions/crates/xgpui-media/Cargo.toml --features wgpu --lib
# The backend has no committed lockfile: omit --locked for its standalone tests.
cargo test --offline --config .cargo/validation.toml --manifest-path gpui/crates/gpui-apple/Cargo.toml --features test-support --lib core_video
```


Video capture saves the requested BGRA PNG plus four sibling PNGs suffixed
`-nv12-full-601`, `-nv12-video-601`, `-nv12-full-709` and `-nv12-video-709`.
All media-lab launch/build/capture recipes default to release. The title identifies
RELEASE/DEBUG; `profile=dev` is an explicit development override. The synthetic
video lab generates its own frames and requires no sample file.

## Windows BGRA surfaces — initial adapter

`DxgiFrame` (re-exported by `xgpui-media` on Windows) retains a native
`ID3D11Texture2D`. `VideoFrame::dxgi(frame, timestamp)` feeds the same bounded
channel and styled video element. Import creates native views and samples the
original texture, without CPU readback, staging upload or a display-sized copy.

The unsafe `DxgiFrame::from_bgra` constructor accepts only dedicated BGRA8 UNORM,
default-usage, shader-readable render targets with one mip, one array slice and no
multisampling. Preparation rejects a texture from another device. Producer writes
must be synchronized before presentation; pixels must remain immutable throughout
the native resource lifetime, including references held by queued D3D11 commands.
Dropping a Rust frame is not a pool-recycle signal. The direct constructor rejects keyed-mutex resources; use the shared-handle
constructor below for those. Device negotiation, decoder array slices and YUV
conversion remain unsupported.

Windows compilation is checked from macOS. A Windows-only WARP regression covers
native resource identity (no copy), retained pixels, resize-independent source
extent, wrong backend/device and unsupported format rejection; execution on
Windows remains outstanding. Linux DMA-BUF runtime verification, explicit
fences/pool reuse and broader surface formats remain required before Phase 5 is complete.

System file playback is tracked separately in [issue #40](https://github.com/gpui-luma-dev/xgpui/issues/40).
The surface lab generates frames itself and requires no video file.

### Keyed DXGI shared snapshots

`unsafe DxgiFrame::try_from_shared_bgra(&host_device, nt_handle.as_handle(), ready_key, release_key)`
opens an NT shared handle on the renderer's D3D11 device. Supply the host device
(the device exposed by `Dx11CustomContext`), not an unrelated producer device.
The handle is borrowed for open only; the imported COM texture retains the native
allocation after the handle closes. Legacy DXGI handles are not accepted.

The surface must satisfy the BGRA descriptor restrictions above and expose a
keyed mutex. The producer writes under its key and releases with `ready_key`.
Acquisition uses timeout zero: `Ok(None)` means busy, so retain the previous
snapshot and retry later; it never waits for the producer. Only exact `S_OK`
grants ownership. DXGI timeout is distinct from abandoned ownership, which returns
an error requiring surface recreation. Producer and consumer keys must differ.

A successful frame owns a shared read lease. Frame clones and prepared compositor
outputs keep that lease alive. The last reference releases with `release_key`
after referring command lists have been submitted or discarded; DXGI provides
the GPU ownership handoff. The producer may write again only after acquiring that
key. Merely dropping the producer's published frame handle is not sufficient:
the video channel's last displayed snapshot may still hold it. Use multiple shared
allocations for a progressing stream; do not wait on the UI thread for the
currently displayed allocation. All native aliases must obey this protocol.
A failed release is logged; a lost/abandoned device or resource must be recreated.
This API supplies snapshots, not a decoder, media clock or generic mutable surface.

The Windows-only shared-surface regression uses separate WARP devices, tests busy
acquisition and borrowed-handle closure, verifies clones/prepared outputs delay
producer access, renders the shared pixels through the compositor, then checks
producer-key handoff and captured color. The tests compile from macOS but have
not yet been executed on Windows. On a Windows SDK host, run in release mode:

```sh
just --justfile apps/xgpui-ext-lab/media-lab/justfile dxgi-test
```

See Microsoft's [AcquireSync contract](https://learn.microsoft.com/en-us/windows/win32/api/dxgi/nf-dxgi-idxgikeyedmutex-acquiresync),
[ReleaseSync contract](https://learn.microsoft.com/en-us/windows/win32/api/dxgi/nf-dxgi-idxgikeyedmutex-releasesync)
and [NT-handle import API](https://learn.microsoft.com/en-us/windows/win32/api/d3d11_1/nf-d3d11_1-id3d11device1-opensharedresource1).

## Linux DMA-BUF snapshots — initial Vulkan adapter

With `xgpui-media`'s `wgpu` feature, `DmaBufLayout`, `DmaBufFrame` and
`VideoFrame::dma_buf` provide an opt-in native snapshot path. This enables
`gpui-wgpu/dma-buf`. On Linux, device creation requests wgpu's external DMA-BUF
capability only when the adapter advertises it. Existing GLES/fallback rendering
still works; DMA-BUF import reports unsupported instead of uploading CPU pixels.
`DmaBufFrame::supported_by(&context)` checks the basic host capability before an
application attempts import. A specific allocation can still be rejected by the
driver. This does not negotiate a producer's device or layout.

```rust,ignore
use xgpui_media::{DmaBufFrame, DmaBufLayout, VideoFrame};
let layout = DmaBufLayout::linear_bgra(source_size, stride, offset, drm_modifier)?;
// SAFETY: dedicated, initialized, immutable BGRA allocation; producer fence and
// matching Vulkan EXTERNAL/GENERAL release have already completed.
let snapshot = unsafe { DmaBufFrame::from_bgra(owned_dma_buf_fd, layout) };
sender.publish(VideoFrame::dma_buf(snapshot, source_timestamp))?;
```

The initial contract accepts exactly one linear `DRM_FORMAT_ARGB8888` plane
(little-endian BGRA bytes), opaque or straight alpha. Explicit modifier zero,
aligned row stride/offset, dimensions 1..16384 and a 256 MiB layout ceiling are
validated. Device limits are checked at import. Tiled/implicit modifiers, YUV,
multiple planes and premultiplied alpha are excluded.

The unsafe constructor transfers an `OwnedFd`. It must be a real DMA-BUF matching
the supplied metadata and backed by a compatible allocation on the same physical
GPU/driver. Producer writes **and its fence** must have completed. This slice
accepts Vulkan exports released to `VK_QUEUE_FAMILY_EXTERNAL` in `GENERAL` layout;
GBM/EGL allocations requiring `FOREIGN_EXT` ownership are outside this contract.
It does not import/wait for a sync-file fence, perform a CPU wait, return a release
fence, or signal that a pool allocation may be recycled. Pixels must stay immutable
for the allocation's entire native lifetime, including GPU references after the
Rust frame drops. Use dedicated snapshots; mutable decoder pools are unsupported.

Preparation duplicates the FD and lets Vulkan consume that duplicate, preserving
the frame's original FD for import retry after an abandoned preparation. It records
the external queue-family acquire on the host preparation encoder, preserving
`GENERAL`. A completion guard retains that raw-barrier resource even if the main
render fails before tracked sampling; wgpu then tracks the normal shader-read layout transition. Existing
pixels are retained rather than treated as uninitialized storage. One host queue
owns the cached import. Repeated placements/resizes reuse it; a different queue is
rejected. No CPU readback, image staging upload, or display-sized copy is performed.
The imported native memory retains its payload through GPU resource lifetime.

The Linux implementation and media API are cross-checked from macOS. Release host
tests validate padded rows/offsets and rejection of invalid dimensions, alignment,
modifiers, overflow and excessive layout sizes. These tests do not exercise a
Linux GPU. Native FD import, validation-layer synchronization and pixel capture
remain **unverified** until run on a compatible Linux Vulkan host. Phase 5 remains
incomplete. The existing interactive synthetic source is still macOS-only.

```sh
# Layout contract checks on the current host (no DMA-BUF/video fixture required):
just --justfile apps/xgpui-ext-lab/media-lab/justfile dma-buf-layout-test
```

The ownership/FD contract follows the Vulkan [external FD import specification](https://docs.vulkan.org/refpages/latest/refpages/source/VkImportMemoryFdInfoKHR.html)
and [queue-family synchronization rules](https://docs.vulkan.org/spec/latest/chapters/synchronization.html#synchronization-queue-transfers).

### Generated Linux native validation fixture

On a compatible Linux Vulkan host, run:

```sh
just --justfile apps/xgpui-ext-lab/media-lab/justfile dma-buf-native-test
```

This recipe defaults to release and supplies its own raw GPU test image; there is
no sample video, decoder, GBM dependency, or user asset to load. It requires a
Vulkan adapter with linear BGRA DMA-BUF **export and import** support and an
installed `VK_LAYER_KHRONOS_validation` layer. The recipe selects Vulkan, enables
wgpu validation/debug flags and requests that layer. Unsupported capabilities or
missing validation support fail explicitly; they do not count as passing tests.
The native test is ignored during ordinary test runs and is selected explicitly
by this recipe, with one test thread to serialize raw producer queue work.

The fixture creates an exportable linear BGRA Vulkan image and uploads a generated
four-quadrant red/green/blue/white pattern on the **producer** side. It waits for
the test producer fence and releases ownership in `GENERAL`, exports an owned
DMA-BUF FD, then destroys its original image/memory resources. Only the FD and
queried row/offset metadata reach the importer. GPUI imports that native payload
and samples it directly. This test-source upload does not change the zero-upload
presentation path, and the test-only producer wait is not a UI-thread decode loop.

The checks cover abandoned preparation/reimport, retained FD payload after
exporter teardown, correct channel order and orientation, native corners and
content mask, opacity, duplicate placements, source reuse after resize, wrong
backend/queue rejection, and dropping producer snapshots while sampling or raw
preparation work is queued. A Vulkan debug messenger counts native validation
errors and fails the test even if pixel checks pass. Completion is checked before
that messenger is destroyed.

A successful native run saves `media-lab-dma-buf.png` in the host's temporary
directory (normally `/tmp/media-lab-dma-buf.png`) and prints its path. It shows a
rounded opaque four-quadrant surface and a second partially clipped half-opacity
placement. Headless readback is used only to validate the rendered target.

The harness and importer cross-compile for Linux from macOS. **No successful Linux
GPU run is claimed yet.** Pixel results, validation-layer behavior and driver
compatibility remain pending execution on a compatible Linux host. The interactive
video lab remains macOS-only; this is a native generated-surface regression, not a
new playback feature.

</details>

<details>
<summary>Test commands and browser implementation notes</summary>

## How to test

Run commands from the repository root. Use an ordinary desktop terminal with GPU
access for pixel tests. `--offline` assumes dependencies have already been fetched;
omit it on a first setup that needs downloads.

### Media unit and GPU tests

```sh
cargo test --release --offline --locked --manifest-path extensions/crates/xgpui-media/Cargo.toml --features wgpu --lib
```

On macOS this runs sixteen tests, including both native Metal and wgpu pixel checks.
The tests generate temporary checkerboard PNGs, decode and mipmap them, render at
multiple sizes, and assert pixel values. They also check clipping, rounded corners,
opacity, native scene order, texture reuse, loader budgets, cancellation and errors.
Upload tests verify the first-paint placeholder, bounded worker/staging admission,
queued progress after handoff, failure/retry status, different pixels after
buffer refill, and independent frame count/byte admission.
Expected result: `16 passed; 0 failed`. The suite does not open a window.

For only CPU loading/mipmap tests, use these filters:

```sh
cargo test --release --offline --locked --manifest-path extensions/crates/xgpui-media/Cargo.toml --lib mipmaps::tests
cargo test --release --offline --locked --manifest-path extensions/crates/xgpui-media/Cargo.toml --lib tests::loader_
```

On macOS the build can still invoke the Metal compiler even when only CPU tests
are selected. Xcode's Metal tools and a writable compiler cache are required.

### Core data and wgpu recording contracts

```sh
cargo test --offline --locked --config .cargo/validation.toml --manifest-path gpui/crates/gpui/Cargo.toml --lib image_texture::
cargo test --offline --locked --config .cargo/validation.toml --manifest-path gpui/crates/gpui/Cargo.toml --lib scene::preparation_tests
cargo test --offline --locked --config .cargo/validation.toml --manifest-path gpui/crates/gpui-wgpu/Cargo.toml --lib custom_draw::tests
```

Expected results: three core data/pool/admission tests, five preparation/adapter
compatibility tests, and ten wgpu contract tests.
These preserve ordinary output extent checks and verify that pending/abandoned
image recordings cannot be reused as submitted work. A worker-staged source is
readopted after an abandoned recording and rejected by a foreign queue identity.

### Windows compilation and native execution

From macOS, with the Windows Rust target and Homebrew LLVM installed:

```sh
PATH="/opt/homebrew/opt/llvm/bin:$PATH" cargo check --offline --locked --manifest-path extensions/crates/xgpui-media/Cargo.toml --target x86_64-pc-windows-msvc
PATH="/opt/homebrew/opt/llvm/bin:$PATH" cargo check --offline --locked --config .cargo/validation.toml --manifest-path gpui/crates/gpui-windows/Cargo.toml --features test-support --tests --target x86_64-pc-windows-msvc
```

Both compilation checks pass locally. They do not execute D3D11. On Windows, run
its recording/readiness tests in a terminal configured with the native build tools:

```sh
cargo test --locked --config .cargo/validation.toml --manifest-path gpui/crates/gpui-windows/Cargo.toml --features test-support --lib custom_draw::tests
```

Native D3D11 execution remains unverified until that command runs on Windows.

### Web documentation preview

```sh
cargo run --locked --manifest-path tools/crate-roadmap/Cargo.toml
node docs/verify.mjs
```

Open `docs/index.html#media-images` in a browser, or select **Extensions →
xgpui-media**. The source guide, crate API/source/dependency tabs and compatibility
notes are embedded in the offline site. Regenerate after editing completed-feature
status or source code; generated `data.js` and source pages are not committed.
Publishing uses the repository's normal Pages workflow.

## Visual media lab

From the repository root:

```sh
just --justfile apps/xgpui-ext-lab/media-lab/justfile ui
just --justfile apps/xgpui-ext-lab/media-lab/justfile ui --image /absolute/path/photo.png
just --justfile apps/xgpui-ext-lab/media-lab/justfile capture
just --justfile apps/xgpui-ext-lab/media-lab/justfile wgpu-capture
```

The app generates frequency, transparent-edge and color-detail samples offline.
Both image panels share zoom and pan. Scroll to zoom, drag to pan, and use the
opacity and corner controls to inspect compositing and clipping. Sweep animates
minification; it respects the system reduced-motion preference.

Choose **Burst ×12** to load fresh snapshots under bounded admission. Watch queued,
staging and submitted states, source retirement, staging-pool reuse and redraw
intervals. Change the per-frame count/byte presets and repeat. READY means the
upload was submitted by the host; it does not report GPU completion. Diagnostics
exclude atlas memory, and redraw intervals are not GPU timings.

Headless capture writes `/tmp/media-lab.png`: atlas on the left, mipmapped media on
the right, with one row per sample. It checks that each source uploads once and
fails on upload errors or timeout. The optional `wgpu-capture` recipe uses wgpu
for headless rendering; the interactive macOS app uses Metal. See
`apps/xgpui-ext-lab/media-lab/README.md` for controls and visual checks.

## Browser demos: filters, mipmapping and contour effects

The WASM gallery includes three GPUI/wgpu demos:

- [Image filters](../examples/extensions/?demo=media-filters): original and adjusted
  placements share one dedicated source texture. Cycle brightness, contrast,
  saturation (including grayscale), hue and tint; reset, vary opacity/corners,
  zoom and animate. The preparation counter stays at one on ordinary redraws.
- [Mipmapping](../examples/extensions/?demo=mipmaps): compare the ordinary single-level
  image atlas with a source-sized 11-level [trilinear texture ↗](https://en.wikipedia.org/wiki/Trilinear_filtering). Cycle zoom through
  90%, 50%, 25% and 12.5%, or animate it to inspect [aliasing ↗](https://en.wikipedia.org/wiki/Aliasing). Both panels use the
  same generated RGBA fixture, geometry, opacity and clipping.
- [Trim / dash](../examples/extensions/?demo=path-effects): compare original,
  trimmed and trimmed/dashed open curves, a closed heart and multiple contours.
  Cycle zero/full trim, solid/even/odd dash arrays, phase and stroke width;
  animate trim and dash phase. This uses `xgpui-svg::path_effects` before GPUI
  stroke tessellation. Animation respects the browser's reduced-motion preference.

Each link supports `&backend=webgpu` or `&backend=webgl` for forced backend checks.
The automatic entry prefers WebGPU and can fall back to WebGL2. Build in release:

```sh
PATH="/path/to/matching/wasm-bindgen/bin:$PATH" bash apps/xgpui-ext-lab/extensions-web/build.sh --release
python3 apps/xgpui-ext-lab/extensions-web/serve.py
```

Use the pinned wasm-bindgen version in the app README. The build refreshes both
standalone demo output and the generated `docs/examples/extensions/` assets;
publication still requires the normal site deployment. No deployment is implied
by a local build. See [demo source](https://github.com/gpui-luma-dev/xgpui/tree/main/apps/xgpui-ext-lab/extensions-web/src/media.rs).

The small 1024² offline fixture uses the public synchronous
`xgpui_media::mipmapped_rgba8(width, height, rgba)` helper. It builds the same
alpha-aware linear-light mip chain used by the native loader and returns validated
`gpui::ImageTextureData`. It validates the full payload (64 MiB maximum) but does
not reserve loader budgets or schedule work. The browser app caches the result of
wgpu's synchronous `create_image_output`; it shares the real filter transform and
compositor path. Fixture generation and the first upload run on the browser thread.
This is portable presentation coverage, **not** a browser port or benchmark of
`MediaLoader`'s native worker-based decode/staging pipeline. No external assets,
video decoder, system surface import or worker/shared-memory setup is involved.

Local release acceptance on 2026-10-26, in-app Chromium on Apple M4 Max: all three
demos rendered on forced WebGPU and forced WebGL2. Checks covered filter presets
and reset, preserved transparent edges, opacity/corners, retained texture reuse,
strong minification and zoom animation, zero/full trim, closed contours, odd dash
arrays, phase/width changes and animation. Narrow comparison panels keep fixed
header heights so label wrapping does not shift the images. No console errors
were observed. WebGL2 logged its expected dual-source blending limitation and a
resize poll timeout while continuing to render; these checks do not establish
GPU timing, browser-wide compatibility or accessibility certification.

</details>

<details>
<summary>Benchmark results and review notes</summary>

## Release upload CPU benchmark

```sh
just --justfile apps/xgpui-ext-lab/media-lab/justfile upload-bench
just --justfile apps/xgpui-ext-lab/media-lab/justfile upload-wgpu-bench
```

Both commands generate their own inputs and require no user assets. Each writes
`/tmp/media-upload-bench[-wgpu].json` and a matching PNG. The native recipe selects
Metal on macOS; the portable recipe enables wgpu. Debug builds are rejected.
For custom output paths, invoke the lab with `--headless --upload-bench
--bench-report PATH --png PATH` (plus `--wgpu` / `--features wgpu` for portable).

The workload uses six deterministic noisy 2048×2048 PNGs with solid center markers,
full mip chains (about 128 MiB retained source payload), and twelve placements in
a 900×600 scene. It warms the pipeline/target with a small source, decodes/mips
all full sources before timing, then stages three waves by invalidating each
source cache between waves. Admission allows two upload workers, 64 MiB staging,
and one source / 32 MiB payload per scene frame. The scene is paced at roughly
60 Hz. A final 120-frame phase renders the same full-source scene from cache.

The JSON stores every frame's `render_scene` CPU wall duration and summed source
preparation duration, plus nearest-rank p50/p95/p99/max and sample counts. It also
records decode/mip wall time separately, observed newly submitted sources and
staging peaks, pool allocations/reuses, per-source upload counts, backend/build
metadata and interpretation caveats. Assertions check per-frame admission, drain
and timeout behavior, all source submissions, reuse bounds and both placements'
final colors. Final render-target readback validates pixels outside timing.

These timings exclude PNG generation, pipeline warmup, decode/mips, pacing sleep
and final readback. They are **not GPU timestamps, whole-window frame times or a
zero-stutter guarantee**. Driver backpressure can affect CPU time; instrumentation
and background system load also matter. Upload-phase samples include placeholder,
staging and adoption frames, so compare sample counts and raw frames rather than
assuming each sample is an upload duration. Observed staging drain includes frame
pacing/polling and backend lifetime semantics; `Ready` alone means host-submitted.
Source/staging accounting is not a total process-memory or VRAM measurement.

### Recorded macOS observations — 2026-10-26

These are single observations from the default release workload, not performance
thresholds. No atlas comparison or window event/layout/animation workload is
included. The wgpu API used by this harness does not expose the adapter name.

| Run | Upload-phase CPU p95 / max | Upload samples | Cached CPU p95 / max | Cached samples |
| --- | --- | --- | --- | --- |
| Native Metal · Apple M4 Max | 0.440 / 0.863 ms | 24 | 0.372 / 0.440 ms | 120 |
| wgpu · macOS | 1.522 / 1.562 ms | 29 | 1.085 / 1.192 ms | 120 |

Raw report assets: [native Metal observation](../benchmarks/media-upload-metal-m4-max-2026-10-26.json)
and [wgpu observation](../benchmarks/media-upload-wgpu-macos-2026-10-26.json).
Their capture paths refer to the original run's temporary directory; regenerate
with the commands above to inspect the PNG on your own machine. Each report
includes all raw timed frames and pool evidence. Neither observation establishes
a result for other machines, Windows or Linux.

## Visible-window performance and branch review

Run `just --justfile apps/xgpui-ext-lab/media-lab/justfile ui-perf` for a release native window
and `/tmp/media-ui-perf.json`. Keep it foreground and avoid interacting until the
report is written, normally about seven seconds. After warming the primary image,
the lab requests continuous frames with a zoom sweep: 180 baseline intervals,
automatic decode/mip/staging/submission of 12 fresh 1536×1024 frequency images,
then 180 cached intervals. Admission is one source / 16 MiB per scene; existing
loader worker and payload limits still apply. Completed snapshots retire as needed.
Reduced motion disables the sweep while continuous frame requests remain active.

Every measured render-start interval is retained, including gaps over 250 ms;
burst-trigger work is included. Only the warmup-to-baseline boundary is omitted.
JSON records raw intervals, nearest-rank p50/p95, maximum, counts over 25/50 ms,
completed sources and errors. A 60-second timeout records a failure. Check for
12 completed sources and no errors. The window remains open for manual checks.
The UI's rolling interval diagnostic now excludes idle pauses by tracking whether
continuous frames were requested, rather than discarding long intervals.

Three observations on an Apple M4 Max, macOS native Metal, 1400×980-point window:

| Run | Baseline p95 | Burst p95 | Cached p95 | Burst maximum | Completed |
| --- | --- | --- | --- | --- | --- |
| 1 | 17.606 ms | 17.552 ms | 17.565 ms | 17.581 ms | 12/12 |
| 2 | 17.551 ms | 16.707 ms | 17.659 ms | 17.577 ms | 12/12 |
| 3 | 17.623 ms | 16.948 ms | 17.572 ms | 17.373 ms | 12/12 |

All three runs had no source errors and no measured intervals over 25 ms in any
phase. Each burst contributed 37 intervals. These observations suggest no upload
cadence regression for this workload; they are not performance thresholds or a
zero-stutter guarantee. Earlier exploratory runs had occasional ~26 ms baseline
and cached gaps. Background load and OS scheduling are uncontrolled.
[Raw visible-window observations](../benchmarks/media-visible-metal-m4-max-2026-10-26.json)
include all samples. Render-start cadence includes UI work, driver backpressure
and scheduling; it is neither GPU timing nor presented-frame or input-latency
measurement. Baseline has no burst previews, cached rendering retains the final
previews, and burst timing includes decode/mips and admission waits. Keep the
window visible: occlusion/minimization can change scheduling.

Manual release-window checks also exercised zoom and opacity while a burst and
sweep were active; controls updated and all snapshots reached submission. This
was a functional responsiveness check, without instrumented input latency.

The branch review covered source/queue cache invalidation, cancellation and
payload reservations, completion-driven staging reuse, shared frame admission,
filter alpha/coverage order, vector bounds, retained native surface ownership,
and additive GPUI trait defaults. It found and corrected the interval diagnostic's
long-gap filtering and clarified DXGI shared-import documentation. No additional
correctness blocker was identified in this review. Native Windows/DXGI and Linux
DMA-BUF execution remain required before claiming those platforms validated.

The implementation deliberately differs from literal RFC examples: explicit
`xgpui-media` image opt-in preserves the existing atlas path; vector effects live
in `xgpui-svg`; completion-safe bounded pools replace a fixed double-buffer ring;
submission is ordered before sampling without claiming 0 ms stalls. Fair scheduling,
chunked uploads, D3D11 pool reuse, GPU timestamps, blur/LUT grading and broader
surface formats remain follow-ups. Decoder/playback adapters belong to issue #40.

### Built-in display rerun

After disconnecting the external monitor, system display information confirmed
that the built-in 3024×1964 Liquid Retina XDR was the only connected display and
the main display. The same release executable and generated workload produced:

| Run | Baseline p95 | Burst p95 | Cached p95 | Burst maximum | Completed |
| --- | --- | --- | --- | --- | --- |
| 1 | 9.088 ms | 8.509 ms | 9.093 ms | 12.594 ms | 12/12 |
| 2 | 9.203 ms | 8.520 ms | 8.641 ms | 9.212 ms | 12/12 |
| 3 | 10.534 ms | 8.558 ms | 9.371 ms | 9.309 ms | 12/12 |

All phase medians were approximately 8.33 ms, consistent with ~120 render starts
per second. All 36 burst sources completed without errors. No measured interval
in any phase exceeded 16.67 ms; the longest burst interval was 12.594 ms.
The burst phases contained 44, 46 and 45 intervals respectively. Relative to the
earlier ~60 fps observations, the monitor change enabled a faster observed app
cadence. These are render-start measurements; actual presented frames and the
instantaneous adaptive panel refresh rate were not instrumented. This small
workload does not establish a universal 120 fps guarantee.
[Raw built-in display observations](../benchmarks/media-visible-internal-m4-max-2026-10-26.json)
retain all samples. The headless CPU benchmark deliberately paces at ~60 Hz and
is independent of the connected monitor; use this visible run to compare cadence.

## Sustained large-image scrolling — 60 Hz baseline

The baseline remains **60 Hz**. Higher-refresh testing is additional coverage,
not a requirement for developers or a substitute for 60 Hz testing.
Run `just --justfile apps/xgpui-ext-lab/media-lab/justfile scroll-perf` after selecting fixed
**60 Hertz** for the target display in System Settings → Displays. Keep the
release window visible and foreground without interacting during measurement;
restore the previous display preference afterward. The app reports the platform's
refresh interval but does not change or cap the display itself.

Four generated 3072×2048 PNGs feed a native scroll container with 24 image cards.
Each full RGBA mip payload is 33,554,428 bytes (just below 32 MiB), versus the
1536×1024 sources in the earlier short-burst test. All four source slots warm
first. The workload then scrolls at 1200 points/second, reversing at the ends:
five seconds cached baseline, 30 seconds repeated file decode/mip/upload
replacements, pending-work drain, and five seconds cached scrolling. Layout and
source-slot count stay the same. A single pending replacement starts at most
every 350 ms; its predecessor stays displayed until the replacement is submitted.
Four thumbnails exercise every retained source and the pending replacement.
Scene admission allows one new source / 32 MiB. The loader uses two decode workers,
a conservative 36 MiB/image reservation and 256 MiB aggregate reservation limit;
upload limits remain two staging workers and 128 MiB outstanding staging.

`/tmp/media-scroll-60hz.json` retains all render-start intervals, including long
gaps. It records p50/p95/max, counts above two 60 Hz periods (33.333 ms) and 50 ms,
replacement completions, errors, observed scroll travel and sampled upload peaks.
Warmup/generation are excluded; phase-transition work is included. The report is
written once; afterward, wheel/trackpad scrolling and the auto-scroll control
remain available for manual checks. Reduced motion or changing the auto-scroll
control during measurement records an error. A 90-second timeout is checked on
render; minimization or occlusion can suspend callbacks and invalidate comparison.

Provisional project acceptance criteria, defined for this workload:

- All measured phases have p95 ≤20 ms.
- Loading p95 is at most 2 ms above cached baseline.
- At most 1% of intervals exceed 33.333 ms; none exceed 50 ms.
- At least 30 replacements complete, observed scroll travel exceeds 1000 points,
  and no errors occur.

These are project regression targets, not universal definitions of acceptable
performance. A failed check calls for investigation without removing outliers.
The `target_hz` field describes the budget; independently verify fixed display
mode. A pass on an M4 Max does not establish performance on lower-powered hardware.
File data may be in the OS cache, so this is not a cold-disk benchmark. Timings are
UI render-start cadence, not GPU timestamps, input latency or presentation feedback.
Sampled resource peaks can miss short worker activity and do not bound total VRAM.

### Fixed-60-Hz observations

Three release runs on the M4 Max (40 GPU cores), built-in 3024×1964 Liquid Retina
XDR display, 1400×900-point window. System Settings was explicitly verified at
**60 Hertz** for these runs; the app reported a 16.667 ms display interval.
ProMotion was restored after testing.

| Run | Baseline p95 | Loading p95 | Cached p95 | Loading maximum | Replacements | Result |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | 17.614 ms | 17.633 ms | 17.660 ms | 17.785 ms | 84 | Pass |
| 2 | 17.594 ms | 17.566 ms | 17.537 ms | 17.699 ms | 84 | Pass |
| 3 | 17.647 ms | 17.657 ms | 17.652 ms | 17.780 ms | 84 | Pass |

All 252 replacements completed without errors or admission retries. Every phase
had a median near 16.667 ms, and no measured gap exceeded 33.333 ms or 50 ms.
Observed scroll travel covered approximately 6000 points. Loading p95 differed
from baseline by less than 0.1 ms in each run. Each run replaced approximately
2.6 GiB of full-mip payload over the 30-second loading phase. These observations
support the 60 Hz baseline for this workload on this machine; they do not
establish results on slower GPUs/CPUs or actual presented-frame delivery.

[Raw sustained-scroll observations](../benchmarks/media-scroll-60hz-m4-max-2026-10-26.json)
contain every sample, the exact criteria, per-phase summaries, completed counts,
observed scroll bounds and sampled staging evidence. Screenshot inspection
confirmed rendered cards and completed replacements in the native scroll view.

</details>


## Higher precision supplied pixels

Phase 3 adds explicitly described RGBA16F sources and native wide-SDR placement.
Use `MediaLoader::load_rgba16f` for background conversion, and
`image_view(image).wide_gamut()?` only with an accepted wide-SDR native window.
The default remains an explicit sRGB preview. This helps with scientific rasters,
design previews and output from existing image providers without adding codecs.
See [color contracts and small examples](media-pipeline.md).

Run `just media --float-images`, or try the
[RGBA16F browser preview](../examples/extensions/?demo=media-float),
[WebGPU](../examples/extensions/?demo=media-float&backend=webgpu) or
[WebGL2](../examples/extensions/?demo=media-float&backend=webgl).
Browser output is clipped sRGB; Windows/Linux native runtime QA remains pending.


## Large raster canvases

Phase 4 adds `TileCanvas`: a retained preview plus viewport-selected immutable
pages, bounded caches, cancellation and eviction. It suits document scans, maps
and scientific rasters that exceed the whole-image limit. Start
`just media --tiled` for a generated 72 MP native sample; no external image needed.

Try the [browser canvas](../examples/extensions/?demo=media-tiles),
[WebGPU](../examples/extensions/?demo=media-tiles&backend=webgpu), or
[WebGL2](../examples/extensions/?demo=media-tiles&backend=webgl).
The [pipeline guide](media-pipeline.md) includes small Rust examples, provider
contracts, budget meanings and backend limits. Browser generation is currently
one small synchronous page per frame; no new decoder is included.

### Bounded media blur (issue #26 Phase 5)

The tiled demo now includes 0/4/8/16 px [Gaussian blur ↗](https://en.wikipedia.org/wiki/Gaussian_blur) and adaptive/full-resolution
quality. It blurs assembled pages once, avoiding internal tile seams. Nonzero
blur uses a smaller preview to fit the existing 1,024-device-pixel capture limit,
including halo. Filtering is premultiplied encoded sRGB; capture/filter textures
are additional to page budgets. Media uploads/imports and target allocations are
reused, but custom media captures refresh each paint. Native `just media --video`
also offers [exposure ↗](https://en.wikipedia.org/wiki/Exposure_value), saturation, [vignette ↗](https://en.wikipedia.org/wiki/Vignetting) and blur on generated BGRA/NV12
surfaces. Native imports are not available in Wasm; test portable blur via
`?demo=media-tiles&backend=webgpu` and `?demo=media-tiles&backend=webgl`.
