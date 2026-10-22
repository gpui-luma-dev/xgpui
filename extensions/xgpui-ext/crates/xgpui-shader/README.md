# xgpui-shader

Reserved for the custom shader and canvas extension described in
[#7](https://github.com/gpui-luma-dev/xgpui/issues/7).

The intended crate owns the public element API, shader parameters, retained GPU
resources, pipeline caches, diagnostics, and backend implementations/adapters as
the agreed dependency graph permits. GPUI supplies a small retained scene and
renderer integration contract.

The reference consumer is an animated mesh gradient that composes with normal
GPUI layout and styling. This placeholder establishes the location only; the
crate API, manifest, backend packaging, and implementation are pending design.
