# xgpui-svg — experimental

Full-color static SVG rendering for GPUI. `SvgImage` owns progressive raster jobs,
timers, atlas cleanup and redraws. `SvgScheduler` optionally shares admission and
estimated image-memory limits across views/windows. This is an opt-in, unpublished
extension; its API may change. GPUI core only adds `ParsedSvg::size()`.

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

## Limits, validation and follow-ups

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

[SVG lab and attributed samples](../../../../apps/svg-lab/README.md) ·
[measurement index](../../../../apps/svg-lab/bench/README.md) ·
[shared-pool results](../../../../apps/svg-lab/bench/results/2026-10-24-scheduler/ASSESSMENT.md) ·
[documented follow-ups](FOLLOW_UPS.md)

From the repository root:

```sh
cargo test --offline --locked --config .cargo/validation.toml --manifest-path extensions/xgpui-ext/crates/xgpui-svg/Cargo.toml --target-dir target
cargo clippy --offline --locked --config .cargo/validation.toml --manifest-path extensions/xgpui-ext/crates/xgpui-svg/Cargo.toml --target-dir target --all-targets -- -D warnings
```

Tests cover actual mounted elements, refinement/timers, sibling identity, pan and
density changes, stale document/policy results, invalid/zero geometry, atlas
removal, FIFO/cancellation, accounting, rebalancing and complete pool drainage.
