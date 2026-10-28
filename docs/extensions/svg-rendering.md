# xgpui-svg — experimental

Full-color static SVG rendering for GPUI. `SvgImage` owns progressive raster jobs,
timers, atlas cleanup and redraws. `SvgScheduler` optionally shares admission and
estimated image-memory limits across views/windows. This is an opt-in, unpublished
extension; its API may change. The GPUI integration adds [`ParsedSvg::size()`](gpui-public-api.md#added-svg-viewport-size).

## GPUI public API changes: additions and compatibility

The reusable element requires `ParsedSvg::size()` to read the resolved viewport
without reparsing. That accessor is additive; existing parsing/rasterization
signatures and whole-document sizing behavior remain unchanged. The document,
policy, queue, scheduler and element APIs live in `xgpui-svg`.

The browser's sharp comparison separately adds `SvgSize::Viewport { size, view_box }`.
Downstream exhaustive `SvgSize` matches must handle the new variant even when
not using the comparison. This does not add cropped scheduling to `SvgImage`.
Viewport output limits do not cap filter intermediates or total process memory.

**[GPUI public API changes: additions and compatibility](gpui-public-api.md#added-svg-viewport-size).**
See also [viewport rasterization and enum compatibility](gpui-public-api.md#added-svg-viewport-rasterization).

## Use

Add a path dependency on this crate. Parse once (on a worker for complex artwork),
retain the immutable document in your view, and build the element during render:

```rust,ignore
use gpui::{prelude::*, *};
use xgpui_svg::{svg_image, SvgDocument, SvgScheduler};

// During loading; retain both handles in the parent view:
let document = SvgDocument::parse(&cx.svg_renderer(), &svg_bytes)?;
let scheduler = SvgScheduler::default(); // optional shared pool

// In render(): no queue, timer, subscription or worker setup.
svg_image("artwork", self.document.clone())
    .scheduler(self.scheduler.clone())
    .w(px(640.)).h(px(480.))
    .bg(rgb(0xf8fafc)).rounded_md()
    .zoom(2.)
    .pan(point(px(20.), px(-10.)))
```

- Use a stable ID unique among siblings and clone the same `SvgDocument` across
  frames. Reparsing each frame restarts refinement.
- The viewport fills its parent by default. Normal `Styled`/`InteractiveElement`
  APIs apply. Artwork uses centered contain-fit and its intrinsic aspect ratio.
  `.logical_size(size(w, h))` overrides the artwork's logical size at 100% zoom.
- Zoom and logical-pixel pan change the artwork, not layout or hitboxes. Mouse
  gestures and file loading stay application-controlled. Parsing accepts the
  application's font/resolver configuration.
- A coarse preview appears first; settled detail follows. Pan and zoom-out reuse
  adequate detail. Removing an element releases its retained image and atlas
  entry. Replacement also explicitly evicts superseded atlas entries.
- Already running synchronous resvg work cannot be interrupted. Stale results
  cannot replace newer documents/settings; a removed element cannot be updated.
  Zero bounds suspend requests. Invalid input fails without retry loops and can
  recover when corrected.

| Default | Value |
|---|---:|
| Maximum supplied SVG data | 32 MiB |
| Per-output RGBA budget | 16 MiB |
| Raster edge cap | 8192 px |
| Preview longest edge | 640 px |
| Detail settle delay | 120 ms after a changed output target |
| Optional shared pool | 128 MiB accounted image costs, 2 active jobs |

Override per-image settings with `.policy(RasterPolicy { .. })`.

## Shared scheduling

Create **one** pool per intended group, then clone it into every participating
image. Without `.scheduler(...)`, elements retain independent queues.

```rust,ignore
use xgpui_svg::{SchedulerLimits, SvgScheduler, MIB};
let scheduler = SvgScheduler::new(SchedulerLimits {
    max_jobs: 2,
    max_bytes: 64 * MIB,
})?;
```

FIFO admission uses wakeups, not polling or UI-thread blocking. Obsolete queued
detail is cancelled; useful same-document previews survive zoom bursts. Pending
requests have unique cancellation tickets. Active work keeps its reservation
and job slot until the renderer actually finishes, including after unmount.

Accounting reserves **3 × output RGBA** for in-flight output/conversion/upload
headroom, then **2 × RGBA** for retained CPU data plus nominal GPU residency.
Replacements count old retained and new in-flight images together. Retained
charges remain until deferred atlas eviction. Failed/stale/cancelled work frees
its reservations. `scheduler.stats()` exposes membership, queued/active jobs,
retained/in-flight/current managed bytes and lifetime accounting/concurrency peaks.

Each member's output cap is also limited to
`max_bytes / (2 * members + 3 * max_jobs)`, rounded down to a whole pixel, with a
one-pixel minimum. This leaves replacement headroom. Membership changes rebalance
resolution; retiring oversized images can temporarily leave a tile empty while
its new preview waits. Extremely many members in a tiny pool can exhaust even
minimum-pixel capacity until members leave.

Limits are immutable; replace the pool to change them. Old-pool work retains its
old reservation while draining, so replacement is not an immediate hard-limit
downgrade across both pools. Do not construct a new pool per frame or per tile
when you intend sharing.

## Optional status and low-level APIs

```rust,ignore
svg_image("artwork", self.document.clone())
    .on_status_change(cx.listener(|this, status: &xgpui_svg::SvgStatus, _, cx| {
        this.svg_status = Some(status.clone());
        cx.notify();
    }))
```

Callbacks are deferred until after drawing and emitted on observable changes.
`SvgStatus` reports `Empty`, `Waiting`, `Queued`, `Preview`, `Detail`, `Ready` or
`Failed`; requested/budgeted/displayed sizes; raster wall time; completed count;
and errors. Counts are per mounted ID. Ready means **budgeted** detail is ready.
The element adds no status overlay of its own.

| API | Purpose |
|---|---|
| `SvgDocument` | Shared immutable parsed tree, intrinsic size and aspect fitting |
| `RasterPolicy` / `Resolution` | Validated requested, preview and output sizing |
| `RasterQueue` / `Job` / `Stage` | Deterministic latest-target scheduling for custom hosts |
| `RasterImage` | CPU RGBA output and raster/conversion wall time |
| `SvgScheduler` / `SchedulerLimits` / `SchedulerStats` | Optional shared pool and accounting |

Direct queue users supply monotonic time and generations, request validated
`resolution.output`, and deliver every active `complete(job, success)`. Only
accept a result when `complete` returns true. `set_current` registers retained
image dimensions; `invalidate` drops desired/current metadata while old work
drains. `SvgImage` handles this lifecycle internally.

## Native sample and validation

The offline native gallery uses committed, attributed SVGs:

```sh
just --justfile apps/xgpui-ext-lab/svg-lab/justfile gallery
```

The browser extension gallery also includes `?demo=svg`: five attributed samples,
synchronized bounded and viewport-rerasterized views, wheel zoom and drag pan,
a ten-tile gallery, and switchable 8/32 MiB shared pools. The rerasterized view
uses `SvgSize::Viewport` to render only visible pixels at display density, keeping
curves sharp at deep zoom. It is a demo comparison outside the bounded scheduler,
with separate output-size and timing diagnostics. Camera/viewport changes rebuild
its raster; ordinary redraws reuse it. **Sharp SVG zoom: ON/OFF** switches the
right view between crisp CPU rerasterization and GPU texture zoom of a cached
100% raster. With it off, zoom/pan leaves the right raster count unchanged;
artwork selection or resize can still rebuild the base raster. The notice
explains the speed/detail tradeoff and that the left bounded view still refines.
Independent **Bounded view: ON/OFF** and **Comparison view: ON/OFF** controls
unmount either renderer and release its raster while retaining the shared camera.
Unmount the gallery too when isolating a single view's cost.
Its diagnostics report element raster latency and accounted bytes, not total
browser memory. The current non-atomic Wasm platform runs raster jobs on the
browser thread: progressive rendering and admission limits apply, but a costly
filter can still block input. Native worker responsiveness measurements above
do not establish browser responsiveness. The browser demo caps concurrency at
one and bounded-element output at 2 MiB. The viewport comparison rejects
output edges above 8192 device pixels. Complex filter intermediates can still
be costly; a 1600% leaf render took about 2.5 seconds in one browser check. Use the gallery's artwork credits link for
creator, source, license and unchanged SVG downloads.

## Limits and follow-ups

The hard shared invariants are **accounted reservations ≤ pool budget** and
**admitted jobs ≤ concurrency limit**. Neither output budgets nor accounting are
a hard process/GPU-memory ceiling. Parsed trees, filters, frame references, atlas
packing, allocator/driver retention and app baseline are not fully modeled.
Smaller budgets reduce deep-zoom detail; viewport mounting remains useful.

Static SVG is supported through GPUI/resvg. Scripting/animation, an SVG DOM,
arbitrary SVG-to-GPUI-path conversion and viewport tiling are outside this
iteration. External resources/fonts retain GPUI/resvg behavior; embed assets or
outline text for portable documents. Parsing and fetching policies remain with
the host. This is not a sandbox for hostile documents.

[SVG lab and attributed samples](https://github.com/gpui-luma-dev/xgpui/blob/main/apps/xgpui-ext-lab/svg-lab/README.md?plain=1) ·
[measurement index](https://github.com/gpui-luma-dev/xgpui/blob/main/apps/xgpui-ext-lab/svg-lab/bench/README.md?plain=1) ·
[shared-pool results](https://github.com/gpui-luma-dev/xgpui/blob/main/apps/xgpui-ext-lab/svg-lab/bench/results/2026-10-24-scheduler/ASSESSMENT.md?plain=1) ·
[documented follow-ups](https://github.com/gpui-luma-dev/xgpui/blob/main/extensions/xgpui-ext/crates/xgpui-svg/FOLLOW_UPS.md?plain=1)

From the repository root:

```sh
cargo test --offline --locked --config .cargo/validation.toml --manifest-path extensions/xgpui-ext/crates/xgpui-svg/Cargo.toml --target-dir target
cargo clippy --offline --locked --config .cargo/validation.toml --manifest-path extensions/xgpui-ext/crates/xgpui-svg/Cargo.toml --target-dir target --all-targets -- -D warnings
```

Tests cover actual mounted elements, refinement/timers, sibling identity, pan and
density changes, stale document/policy results, invalid/zero geometry, atlas
removal, FIFO/cancellation, accounting, rebalancing and complete pool drainage.

## Contour trim and dash effects

`xgpui_svg::path_effects` adds programmatic retained vector contours alongside the
static SVG document API. It does not convert imported SVG documents to native
paths or modify the existing raster pipeline.

```rust,ignore
use gpui::{point, px};
use xgpui_svg::path_effects::{Contour, MeasuredPath, PathEffect};
let contour = Contour::new(point(px(20.), px(100.)))
    .cubic_to(point(px(60.), px(0.)), point(px(140.), px(180.)), point(px(200.), px(80.)))
    .quadratic_to(point(px(250.), px(0.)), point(px(300.), px(100.)));
// Retain this table while animating the effect.
let measured = MeasuredPath::new(&[contour], px(0.05))?;
let effect = PathEffect::new()
    .trim(0.0..0.75)?
    .dash(&[px(16.), px(8.)], px(5.))?;
window.paint_path(measured.stroke(&effect, px(4.))?, gpui::rgb(0x9ee3bd));
```

Contours retain line, quadratic and cubic verbs. Arc-length lookup uses cached
adaptive subdivisions, including collinear curves with nonlinear parameter speed.
Trim endpoints use de Casteljau slicing; the resulting segments remain curves
until the existing GPUI stroke tessellator runs. `MeasuredPath::apply` returns
contours when callers need geometry rather than a native stroke.

Trim is an ordered, nonwrapping range within 0..1 over the **total concatenated
length** of all contours. Empty trim is empty geometry. Close edges contribute to
length. Solid full contours retain closure; partial trims have open ends. Trim is
applied before dashing, and dash phase restarts at each retained contour's start.
Positive phase advances into the pattern, negative phase wraps. Odd dash arrays
repeat; an empty array is solid. Visible pieces spanning a full closed contour's
seam are joined to avoid introducing caps there.

Lengths, stroke widths, tolerance and dash phase use logical pixels. The table's
0.001..16 px tolerance bounds each adaptive leaf's control-point deviation from a
uniformly parameterized chord; it is not a global arc-length error guarantee. Use
smaller tolerances for more precise animation and retain the measurement cache.
Native stroke joins/caps use GPUI defaults. Fill effects, SVG arc verbs, automatic
SVG extraction and Lottie playback are not included.

Inputs are checked before tessellation: finite coordinates within ±1,000,000 px,
at most 8,192 contours/segments (including implicit closing edges), 65,536 total
lookup samples, 64 input dash entries and 16,384 emitted curve pieces. Dash lengths
must be finite and strictly positive; zero entries are rejected. Stroke widths are
positive and at most 4,096 px. Excessive subdivision, fragmentation or numerically
unresolvable dash lengths return errors rather than allowing unbounded work.

### Native visual test

```sh
just --justfile apps/xgpui-ext-lab/media-lab/justfile paths
just --justfile apps/xgpui-ext-lab/media-lab/justfile paths-capture
just --justfile apps/xgpui-ext-lab/media-lab/justfile paths-wgpu-capture
cargo test --offline --locked --config .cargo/validation.toml --manifest-path extensions/xgpui-ext/crates/xgpui-svg/Cargo.toml --lib path_effects
```

The lab shows original, trimmed and trimmed-plus-dashed strokes in three columns.
Rows cover open Béziers, a closed heart, and multiple contours. Cycle trim through
100%, 75%, 50%, 25% and 0%; change dash pattern/phase and stroke width; animate trim
and phase. Animation respects reduced motion. Check curve continuity and closed
seams as phase changes. The Images button returns to media tests.

Headless captures have the same three rows/columns, fixed at 68% trim and a 16/8
pattern with phase 5. They check visible pixel coverage and decreasing coverage
from original to trim to dashes. Metal and wgpu capture paths are available;
native Windows/Linux execution remains outstanding.


### Browser contour-effects comparison

Open [Trim / dash](../examples/extensions/?demo=path-effects), or append
`&backend=webgpu` / `&backend=webgl`. Three columns compare original, trimmed and
trimmed/dashed strokes for open curves, a closed heart and multiple contours.
Cycle trim to zero/full, even/odd dash patterns, phase and width; optional animation
respects reduced motion. This uses the shared contour effects and native GPUI
stroke tessellation, not SVG rasterization or CSS dashes. Both forced backends
were checked locally in release on 2026-10-26. The separate progressive SVG demo
and its artwork/attribution remain unchanged.
