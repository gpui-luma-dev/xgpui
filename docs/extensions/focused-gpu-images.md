# Focused GPU images

XGPUI combines host-owned Metal recording with retained raster viewports for
photo presentation. Applications keep GPUI entities, layout/input, cached views,
direct main-pass shaders, color math, media sources and tile scheduling.

## Host Metal recording

Texture producers call `MetalCustomContext::record` and finish every encoder
inside the closure. The host shares one unsubmitted command buffer across the
preparation batch, validates outputs and submits the deduplicated commands.
Producers receive no native queue. Cache entries retain an opaque queue identity,
not a recording context. A context binds encoding to its creating thread; admitted
background image uploads use their existing separate staging path.

This preserves validation before submission and fixes dense producer exhaustion
without a general task graph. One batch may contain multiple compute/render
encoders, including the existing mesh and application field algorithms. Native
resources remain retained through their Metal uses. The independent private-target
readback fixture fix was extracted from PR116.

It does not preempt a slow shader/compiler or eliminate the window renderer's
existing presentation synchronization. Direct UI shaders remain the preferred
path for simple fills, gradients and glows. First-use pipeline compilation in
existing producers has not been moved to a new compiler service.

## Retained photo pages

`RasterSource::new(decoded_rgba8, resolved_color, max_source_bytes)` takes one
bounded decoded source. It supports straight-alpha named RGB and existing custom
RGB matrix/TRC definitions. Tile workers produce only requested mip rectangles,
including halos, by averaging associated linear light and storing bounded linear
P3 RGBA16F pages. Transparent pixel RGB does not bleed into their neighbors.
There is no complete GPU float-image/mipmap allocation. The caller owns file
format/profile resolution and CPU source admission; this is not a RAW decoder.

`TileCanvas::advance(view)` retains the existing scheduling, cancellation, stale
result rejection and page leases. `TileCanvas::paint(bounds, window, CanvasPaint)`
keeps the displayed overview while a replacement source prepares. Pending pages
are adopted with zero sampling opacity. Ready pages refine that overview. Source
replacement retains only the prior overview, releasing fine-page ownership to
leave capacity for its successor within the same scheduler/allowance.

`CanvasPaint` selects native wide SDR or sRGB preview and optional existing GPU
image adjustments. Parameter edits reuse source/page identity and uploads. The
window's ordinary controls keep their existing sRGB interpretation. Wide output
requires float pages and accepted native wide SDR; unsupported requests fail.

CPU source bytes, tile/page leases, staging and the existing idle staging pool
are separate allocations. The tile allowance is not a total VRAM limit. Raster
storage is a bounded display representation; general extended-linear processing
intermediates, RAW development and arbitrary processing graphs are not introduced.

## Verification

Keep the original supervised Metal producer regression and six-image benchmark
workload definitions. Native acceptance includes dense replay/replacement,
shared recording limits, wrong-thread rejection, cache reuse, linear/alpha mip
references, float-page sampling, and the consumer's explicit photo correctness
probe. GPU readback is confined to tests/diagnostics. CPU render-call timings and
these captures do not establish displayed FPS or other native OS acceptance.

Executed on the development Mac for this replacement:

- Native Metal library: 52 passed, with Metal API Validation enabled.
- Shader extension: 62 passed, including direct shaders and wide captures.
- Media extension: 64 passed, including deterministic retiring-worker retry.
- Consumer: 145 passed, 7 native-only tests ignored; 9 dependency tests passed.
- Original supervised 512-producer regression: passed unchanged, including replay
  and dense/sparse/dense replacement, with Metal API Validation enabled.
- Native photo probe: passed for the bundled photo and a 6000×4000 RGB fixture.
  Exposure reused source/pages; physical 1:1, independent controls, retained
  source replacement and suspend/resume passed. The 24 MP probe retained
  96,000,000 CPU source bytes and 28,942,648 page GPU bytes at its reported stage.

Other OS GPU runtime acceptance and displayed frame pacing were not measured.
