# xgpui-svg — experimental

Full-color progressive SVG elements and opt-in shared job/image-memory scheduling
for GPUI. The API is experimental and unpublished; budgets account for image
costs, not total process memory.

The maintained [usage and contract guide](../../../docs/extensions/svg-rendering.md)
is also published on the documentation website. It covers construction, defaults,
shared pools, lifecycle, diagnostics, low-level APIs and validation commands.

[Native lab and attributed samples](../../../apps/xgpui-ext-lab/svg-lab/README.md) ·
[Performance evidence](../../../apps/xgpui-ext-lab/svg-lab/bench/README.md) ·
[Deferred follow-ups](FOLLOW_UPS.md)

## GPUI public API changes: additions and compatibility

[GPUI public API changes: additions and compatibility](../../../docs/extensions/gpui-public-api.md#added-svg-viewport-size).
See the [crate guide](../../../docs/extensions/svg-rendering.md#gpui-public-api-changes-additions-and-compatibility) for required additions and downstream compatibility impact.

## Programmatic path effects

`path_effects::{Contour, MeasuredPath, PathEffect}` retains line/quadratic/cubic
contours, caches approximate arc lengths, and applies normalized trim and phased
dash arrays before GPUI stroke tessellation. Existing SVG raster callers are
unchanged; imported SVG-to-native conversion is not provided. See the SVG web
guide for validated limits, composition semantics and examples. The native
[media lab](../../../apps/xgpui-ext-lab/media-lab/README.md) has a `--paths` comparison view.
