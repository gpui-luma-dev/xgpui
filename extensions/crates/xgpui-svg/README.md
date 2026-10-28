# xgpui-svg

Experimental SVG extension for **xgpui**. Display full-color SVG documents with
progressive refinement, optional shared raster scheduling and programmatic path
effects.

## Dependencies

```toml
[dependencies]
gpui = { package = "xgpui", version = "1.0.0" }
xgpui-svg = "0.2.0"
```

## Display a document

Parse once, retain the document in your view, and build the element during render:

```rust,ignore
use gpui::{prelude::*, *};
use xgpui_svg::{svg_image, SvgDocument, SvgScheduler};

// During loading; retain these handles in the parent view.
let document = SvgDocument::parse(&cx.svg_renderer(), &svg_bytes)?;
let scheduler = SvgScheduler::default();

// During render, using the retained handles:
svg_image("artwork", self.document.clone())
    .scheduler(self.scheduler.clone())
    .w(px(640.)).h(px(480.))
    .zoom(2.)
    .pan(point(px(20.), px(-10.)))
```

Use a stable ID unique among siblings and reuse the same document across frames.
The element manages refinement jobs, redraws and retained image cleanup. Repeated
parsing during render restarts that work. File loading, mouse gestures and font
configuration remain application responsibilities.

## Progressive rendering and scheduling

A coarse preview appears before settled detail. Zoom and pan alter the artwork
without changing layout or hitboxes. Share a `SvgScheduler` to coordinate job
admission and estimated image-memory limits across views or windows.

Budgets account for image costs, not filter intermediates or total process
memory. Already-running synchronous raster work cannot be interrupted; stale
results are prevented from replacing newer settings.

## Path effects

`path_effects::{Contour, MeasuredPath, PathEffect}` provides trim and phased dash
operations on retained line, quadratic and cubic contours before stroke
tessellation. Arc lengths are approximate. This API does not convert imported
SVG documents into native path geometry.

See the [SVG guide](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/extensions/svg-rendering.md)
for construction, viewport defaults, scheduling, path effects and supported limits.

## Examples and status

Run `just svg` from the repository root. The
[SVG lab](https://github.com/gpui-luma-dev/xgpui/blob/main/apps/xgpui-ext-lab/svg-lab/README.md)
contains examples and artwork attribution. The
[media lab](https://github.com/gpui-luma-dev/xgpui/blob/main/apps/xgpui-ext-lab/media-lab/README.md)
also includes a `--paths` comparison view.

This API is experimental. Consult the
[performance evidence](https://github.com/gpui-luma-dev/xgpui/blob/main/apps/xgpui-ext-lab/svg-lab/bench/README.md)
and [follow-ups](https://github.com/gpui-luma-dev/xgpui/blob/main/extensions/crates/xgpui-svg/FOLLOW_UPS.md)
for validated behavior and remaining work.

## GPUI public API changes: additions and compatibility

Use this crate with xgpui's matching framework packages. Its integration depends
on [xgpui's GPUI API changes](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/extensions/gpui-public-api.md#added-svg-viewport-size).
See the [compatibility guide](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/extensions/svg-rendering.md#gpui-public-api-changes-additions-and-compatibility)
for additions, affected public types and migration requirements.

## Project

[Repository](https://github.com/gpui-luma-dev/xgpui) · [Release preparation](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/PUBLISHING.md) ·
[Website](https://xgpui.dev/). Licensed under Apache-2.0.

This extension uses its own `0.2.0` version, separate from xgpui framework
`1.0.0` and the upstream GPUI baseline.
