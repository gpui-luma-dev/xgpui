# xgpui-media

Experimental media extension for **xgpui**. Load images, share mipmapped textures
between views, adjust colors and present bounded streams of immutable video
frames.

## Dependencies

```toml
[dependencies]
gpui = { package = "xgpui", version = "1.0.0" }
xgpui-media = "0.3.0"
```

## Pixels and color metadata

For applications that already produce pixels, supply an explicit `ImageColor`
(primaries, transfer and alpha association) to the supplied-pixel loaders. Select
gamut mapping and storage deliberately; declaring Adobe RGB does not discover or
parse an ICC profile. File decoding and metadata discovery belong in application
adapters. The existing file loader below is an sRGB convenience path.

Use `MediaLoader::load_pixel_buffer` with `PixelColor` for named, custom or extended-linear interpretation. The optional color-crate [ICC adapter](../../../docs/extensions/icc-profiles.md) resolves supplied RGB matrix/TRC bytes into a custom definition. Start with [pixels + metadata](../../../docs/extensions/pixels-and-metadata.md) for the complete path.

## Load an image

Create a loader once, retain the pending job, and poll its result from the view
or a GPUI task:

```rust,ignore
use xgpui_media::MediaLoader;

let loader = MediaLoader::default();
let job = loader.load("photos/cover.webp")?;
// Store job in view state; use try_take() when the image is ready.
```

Retain the resulting `MediaImage` and build an element during render:

```rust,ignore
use gpui::{prelude::*, *};
use xgpui_media::image_view;

image_view(image.clone())
    .w(px(480.)).h(px(320.))
    .rounded(px(12.))
```

Loading must not restart on every render. Arrange notifications or polling,
check failure status, and keep the job alive until completion. Cloned image
handles share the retained upload. `image_view` stretches to the supplied size;
preserve the source aspect ratio when needed.

## Portable image presentation

Ordinary `MediaImage`, `image_view(image)` and `image.paint(...)` now use the
portable content API. The renderer owns GPU uploads, completion and redraws.
`LoadStatus::Ready` means decoded CPU pixels are available, not GPU completion.
Source pixels and all mip levels share one packed allocation; cloning a source
or retaining its graph keeps the loader's memory reservation alive.

Use `image.draw()` for configured placement, `image.content()` for graph inputs,
or `image.paint_mapped(...)` for fit/alignment/crop control. For example:

```rust,ignore
use gpui::content::{ImageFit, ImageMapping};
let draw = image.draw().mapping(ImageMapping::new(ImageFit::Contain));
window.paint_content(geometry, draw);
```

The default draw requests mip filtering and an explicit sRGB preview. FP16 P3 and
Rec.2020 pixels retain their source metadata; previewing does not repack or mutate
the source. `ContentColorOutput::MatchWindow` instead preserves source gamut and
requires a capable wide window. Atomic groups accept configured draws through
`group.push(geometry, image.draw())` and commit their mapping/output intent together.

Filters, linear adjustments and wide images now use the same portable source upload.
`filtered(...).draw()` returns a checked placement; `adjusted(...).draw()` applies
linear working-space adjustments. Parameter changes update draw uniforms without
uploading or hashing the source again. `image_view` also accepts `.mapping(...)`
and a stable per-placement `.slot(...)` for replacement history.

`ContentColorOutput::WideSdr` explicitly requires a capable wide window, including
when adjustments extend an RGBA8 source. `image.wide(...)` selects this intent.
Alpha is preserved; coverage and opacity apply once after color conversion.
There is no public per-image GPU upload status/count/retry API. Strict headless
validation uses `prepare_content_snapshot` and retries typed Pending results.
Tiled pages also use portable immutable sources and atomic presentation groups.
Their provider scheduler reports CPU decoding; GPU readiness is renderer-owned.
The obsolete media staging layer and loader upload-policy API have been removed.
Use `MediaLoader::new(MediaLimits)` for CPU admission; portable renderers own GPU
allocation, upload scheduling, completion and retained presentation.

## Image adjustments

`ImageFilter` applies color adjustments while sampling, preserving alpha and
sharing the source upload:

```rust,ignore
use xgpui_media::ImageFilter;

image_view(image.clone())
    .filter(ImageFilter::new().brightness(1.1).saturation(1.3))
```

The [image guide](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/extensions/media-images.md)
explains loading budgets, supported pixel formats, color contracts and filters.
The file loader supports PNG, JPEG and WebP first frames; URL loading and
animated-image playback are not included.

## Video frame presentation

`video("preview", stream)` displays the latest bounded frame;
`video_frame("preview", frame)` paints an immutable snapshot. Supply distinct,
stable IDs for separate placements so pending GPU work retains their previous
valid frames independently. The stream provides neither a decoder nor a playback clock.

| Native adapter | Frame contract |
| --- | --- |
| macOS `CoreVideoFrame` | Portable BGRA import or source-sized GPU NV12 conversion, with explicit matrix/range handling. |
| Windows `DxgiFrame` | Immutable same-device BGRA surfaces; keyed handles use nonblocking read leases. |
| Linux `DmaBufFrame` | Immutable linear BGRA Vulkan snapshots with producer synchronization complete. |

The Linux adapter requires `wgpu`. Native frame imports have explicit unsafe
ownership contracts; read the guide before integrating them. Windows/Linux
runtime verification, DMA-BUF explicit fences and pool reuse remain pending.
[System file playback](https://github.com/gpui-luma-dev/xgpui/issues/40) is follow-up work.

## Backends and examples

Native rendering uses Metal or D3D11; `wgpu` enables the optional wgpu adapter.
Feature/backend support varies by image format and import path.

Run `just media` from the repository root. The
[media lab](https://github.com/gpui-luma-dev/xgpui/blob/main/apps/xgpui-ext-lab/media-lab/README.md)
and [media pipeline guide](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/extensions/media-pipeline.md)
cover examples, validation evidence and remaining limits.

## GPUI public API changes: additions and compatibility

Use this crate with xgpui's matching framework packages. Its integration depends
on [xgpui's GPUI API changes](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/extensions/gpui-public-api.md#added-source-sized-media-textures).
See the [compatibility guide](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/extensions/media-images.md#gpui-public-api-changes-additions-and-compatibility)
for additions, affected public types and migration requirements.

## Project

[Repository](https://github.com/gpui-luma-dev/xgpui) · [Release preparation](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/PUBLISHING.md) ·
[Website](https://xgpui.dev/). Licensed under Apache-2.0.

This extension uses its own `0.2.0` version, separate from xgpui framework
`1.0.0` and the upstream GPUI baseline.


## Explicit Adobe RGB supplied pixels

`pipeline::ImageColor::ADOBE_RGB_1998` describes encoded Adobe RGB (1998)
input. `pipeline::rgba16f_data_with_mapping` and
`MediaLoader::load_rgba16f_with_mapping` select clipping or chroma reduction
during conversion to bounded premultiplied linear P3 RGBA16F. Existing APIs keep
their clipping default. Mapping precedes associated linear-light mip generation.

This is input conversion support: colors outside P3 are mapped before storage.
For wider storage, `pipeline::rec2020_rgba16f_data` and
`MediaLoader::load_rec2020_rgba16f` retain Adobe RGB colors in explicitly tagged
bounded premultiplied linear Rec.2020 SDR with associated mipmaps. Metal/D3D11/wgpu
provide clipped sRGB previews. Native `wide()` accepts P3 and Rec.2020 storage:
Metal composes/captures in extended linear P3; Windows uses scRGB with existing
capture restrictions. Windows runtime remains unverified. Keep P3 storage for the
entire P3 gamut: bounded Rec.2020 clips a small P3 boundary region. Tiled FP16 pages
still require P3. ICC inference remains separate work. Transfer functions are independent of primaries. CPU matrices accept Adobe
RGB; GPU `ImageAdjustments` explicitly require sRGB or P3 working primaries.

## Resolved supplied-pixel metadata

`pipeline::PixelColor` accepts a bounded `Named(ImageColor)`, bounded `Custom`
RGB definition, or `ExtendedLinear` RGB. Custom definitions come from
`xgpui_color::RgbDefinition` and have already-adapted D65 colorants and independent
channel curves. Extended-linear RGB may be negative or above one; alpha remains
in 0–1. Premultiplied input is unassociated in its declared domain, transparent
RGB is canonicalized to zero, and conversion retains extended values until the
explicit storage mapping.

```rust,ignore
use xgpui_media::pipeline::{AlphaMode, GamutMapping, ImagePrimaries, PixelColor};

let source = PixelColor::ExtendedLinear {
    primaries: ImagePrimaries::DisplayP3,
    alpha: AlphaMode::Straight,
};
let job = loader.load_pixel_buffer(
    width, height, pixels, source,
    ImagePrimaries::Rec2020, GamutMapping::Clip,
)?;
```

The buffer is tightly packed, row-major `Vec<[f32; 4]>`; dimensions and byte
admission are checked before worker creation. No arbitrary layout/stride or
additional source precision is implied. Destination storage is explicitly P3
or Rec.2020 RGBA16F. `pipeline::pixels_rgba16f_data` is the synchronous equivalent.
Mipmaps average mapped associated linear light. `MediaImage::source_color()` now
returns the resolved `PixelColor`, preserving custom metadata rather than
mislabeling it as a named space. These paths add no file decoder or ICC dependency.


Custom `PixelColor` metadata now holds an `Arc<RgbDefinition>`; construct it with
`rgb: definition.into()` and clone it cheaply when reusing the source. Decoding
borrows metadata, so it does not clone a table for every pixel. The input limit
counts pixels plus custom metadata, and residency reservations retain a
conservative metadata/table allowance until the image is released. Shared tables
may be counted more than once for conservative admission.

An application may enable `xgpui-color/icc`, parse profile bytes on its worker,
and supply the resulting definition without adding any codec support here:

```rust,ignore
let definition = xgpui_color::icc::IccRgbProfile::parse(&profile_bytes)?
    .into_rgb_definition();
let source = xgpui_media::pipeline::PixelColor::Custom {
    rgb: definition.into(),
    alpha: xgpui_media::pipeline::AlphaMode::Straight,
};
let job = loader.load_pixel_buffer(width, height, pixels, source, storage, mapping)?;
```


CoreVideo and `VideoFrame::new(size, timestamp, portable_content)` now use portable
content and placement transforms. Generic frames are described at the declared
source size and reject mismatched output extents. `VideoFrame::external` accepts
a portable imported image. CoreVideo no longer implements native `CustomContent`
preparation or owns queue/output caches. NV12 converts into encoded-sRGB half-float
storage to preserve precision before adjustments, without CPU pixel readback.
DXGI BGRA now uses portable external content on the matching D3D11 device.
Shader-readable sources need no render-target binding. Keyed snapshots remain
leased through execution and presentation completion. Linux DMA-BUF BGRA also
uses portable external content, with one cached Vulkan import per device/queue
and retained ownership through graph execution and presentation. VideoFrame has
no native forwarding branch. Run `just media-dma-buf-test` on a compatible Linux
host for generated pixels, synchronization and retirement validation; Linux
runtime acceptance remains pending.


Media producer API cleanup: use `image.content()` / `frame.content()` for graph
sources and `image.draw()` / `frame.draw()` for presentation. Filtered, adjusted
and wide image values also expose `draw()`. These values do not implement native
CustomContent preparation. When building a raw scene, use
`draw.into_scene_content()`; UI code uses `window.paint_content(geometry, draw)`.
`paint_filtered(...)` returns a typed Result; handle it with `?` or an explicit
error path. VideoFrame::content now returns Content rather than an erased scene
object.
