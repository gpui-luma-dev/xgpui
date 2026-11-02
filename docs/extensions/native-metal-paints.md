# Native Metal portable content

On macOS, SolidColor, Gradient, MeshGradient, ShaderPaint and WideColor describe
portable Content graphs. GPUI owns Metal execution; ordinary producers and apps
do not create command buffers or select a backend. Use ContentDraw and stable
presentation slots for changing placements. See [portable paints](portable-paints.md)
for usage and the [API/lifecycle specification](portable-content-api-design.md)
for graph validation, resource ownership and implementation checkpoints.

## Execution and presentation

A bounded worker queue encodes, submits and retires native work. Typed graph
resources declare render/compute/copy/upload dependencies. Completed native images
are adopted by window presentation without producer CPU pixel readback. Pending
replacements retain compatible completed output; atomic groups publish complete
image/geometry generations together. Removal, cancellation, errors after a
submitted prefix and presentation sampling retain resources until native work
retires. This does not claim automatic reconstruction after device loss.

Image metadata, alpha association, crop/mip sampling, encoded versus linear
adjustments and explicit output policy remain portable. Native wide SDR requires
an accepted wide window; wide source metadata alone is not permission to silently
clip output. CoreVideo BGRA aliases native storage; NV12 imports retain both planes
and convert on the GPU. See [media images](media-images.md) for format, readiness
and ownership restrictions.

## Validate on a Mac

From the repository root:

```sh
just test-all --only 'gpui-apple,issue110-regression,media*'
just media-import-test
```

These are CLI/GPU checks, not applications waiting for button presses. Look for
zero failing checks. The supervised issue-110 regression must finish its control,
repeated-color and distinct-output cases within their process deadlines, verify
exact FP16 pixels, report cached replay and zero producer readbacks, and keep
in-flight producer work bounded. Native import fixtures verify pixels and leases
through conversion, cancellation, submitted-prefix failure and sampling teardown.

For GUI acceptance, use `just test-ui --only 'portable-content*,paint-gradients,media*'`.
Exercise rapid parameter changes, animation, both 512 → 1 and 1 → 512 transitions,
resize, clipping/opacity and video pause/replacement. Look for responsive UI,
complete retained groups and no stale geometry, flashes or indefinite pending
content. Follow the printed checklist and record the result after closing each app.

The maintained macOS acceptance host is Apple Silicon. Those runtime results do
not establish Windows/Linux or browser acceptance. The post-refactor dense versus
instanced performance investigation remains [issue #113](https://github.com/gpui-luma-dev/xgpui/issues/113).
