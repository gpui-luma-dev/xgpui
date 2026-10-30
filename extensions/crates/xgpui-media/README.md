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

`video(VideoStream)` displays the latest bounded frame; `video_frame` paints an
immutable snapshot. The stream provides neither a decoder nor a playback clock.

| Native adapter | Frame contract |
| --- | --- |
| macOS `CoreVideoFrame` | BGRA IOSurface or opaque NV12, with explicit matrix/range handling. |
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
`MediaLoader::load_rec2020_rgba16f` retain Adobe RGB/P3 colors in explicitly tagged
bounded premultiplied linear Rec.2020 SDR with associated mipmaps. Metal/D3D11/wgpu
provide clipped sRGB previews. `wide()` rejects this format until composition and
captures support it; tiled FP16 pages still require P3. ICC inference and full-gamut
native presentation remain separate work. Transfer functions are independent of primaries. CPU matrices accept Adobe
RGB; GPU `ImageAdjustments` explicitly require sRGB or P3 working primaries.
