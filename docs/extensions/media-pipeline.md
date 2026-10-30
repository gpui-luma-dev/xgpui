# Media pipeline contracts

Phases 1–5 of [issue #26](https://github.com/gpui-luma-dev/xgpui/issues/26) define
CPU reference behavior, retained-source linear adjustments and the contracts for the next image pipeline. The types
live in `xgpui_media::pipeline`. The existing image loader, encoded-sRGB `ImageFilter` and native video surfaces
retain their contracts. Matrix/vignette adjustments opt into a separate sampling path.

Use these contracts to agree on how an editor interprets pixels, how an asset
browser accepts background results, or how a map/document viewer plans large
raster pages. Visible matrix/vignette controls are available in the native and browser labs.
RGBA16F supplied-pixel uploads and native wide-SDR placements are available. Provider-driven progressive/tiled drawing is available below, followed by bounded preview blur and native video modifiers.

## Color and alpha

| Contract | Meaning |
| --- | --- |
| Primaries | [sRGB ↗](https://en.wikipedia.org/wiki/SRGB), [Display P3 ↗](https://en.wikipedia.org/wiki/DCI-P3) or [Adobe RGB (1998) ↗](https://en.wikipedia.org/wiki/Adobe_RGB_color_space), all [D65 ↗](https://en.wikipedia.org/wiki/Standard_illuminant) |
| Transfer | Independent linear, sRGB (also used by encoded P3), or Adobe RGB (1998) transfer |
| Source pixels | Bounded [SDR ↗](https://en.wikipedia.org/wiki/Standard-dynamic-range_video) RGB and alpha, each 0..1 |
| Association | Straight, or [premultiplied ↗](https://en.wikipedia.org/wiki/Alpha_compositing) in the declared transfer domain |
| Working pixels | [Straight-alpha ↗](https://en.wikipedia.org/wiki/Alpha_compositing), linear RGB in explicit working primaries |
| Intermediate RGB | May be negative or above one; no per-adjustment clipping |
| Output | Convert primaries, explicitly map [gamut ↗](https://en.wikipedia.org/wiki/Gamut), encode, then associate |
| Storage | [RGBA8 ↗](https://en.wikipedia.org/wiki/RGBA_color_model) or RGBA16Float metadata, independent of color interpretation |

`ImageColor::decode` unassociates before transfer decoding, then converts working
primaries using `xgpui-color`. Associated source RGB must not exceed alpha. Zero
alpha canonicalizes RGB to zero, including after matrix offsets. Alpha is finite,
bounded and unchanged by all adjustments. Nonfinite inputs, mismatched working
spaces and arithmetic overflow return errors.

`WorkingPixel::output` requires an explicit `GamutMapping::Clip` or
`GamutMapping::ReduceChroma`. This produces bounded SDR reference pixels, not an
OS swapchain payload. Native wide-SDR adapters still follow #25: Metal's linear
P3 path and Windows' [scRGB ↗](https://en.wikipedia.org/wiki/ScRGB) path have distinct presentation contracts. RGBA16F
storage alone does not establish color meaning, color profile ingestion or [HDR ↗](https://en.wikipedia.org/wiki/High_dynamic_range).

## Ordered adjustments

The new `ColorMatrix` applies affine RGB adjustments in a named **linear** working
space. Its three rows multiply `[R, G, B, 1]`; there is no arbitrary alpha row.
`ImageFilter` keeps its existing **encoded-sRGB** behavior and cannot be silently
substituted for this transform.

```rust
use xgpui_media::pipeline::{
    ColorMatrix, GamutMapping, ImageColor, ImagePrimaries,
};

// CPU reference: +1 stop doubles linear light, not the encoded sRGB number.
let input = ImageColor::SRGB.decode([0.5, 0.5, 0.5, 0.25], ImagePrimaries::Srgb)?;
let matrix = ColorMatrix::identity(ImagePrimaries::Srgb)
    .exposure(1.0)?
    .contrast(1.1, 0.18)?
    .saturation(1.2)?;
let output = matrix.apply(input)?.output(ImageColor::SRGB, GamutMapping::Clip)?;
assert_eq!(output[3], 0.25);
```

Calls compose in their written order. [Exposure ↗](https://en.wikipedia.org/wiki/Exposure_value) is in EV/stops; +1 doubles and -1
halves RGB. Contrast uses `factor * (RGB - pivot) + pivot` with an explicit 0..1
linear pivot and a nonnegative factor. Saturation uses the Y coefficients from
the working primaries' D65 XYZ matrix: 0 is grayscale, 1 is identity. Factors
must be finite and nonnegative. Finite extended intermediates survive until
output mapping, so exposure followed by its inverse can recover the original.

Temperature/white balance has no API in this phase: a meaningful illuminant and
adaptation contract must precede such a control. Affine matrices do not represent
all photographic operations.

## Vignette reference

`Vignette` darkens linear RGB after the matrix and before output mapping. Radius
is the unaffected inner radius; feather is the positive transition width.
Intensity and roundness are in 0..1. Roundness 0 follows the source rectangle's
ellipse; roundness 1 is circular in source pixels, with radius 1 reaching the
shortest image edge. Intermediate roundness blends those distance measures.

```rust
use xgpui_media::pipeline::Vignette;

let vignette = Vignette::new(0.4, 0.65, 0.35, 1.0)?;
let adjusted = vignette.apply(matrix.apply(input)?, [0.9, 0.5], [8000, 6000])?;
```

Coordinates always refer to the full source image, not a tile or the destination
widget. The feather uses ascending smoothstep bounds. This prevents the original
RFC's reversed-edge ambiguity and keeps rectangular images aspect-correct.
The GPU sampling path follows this reference without an offscreen pass.

## Render retained-image adjustments

Use the new explicit placement API for RGBA8 encoded-sRGB images:

```rust,ignore
use xgpui_media::{image_view, pipeline::{
    ColorMatrix, ImageAdjustments, ImagePrimaries, Vignette,
}};

let matrix = ColorMatrix::identity(ImagePrimaries::Srgb)
    .exposure(1.0)?.contrast(1.1, 0.18)?.saturation(1.2)?;
let adjustments = ImageAdjustments::new(matrix,
    Some(Vignette::new(0.4, 0.35, 0.65, 1.0)?))?;
image_view(image.clone()).adjustments(adjustments)
```

The source is retained/uploaded once. Changing placement parameters updates the
sampling uniforms; it does not decode, re-encode or upload source pixels again.
`MediaImage::adjusted` and `paint_adjusted` expose the same behavior to custom
placements. `.filter(...)` selects the old encoded path; `.adjustments(...)`
selects the linear path. The last setter wins, with no implicit combination.

`ImageAdjustments::new` accepts linear sRGB or Display P3 working primaries.
Coefficients/feathers that cannot be represented safely in the f32 shader ABI
return errors. RGB clips once after matrix and [vignette ↗](https://en.wikipedia.org/wiki/Vignetting) in the selected output
gamut. Alpha, native opacity, corners and clipping retain their meanings.

This helps with exposure previews in a photo library, brightness controls for
scientific imagery, or a document viewer that highlights a region with a soft
vignette. Source interpretation is explicit rather than dependent on a label like
“brightness.” It is still an SDR image path; no ICC/RAW/HDR processing is implied.

### Try the demos

```sh
just media --adjustments
just media --adjustments --headless --png /tmp/media-adjustments.png
```

The native lab generates its samples. Both panels share one [mipmapped ↗](https://en.wikipedia.org/wiki/Mipmap) source:
original on the left, adjusted on the right. Cycle exposure, contrast, saturation
and vignette; animate exposure, try Alpha edges, opacity/corners, reset controls
and zoom/pan. The source upload count should stay at one until you select/reload a
source. Toggle **Linear adjustments OFF** to return to the earlier atlas/filter
comparison. Use release mode and keep 60 Hz as the performance baseline.

After rebuilding the Wasm assets with `just web-build`, use:

- [Browser demo](../examples/extensions/?demo=media-filters)
- [Force WebGPU](../examples/extensions/?demo=media-filters&backend=webgpu)
- [Force WebGL2](../examples/extensions/?demo=media-filters&backend=webgl)

The browser starts in linear mode on a rectangular 1280 × 800 RGBA fixture.
**Use encoded filters** switches to the previous filter controls. **Animate**
sweeps exposure and zoom; the texture preparation count should remain one.
Browser fixture generation/upload is synchronous once per source, and does not
exercise native decoding/upload budgets or video imports.

## Supplied RGBA16F images

Use this for application-generated scientific rasters, design previews or pixels
from an existing decoder that already reports their color interpretation. No new
image codec is needed. The existing PNG/JPEG/WebP loader remains SDR sRGB and
does not infer [ICC ↗](https://en.wikipedia.org/wiki/ICC_profile) metadata.

```rust,ignore
use xgpui_media::{MediaLoader, image_view, pipeline::ImageColor};

// Supplied float components describe bounded SDR, not scene-referred HDR.
// Retain/poll ImageJob just like MediaLoader::load(path).
let mut job = MediaLoader::default().load_rgba16f(
    2, 1,
    vec![[1.0, 0.1, 0.0, 1.0], [0.0, 0.8, 0.2, 0.5]],
    ImageColor::LINEAR_P3,
)?;
if let Some(image) = job.try_take() {
    // Default: explicitly clipped, encoded sRGB preview.
    let preview = image_view(image.clone());
    // Requires an accepted native WindowColorMode::WideGamutSdr window.
    let wide = image_view(image).wide_gamut()?;
}
```

Set `ImageColor { primaries, transfer, alpha }` for other supplied conventions:
sRGB/Display P3/Adobe RGB (1998)/Rec.2020 primaries, independent linear/sRGB/Adobe RGB transfer, straight/premultiplied. The worker validates,
unassociates in the declared source domain, decodes transfer, converts to linear
P3 and associates alpha. It builds alpha-aware area mips before [binary16 ↗](https://en.wikipedia.org/wiki/Half-precision_floating-point_format) packing.
Storage is **bounded premultiplied linear Display P3/D65 RGBA16F**, little-endian.
Invalid values and associated RGB above alpha are errors. Zero-alpha RGB becomes
zero. Original source metadata is available via `MediaImage::source_color()`.

The canonical texture retains more precision than RGBA8 and supports linear
filtering/mips. An 8-bit preview [framebuffer ↗](https://en.wikipedia.org/wiki/Framebuffer) still limits visible precision.
The GPU converts into the adjustment's named working primaries, retains extended
intermediates, then clips once to the chosen output gamut. No HDR highlights,
[RAW ↗](https://en.wikipedia.org/wiki/Raw_image_format) development or ICC profile processing are implied. GPU placements currently
use Clip for their output conversion. Supplied-pixel ingestion also offers
`ReduceChroma` before storage through the explicit mapping API below.

### Rec.2020 SDR storage

To retain Adobe RGB source colors, use an explicit wider storage path:

```rust
let job = loader.load_rec2020_rgba16f(
    width, height, supplied_rgba, ImageColor::ADOBE_RGB_1998,
    GamutMapping::Clip,
)?;
```

The synchronous equivalent is `pipeline::rec2020_rgba16f_data`. The output tag is
`ImageTextureFormat::LinearRec2020Rgba16Float`: bounded premultiplied linear
Rec.2020/D65, eight bytes per texel, with associated linear area mipmaps. Mapping
is into Rec.2020 before association. `ImageColor::LINEAR_REC2020` describes linear
supplied input, independently of any encoded transfer curve. Existing P3 loaders
keep their behavior.

Metal, D3D11 and wgpu image shaders interpret the new storage tag for clipped
sRGB previews. Native Metal and macOS wgpu execution are tested; D3D11 runtime
remains unverified. GPU preview adjustments still use sRGB/P3 working matrices.
`wide()` accepts P3 and Rec.2020 storage in an opted-in native wide-SDR window.
Metal converts Rec.2020 to extended linear P3 and preserves extended channels
through normal blending, captures and blur. Windows converts to scRGB, with its
existing capture/path-mask restrictions and unverified native runtime. Browser /
wgpu hosts reject native wide output. Tiled FP16 canvas pages remain P3.

For the full P3 gamut, keep P3 storage: P3 red has a small negative blue component
in Rec.2020, which bounded Rec.2020 ingestion maps. Adobe RGB fits Rec.2020.
Wide image adjustments use sRGB/P3 working matrices and require each affine row's
absolute sum to be at most 4, retaining finite FP16 values through the four
supported nested color filters. Artistic blend modes retain their bounded channel
definitions; normal blending and linear filters preserve extended channels.
This is SDR rendering support, not HDR, ICC inference or a promise of display coverage.

Independent 120-digit primary/white matrix solves and Adobe transfer calculations
cover 1,000 source/alpha combinations. Stored channels allow at most one FP16
step for intermediate rounding, with exact zeros. Odd mips and retention of
out-of-P3 Adobe green are checked. Regenerate with
`python3 scripts/generate_adobe_rgb_references.py --check`.

### Explicit Adobe RGB input mapping

```rust
use xgpui_media::pipeline::{GamutMapping, ImageColor};

let job = loader.load_rgba16f_with_mapping(
    width, height, adobe_pixels,
    ImageColor::ADOBE_RGB_1998,
    GamutMapping::ReduceChroma,
)?;
```

The synchronous equivalent is `pipeline::rgba16f_data_with_mapping`.
Both convert into bounded P3 before associating alpha and generating mipmaps.
Existing `load_rgba16f` / `rgba16f_data` retain their clipping default.
These functions accept explicitly described Adobe RGB pixels; they do not
preserve Adobe RGB colors outside P3 or infer embedded ICC profiles.
`MediaImage::source_color()` retains the original interpretation, while
`format()` identifies the canonical P3 storage. Mapping is selected per ingestion
job and each result retains its own source identity.

`ColorMatrix` can operate in Adobe RGB on the CPU. `ImageAdjustments::new`
explicitly rejects Adobe RGB working matrices because the GPU currently supports
only sRGB/P3; convert the input to canonical P3 and choose either supported
working space for placement. See the [wider-gamut design](color-math.md#stage-b-design-preserve-both-source-gamuts).

`load_rgba16f` uses the same worker admission, cancellation, payload reservation,
background staging pool, per-frame transfer limits and immutable source cache as
ordinary images. The full chain still has a **64 MiB** payload cap; RGBA16F costs
8 bytes/pixel before mip overhead and staging padding. Caller-supplied f32 arrays
cost 16 bytes/pixel and must fit `max_encoded_bytes` (also the supplied-input cap).
Conversion scratch and driver overhead are additional; these are not RSS/VRAM caps.
For small fixtures or application-owned workers, `pipeline::rgba16f_data` returns
validated core upload data synchronously without loader admission.

| Backend | RGBA16F sampling | Output policy / validation |
| --- | --- | --- |
| macOS Metal | Native RGBA16Float, [trilinear mips ↗](https://en.wikipedia.org/wiki/Trilinear_filtering) | Explicit sRGB preview, or accepted wide-SDR linear-P3 compositor; release pixels and UI tested |
| Windows D3D11 | R16G16B16A16_FLOAT | Preview, or existing wide-SDR/scRGB compositor; cross-compilation only, native QA pending |
| wgpu / Linux | Rgba16Float | Explicit sRGB preview; wide placement returns an error; macOS wgpu pixel tests, Linux native QA pending |
| Wasm WebGPU / WebGL2 | Rgba16Float through wgpu | Explicit sRGB preview; both browser demos manually tested; no browser wide presentation claim |

Unsupported texture/device limits fail rather than quantizing to RGBA8. A native
wide placement requires window acceptance; the lab labels preview fallback when
wide mode is unavailable. Use linear `ImageAdjustments` for float sources; a
nonidentity encoded `ImageFilter` returns an error.

### Try RGBA16F

```sh
just media --float-images
```

The release native lab generates a 1280 × 800 source with saturated P3 colors,
near-black gradients and transparent edges. The left panel is an sRGB preview;
the right uses accepted native wide output. Cycle exposure/saturation, vignette,
opacity and output mode, then reset. Both panels must retain one preparation.
A screenshot is an sRGB capture, not a wide-gamut export.

- [RGBA16F browser demo](../examples/extensions/?demo=media-float)
- [Force WebGPU](../examples/extensions/?demo=media-float&backend=webgpu)
- [Force WebGL2](../examples/extensions/?demo=media-float&backend=webgl)

Both browser panels are explicitly clipped sRGB previews of the retained P3
RGBA16F source. Controls, animation and zoom reuse one upload. Keep 60 Hz as the
performance baseline; this phase's checks establish correctness, not a new
sustained frame-time benchmark.

## Progressive source protocol

The generic protocol permits previews/refinements or a final image only. This
state type does not start workers; TileCanvas below provides admitted native
provider jobs. An adapter can wrap an existing decoder, system facility or
application-produced raster source without adding a new codec engine.

```rust
use std::sync::Arc;
use xgpui_media::pipeline::{
    ProgressiveImage, SourceCapabilities, SourceEvent, SourceId, SourceUpdate,
};

// T is an immutable application snapshot; u8 is just a small protocol example.
let mut state = ProgressiveImage::<u8>::new(
    SourceId { asset: 42, generation: 1 },
    SourceCapabilities::default(), // final-image-only provider
);
let request = state.request();
assert!(state.publish(SourceUpdate {
    request,
    sequence: 0,
    event: SourceEvent::Complete(Arc::new(7)),
}));
```

- `Loading` can advance directly to `Ready`, or through `Preview`/`Refining`.
- Provider capability flags govern optional events. Refinement can arrive without
  a preview; a preview cannot replace a refinement or an earlier preview.
- Sequence numbers strictly increase within a request. Stale or invalid updates
  return false and change neither the snapshot nor the last accepted sequence.
- `restart` creates a new request revision, even for the same source or a viewport
  change. Tokens also carry a private state identity so unrelated views cannot
  accidentally publish into one another. Old requests cannot win an A→B→A race.
- `Ready`, `Failed` and `Cancelled` are terminal until restart. Failure retains
  the last usable snapshot. Cancellation suppresses publication and releases the
  state's ownership; it does not forcibly interrupt a decoder.
- Snapshots are immutable `Arc<T>` values. A worker must not rewrite data/resources
  retained by earlier scenes. State replacement does not revoke other Arc owners.
- Provider completion is separate from upload completion and presentation. The
  host must serialize updates and arrange notifications/redraws.

`SourceId` is application-owned: keep the asset ID stable and advance generation
whenever content, dimensions or color metadata changes. A future tile update
publishes a new snapshot/version; it must not mutate an in-flight GPU page.

## Tiled identities and budgets

`ImageDescriptor` and `TilePyramid` plan pages without allocating image pixels.
Mip dimensions floor-halve, matching the existing RGBA8 mip path. Tiles include
clipped overlap (halo) for filters that sample neighboring pixels. Tile edges
plus both halos must fit within the contract's 16384-pixel maximum extent;
backend-specific limits still need capability checks when uploads are implemented.

```rust
use xgpui_media::pipeline::{
    ImageDescriptor, PixelFormat, TileBudgets, TileLayout, TilePyramid,
};

let image = ImageDescriptor::new(8000, 6000, PixelFormat::Rgba16Float,
    ImageColor::LINEAR_P3)?;
let pyramid = TilePyramid::new(image, TileLayout::new(512, 2)?);
let tile = pyramid.tile(SourceId { asset: 42, generation: 1 }, 0, 4, 3)?;
TileBudgets::default().check_tile(&tile)?;
```

A raw page key includes source/generation, dimensions, mip and tile coordinates,
layout/halo, storage format and color/alpha interpretation. Draw-time matrix and
vignette parameters are excluded so controls can reuse uploads. A future cache
of processed intermediates must also key on all effect parameters.

Budgets independently describe CPU decoded/scratch reservations, GPU residency,
staging storage, per-frame transfer bytes/count and decode concurrency. The
feasibility check includes 256-byte row-aligned staging costs. It does not reserve
memory or implement scheduling, eviction, viewport selection or worker admission.

TileCanvas below enforces these policies: visible-page priority, pinned resources
until safe retirement, stale-request cancellation and capped native staging pools. Provider/codec internals and driver overhead need separate
limits. The policy is not a total RSS/VRAM cap, and does not lift the existing
64 MiB whole-image upload limit. Sparse hardware textures are optional.

## Progressive tiled canvases

Use `TileCanvas` for a scanned document, map, large screenshot or scientific
raster whose whole image would exceed the retained-image payload limit.
`GeneratedCanvas` provides a deterministic 12,000 × 6,000 (72 MP) example without
an asset file, decoder or full-resolution allocation.

```rust,ignore
use std::sync::Arc;
use xgpui_media::pipeline::{
    CanvasOptions, CanvasView, GeneratedCanvas, TileCanvas,
};

let mut canvas = TileCanvas::new(
    Arc::new(GeneratedCanvas::new(0)),
    CanvasOptions::default(),
)?;

// Once per frame, from the host view:
canvas.advance(CanvasView {
    center: [6000.0, 3000.0],   // original source pixels
    scale: 0.1,               // logical pixels per source pixel
    viewport: [1000.0, 500.0],
    device_scale: f64::from(window.scale_factor()),
})?;
if let Some(snapshot) = canvas.snapshot() {
    snapshot.paint(bounds, window);
}
```

Retain the canvas in your view. The host arranges redraws while work remains;
the shared native/browser demo shows polling and interaction. `snapshot()`
returns an immutable page table whose pages remain pinned while scenes/GPU work
retain them. `status()` describes provider publication, not GPU completion.
`stats().ready_pages` counts submitted, reusable tile sources separately.

### Providers and progressive behavior

Implement `TileProvider` to describe a source and return an individual requested
`TileDescriptor`. The request includes source generation, mip, content rectangle
and an upload rectangle with a clipped halo. Supply a **single-level**
`ImageTextureData::page` of exactly that upload extent. The scheduler validates
format, dimensions and payload before publication. RGBA8 pages are straight
encoded sRGB; RGBA16F pages are bounded premultiplied linear P3/D65. Float pages
use the explicit sRGB preview path; tiled native wide output and tiled adjustment
coordinates are not enabled by this API.

Providers supply prefiltered [pyramid ↗](https://en.wikipedia.org/wiki/Pyramid_%28image_processing%29) pixels, including halos. They must check
the cancellation flag regularly and account for their own encoded input,
decoder caches and scratch. A supplied thumbnail can serve the coarse-mip
request. This canvas requires a coarse preview and requested pyramid pages;
final-image-only sources can continue using `MediaLoader` and the generic
`ProgressiveImage` protocol instead.

The preview is requested first and remains visible under missing tiles. Tile
selection accounts for viewport, zoom and device scale; visible pages nearest the
viewport center are requested first. If all selected pages cannot fit the policy,
selection chooses a coarser mip. This preserves a bounded usable view instead of
pretending the whole source fits on the GPU.

A refinement **replaces** its preview region, including for translucent pixels.
Unready sources warm up with zero sampling opacity, so their first upload cannot
double-blend alpha over the preview. Tile content is clipped separately from its
halo. A provider failure keeps the last usable snapshot and stops retrying that
failed view; a changed view or source can retry.

```rust,ignore
// Source IDs are application-owned; advance generation when pixels change.
canvas.replace(Arc::new(GeneratedCanvas::new(1)))?;
canvas.cancel(); // stops publication and releases this canvas's page ownership
```

Replacement may change dimensions or canonical format. Old requests cannot
publish into the new source, and old immutable resources remain charged until
their workers, scenes and GPU uses release them. No in-flight texture is rewritten.

### Budgets and backend behavior

`CanvasOptions` defaults to 256-pixel pages with a one-pixel halo, 16 MiB each for
CPU/GPU payload reservations, 4 MiB staging, two provider jobs, at most two uploads
and 1 MiB aligned transfer bytes per frame, and 48 cached pages. An additional
512 KiB provider scratch reservation is charged per active job. Tune these
values for your provider; a larger filter needs a larger halo.

- CPU reservations include retained page bytes and declared active-job scratch.
  GPU reservations conservatively include pending, cached, visible and retired
  page payloads. Idle nonvisible pages are evicted by least-recent use.
- Eviction removes cache ownership, not other owners. Held snapshots and GPU
  submissions keep their leases; admission waits when those bytes are still busy.
- Native providers run on admitted workers, followed by existing background GPU
  staging. Native upload handoff enforces aligned per-frame byte/count limits.
  Native pooled staging counts idle allocations toward its separate pool cap.
  Existing large-image benchmark harnesses now allow 33 MiB rather than 32 MiB
  per frame so their 32 MiB pixel chains also fit aligned rows. Historical
  benchmark reports retain their original policy and results.
- Wasm currently generates at most one small page per `advance` on the host
  thread. Upload admission and in-flight staging are bounded again at preparation.
  This demo does not measure a browser worker, file I/O or decoder.
- Metal retains source snapshots through command completion. wgpu now retains them
  through queue completion; D3D11 uses event-query retirement. Windows/Linux native
  runtime QA remains pending; cross-compilation is not runtime validation.

`pending_staging_bytes`, `upload_staging_bytes` and `staging_pool_bytes` describe
different stages. They are not interchangeable. These limits concern page payloads,
declared scratch and source staging, not process RSS, driver VRAM, render targets,
provider internals, allocator overhead or tiny transparent helper textures.
Keep one canvas/policy owner for the budget domain you intend to bound. A new
canvas has its own limits; dropping an old one does not instantly revoke GPU use.

### Run and inspect

```sh
just media --tiled
```

- [Progressive tiled browser demo](../examples/extensions/?demo=media-tiles)
- [Force WebGPU](../examples/extensions/?demo=media-tiles&backend=webgpu)
- [Force WebGL2](../examples/extensions/?demo=media-tiles&backend=webgl)

Start with Fit, choose 1:1, pan/drag, wheel zoom and try Cache: 2 MiB. Watch mip,
ready/visible counts, reservations, staging, eviction and cancellation. Replace
the source repeatedly, then Cancel/Restart. The colored grid is generated imagery;
the demo's coarse levels sample the analytic function at mip centers and can alias.
They are not a substitute for a real provider's properly prefiltered pyramid.

The demo shows first-preview CPU availability and per-view submission latency.
Those observations include host scheduling and generation/upload work; there is
no disk or codec latency in this workload, and submission is not physical display
completion. Keep **60 Hz as the performance baseline**. This phase's manual runs
are functional checks, not a new sustained frame-time or total-memory benchmark.

## Reusable preview blur and native video effects

`ImageBlur` reuses `xgpui-shader`'s [Gaussian blur ↗](https://en.wikipedia.org/wiki/Gaussian_blur) compositor. It blurs a completed
**sRGB preview**, after source matrix/vignette adjustments and before final group
opacity. The kernel filters premultiplied RGBA, including alpha. This is encoded
sRGB filtering, not a linear-light photographic [convolution ↗](https://en.wikipedia.org/wiki/Convolution) or wide-SDR effect.
RGBA16F sources must use their sRGB preview placement inside the blur closure.

```rust
use gpui::{px, LayerBlurQuality};
use xgpui_media::pipeline::ImageBlur;

// Keep this in view state; clone it for paint callbacks.
let blur = ImageBlur::new(px(8.))?
    .with_quality(LayerBlurQuality::Adaptive);
// In a paint callback, compose all visible pages before applying the kernel.
blur.paint(bounds, window, |window| snapshot.paint(bounds, window))?;
```

Use the same closure for `MediaImage::paint_adjusted` or `VideoFrame::paint`.
Radius is Gaussian kernel support in logical pixels, from 0 to 16, with at most
64 device pixels of support. Adaptive quality uses smaller intermediate targets
for larger radii; `FullResolution` provides the reference mode.

The shared compositor limits each capture, **including its sampling halo**, to
1,024 device pixels per side and allows four layer groups per scene. Nonzero
`ImageBlur::paint` returns an explicit size error before recording an oversized
capture. Radius zero paints directly. Choose a bounded preview rather than
assuming whole-window blur. The tiled demo switches to a smaller preview when
blur is enabled; pan and zoom still select source pages normally.

Blurring the assembled preview gives neighboring pages one continuous kernel;
there is no per-page blur boundary and no need to grow the source tile halo for
this output-space effect. Existing tile halos still provide [bilinear sampling ↗](https://en.wikipedia.org/wiki/Bilinear_interpolation).
Undrawn pixels beyond the image/canvas viewport are transparent, which can soften
outer edges; output clips to the requested bounds. No extra off-viewport source
pages are requested. This is different from source-space blur of a complete asset.

Allocated capture/filter targets are reused while their dimensions and quality
permit. Custom media draws refresh raw captures on each paint, including paused
frames: this does not promise retained filtered pixels or zero GPU work. Source
image uploads/native texture imports remain reusable. Capture, intermediate and
filtered textures are **additional to `TileBudgets`**; page statistics do not
represent total effect memory. Resizing or switching quality can allocate again.

Existing immutable native video frames accept the same linear adjustments:

```rust
use xgpui_media::{video_frame, VideoFrame};
// `frame` is an application-supplied immutable native snapshot.
let adjusted = frame.adjusted(adjustments);
let view = video_frame(frame)
    .adjustments(adjustments)
    .blur(blur.clone());
// Or use video(stream).adjustments(adjustments).blur(blur.clone()).
```

`VideoFrame::adjusted` preserves size, timestamp and underlying allocation
ownership; it changes placement parameters, not source pixels. Native [BGRA ↗](https://en.wikipedia.org/wiki/RGBA_color_model) and
[NV12 ↗](https://en.wikipedia.org/wiki/YCbCr) use the existing range/matrix conversion to encoded sRGB first, followed by
linear working-color adjustments. Application-owned adapters must supply an
explicit matching SDR contract. `VideoView::blur` falls back to the adjusted,
unblurred frame when capture bounds exceed the compositor limit; use
`ImageBlur::paint` directly when an explicit error is needed. File playback,
decoders, seeking, clocks and mutable decoder-pool synchronization remain in #40.

Practical uses:

- Soften a document/map preview while retaining sharp controls and overlays.
- Preview image adjustments or a blurred thumbnail without re-uploading its pixels.
- Apply exposure, grayscale and vignette to camera or screen-sharing surfaces.
- Build a small frosted video preview with the same effects used for still images.

Run `just media --tiled` for blur radius/quality, pan, replacement and cache pressure.
Run `just media --video` for synthetic macOS Metal BGRA/NV12 frames, effects,
pause/resume, opacity and corners; no sample video is required. Browser coverage
uses the portable tiled preview, not imported native video surfaces:
[WebGPU](examples/extensions/?demo=media-tiles&backend=webgpu) ·
[WebGL2](examples/extensions/?demo=media-tiles&backend=webgl).

## Capability reporting and validation

`PipelineCapabilities::current()` reports linear adjustments and vignette as
available, along with RGBA16F, progressive/tiled images and bounded sRGB preview blur. These
flags describe implementation availability; they are not a driver probe, proof
of display gamut or validation of the existing #23 surface adapters. CPU reference
methods above remain available independently of rendered placements.

Run the CPU contract tests from the repository root:

```sh
cargo test --release --locked --config .cargo/validation.toml --manifest-path extensions/crates/xgpui-media/Cargo.toml --target-dir target --lib pipeline::
```

On macOS, `--features gpui-apple/runtime_shaders` avoids the build-time Metal
compiler when needed. Tests cover known transfer/exposure values, working-space
luminance, alpha, matrix order, unclipped intermediates and explicit mapping,
aspect-correct vignette falloff, stale/terminal source states, tile edges/halos,
overflow, cache identities and independent pool/frame feasibility.

### Phase 2 verification (2026-10-26)

- 35 release media tests pass with the `wgpu` feature, including both Metal and
  wgpu pixel comparisons to the CPU reference. Coverage includes five exposure
  settings, source-aspect vignette, translucent alpha/opacity, switching back to
  encoded filters and one-upload reuse.
- The release native Metal app was manually exercised: exposure, contrast,
  grayscale, vignette, animation/reset, translucent samples and opacity/corners.
  Generated contact sheets were also captured on Metal and wgpu.
- Release Wasm was rebuilt and the demo manually exercised with **forced WebGPU**
  and **forced WebGL2**, including controls, animated exposure/zoom, opacity,
  corners, reset and mode switching. Preparations remained at one.
- Strict media/lab Clippy, formatting and documentation checks pass. Windows
  cross-compilation passes; Windows/Linux native GPU execution remains pending.

These are functional and pixel checks, not GPU timestamps or a new sustained
60 Hz performance benchmark. Keep the documented 60 Hz baseline for performance
acceptance; no universal 120 FPS claim is made.

### Phase 3 verification (2026-10-26)

40 release media tests pass with wgpu enabled, including Metal/wgpu float-source
pixels against CPU references in both working spaces, saturated colors,
transparency/opacity, vignette, upload reuse, half precision, odd-edge mips,
invalid input, payload limits and unsupported wide-output rejection. Metal wide
tests check linear-light blending before the explicit sRGB readback mapping.
Release native controls and both browser backends were manually exercised.

### Phase 4 verification (2026-10-26)

47 release media tests pass with wgpu enabled. Scheduler tests exercise mip/device
scale selection, coarsening under pressure, held-snapshot accounting, eviction,
cancellation, repeated generation/geometry replacement, provider failure/panic and
page validation. Metal/wgpu pixels cover clipped odd pages, halos, transparency
and very large virtual preview bounds at 64× zoom.

The release native Metal and forced WebGPU/WebGL2 demos were manually exercised:
Fit/1:1, pan/drag/wheel zoom, replacement, cancellation/restart and low-memory
mode. The 2 MiB policy coarsens the selected mip while the preview stays usable.
Strict lint/build/documentation checks and Windows cross-compilation pass.
Windows/Linux native GPU testing remains under #23.

**Status:** Phases 1–5 are implemented on the working branch, pending review. System playback remains #40. #23's Windows/Linux runtime testing remains outstanding.

### Phase 5 verification (2026-10-26)

50 release media tests pass with wgpu enabled. New native surface tests compare
BGRA and full/video-range BT.601/709 NV12 adjustments to CPU references in both
working spaces, including vignette, opacity, timestamp/size preservation and a
single texture import across edits. Capture bounds/support tests cover display
scale and the halo size check before recording. Eight existing shader blur tests pass:
Gaussian references at fractional DPI, premultiplied alpha, halo/clipping,
quality changes, nested invalidation and target reuse.

Visible release Metal canvas and video labs, plus forced WebGPU and WebGL2 tiled
blur demos, were tested directly. Radius/quality, pan, replacement/cache pressure,
video pause/resume, format switching, opacity, corners and reset were exercised.
Testing caught the existing 1,024-device-pixel capture limit; bounded previews and
explicit API checks prevent an oversized blur from blanking the renderer.
These are functional/reference checks, not a new sustained 60 Hz benchmark.
Windows/Linux native runtime validation remains outstanding under #23.
