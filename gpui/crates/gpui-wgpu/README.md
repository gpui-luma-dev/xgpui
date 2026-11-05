# xgpui-wgpu

wgpu rendering backend for **xgpui**, including GPU context management,
texture atlases, text and headless rendering support.

## When to use this crate

Platform backends use this renderer; it does not create an application window
on its own. Depend on it directly when implementing renderer integration or
working with its backend-specific APIs.

For application setup, use the matching framework packages:

```toml
[dependencies]
gpui = { package = "xgpui", version = "=3.0.0" }
gpui_platform = { package = "xgpui-platform", version = "=2.0.2" }
```

The registry package is `xgpui-wgpu`; its Rust library name is
`gpui_wgpu`. Keep framework packages on a compatible xgpui release.

## Cosmic font inspection

Version 2.1.0 exposes resolved-face OpenType features and variable-axis metadata
through GPUI's `font_capabilities`, `font_axes` and `opentype_features` queries.
Cosmic obtains table bytes from the actual resolved font and collection face;
core parses the metadata privately. Missing tables remain distinct from errors.

Arbitrary explicit axis application is not enabled: `static_variations` is false
and nonempty settings are rejected. Applications can select `xgpui-parley` for
static axes on Linux. See the [font-controls guide](https://xgpui.dev/#font-controls).

## Features

| Feature | Purpose |
| --- | --- |
| `font-kit` | Optional font discovery. |
| `dma-buf` | Linux Vulkan DMA-BUF import support. |
| `test-support` | Rendering test helpers. |

## Documentation

- [Local development and platform prerequisites](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/LOCAL_DEVELOPMENT.md)
- [xgpui API additions and compatibility](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/extensions/gpui-public-api.md)
- [Release preparation](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/PUBLISHING.md)
- [Repository](https://github.com/gpui-luma-dev/xgpui) · [Website](https://xgpui.dev/)

## Versioning and provenance

This backend uses version `2.1.0` with core `3.0.0`, independently of the upstream
release.
This maintained downstream package derives from Zed's `gpui_wgpu` crate via
[gpui-unofficial](https://github.com/gpui-archipelago/gpui-unofficial).
The accepted source capture and Zed provenance are recorded in
[upstream.json](https://github.com/gpui-luma-dev/xgpui/blob/main/upstream.json).

Licensed under Apache-2.0. xgpui is independent of Zed Industries.
