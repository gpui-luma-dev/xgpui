# First native milestone: rounded control backgrounds

Branch: `issue-37/rounded-overflow`; base target: `issue-37/epic`.
Feasibility baseline preserved as commit `7f22bef` on `issue-37/clip-feasibility` and
carried onto this branch. No merge and no changes to `main`.

## Delivered

A registered [native control lab](../../apps/xgpui-ext-lab/rounded-overflow-lab/README.md)
compares the same gradient/solid background, square child backgrounds, native text,
border and configurable radius under legacy rectangular overflow versus rounded
subtree composition. It paints actual GPUI descendants, not a browser texture demo.

The implementation deliberately uses bounded color isolation for this first
native experiment. The original recommendation to avoid per-container color targets
for a general implementation remains open; this is a correctness baseline and
cost comparison candidate, not evidence that isolation is the final architecture.

CPU masks are separate from `ContentMask`; immutable side metadata is associated
with hitbox IDs and retained across prepaint reuse. Cached views compare active
rounded masks in their cache key, so geometry-only changes invalidate hitboxes.
Excluded corners do not occlude underlying mouse or scroll targets.

Paint uses the existing shared layer compositor with private scene-owned geometry.
`Scene::replay` copies the associated geometry while retaining the capture Arc
that owns its identity. Shape updates do not create shader variants. Composition
applies coverage once to associated RGB/alpha after capturing the real subtree.
No public primitive, style, hitbox or capture record gained a field. The actual
core additions/protocol change are individually listed in the
[API review](api-review.md), pending final user review.

## Verification

On Apple M4 Max:

- Real-window Metal check: legacy corner remains colored; rounded corner reveals
  the underlying background; corner click is rejected; visible header click works.
- Radius-only changes update input and output, including switching to radius zero.
- Configured background changes switch solid colors and restore the gradient.
- Metal and wgpu headless regression tests vary radius repeatedly on one renderer,
  compare corner/interior pixels, and compare output after `Scene::replay`.
- CPU regressions verify cached prepaint preserves unchanged masks and radius-only
  changes invalidate cached child input. They also verify excluded occluding corners allow underlying mouse and
  scroll targets, while interior occlusion and nested input intersections remain.
- Shared shader validates/translates through Naga for MSL/HLSL/SPIR-V.
- All 40 existing subtree-effect tests pass, including opacity, blur halos,
  cached children, destination blending, wide-gamut and retained cleanup.

Commands:

```sh
just rounded-overflow
just rounded-overflow check
cargo test --offline --config .cargo/validation.toml -p xgpui --lib --features test-support rounded_hit_test_tests
cargo test --offline --config .cargo/validation.toml -p xgpui-shader --features portable-shaders,wgpu,gpui-apple/runtime_shaders rounded_overflow -- --test-threads=1
```

The explicit Metal feature in the final command is for macOS's runtime shader
build. Other platforms use their applicable CI configuration. This is not a request
for Windows/Linux user testing before merge.

## Limits and next work

- Two color textures/container; four groups/frame; maximum capture side 1024 device
  pixels including the existing sampling allowance. Costs are not benchmarked yet.
- Grayscale text in captured scopes. Circular per-corner radii only; the lab's
  uniform 2px border produces circular inner corners. Unequal border widths and
  elliptical inner curves still need a geometry extension.
- Nested captured groups involving rounded output are rejected. Joint AA for
  intersecting clips, intrinsic geometry edges and fallback remain open.
- Deferred overlays retain their existing escape semantics. Foreign native surfaces,
  backdrop composition inside captures and full CSS overflow semantics are not added.
- Headless runtime coverage here is on the Mac's Metal backend, including wgpu over
  Metal. Native D3D11, Windows/Linux runtime, actual WebGPU/WebGL2 GPUI capture transport,
  fractional-DPI integrated windows, wide-gamut capture, scrolling and final performance
  budgets remain unverified. The earlier isolated probe covers different geometry/
  transport questions and does not stand in for these checks.

Next: measure this correctness baseline; integrate stable repeated-shape coverage
without per-container color targets; resolve nested AA, effects and border geometry;
expand the lab/probes; complete backend capability checks; then review a final
opt-in styled API. Do not treat this milestone as completion of the epic.
