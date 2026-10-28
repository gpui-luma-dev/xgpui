# Shared SVG scheduling and aggregate image budget

The shared scheduler materially reduced measured process memory while preserving
roughly 60 host redraws/second on this machine. It bounds **accounted image
reservations**, not whole-process physical footprint. Smaller pools reduce
raster detail and can introduce admission latency/transient empty viewports.

## Implementation

Retain one `SvgScheduler` in the parent view and pass clones using
`svg_image(...).scheduler(pool.clone())`. The default pool configuration is two
jobs and 128 MiB; a 64 MiB pool was also tested. Existing callers without a pool
keep independent queues. No new third-party dependencies were required.

- FIFO admission uses future wakeups; it does not poll or block the UI thread.
- A running job reserves 3× output RGBA for output/conversion/upload headroom.
  On successful acceptance this becomes 2× RGBA for retained CPU data plus
  nominal GPU residency. Replacements count the old image and new work together.
- Per-member output caps are limited to `budget / (2 * members + 3 * max_jobs)`
  so retained images leave replacement headroom. Membership changes trigger
  resolution rebalancing. There is no image-history cache.
- Admission tickets are unique per request: cancelling an older pending future
  cannot accidentally remove a newer request belonging to the same element.
  Superseded queued detail, zero bounds, invalid input and scheduler replacement
  cancel pending work without occupying the FIFO head indefinitely.
- Non-interruptible rendering carries its reservation into the actual background
  worker. Removing its element cannot free a still-running job's memory/slot.
  Retained charges survive until deferred sprite-atlas eviction.
- Stats report active/queued jobs, membership, retained/in-flight/current managed
  bytes, and lifetime peaks. A `Queued` phase exposes admission waiting to apps.

These are estimates of attributable image costs. Filters, parsed trees, frame
references, atlas packing, allocator/driver retention, and the application
baseline are not completely captured. There is no hard 64/128 MiB RSS or GPU cap.
Replacing a pool does not retroactively change reservations in an old pool.

## Native comparison

Apple M4 Max, 128 GiB unified memory, macOS 27.0.1, release build, 2× density,
1140×720 logical window. Eight pinned complex SVGs repeat into 24 independent
IDs with shared parsed documents. Each case has a local 16 MiB output policy.
Visible mode mounts approximately 12–15 tiles including overscan.

Fresh process per case, same binary and proper temporary macOS app bundle.
Each 36-second run covers baseline, zoom, scroll, combined motion, settle and
complete unmount. Earlier unbundled launch probes timed out in the macOS event
loop; they are excluded. The final runner clears old report files before each
attempt, so stale data cannot masquerade as a successful run.

| Mode | Shared pool / max jobs | Peak whole-process footprint | Active redraw p95 | Peak current RGBA sum |
|---|---:|---:|---:|---:|
| Eager, 24 | Independent | 1,426 MiB | 17.5 ms | 383.8 MiB |
| Visible + overscan | Independent | 981 MiB | 16.9 ms | 239.9 MiB |
| Eager, 24 | 64 MiB / 2 | 423 MiB | 17.7 ms | 28.4 MiB |
| Visible + overscan | 64 MiB / 2 | 447 MiB | 17.0 ms | 30.4 MiB |
| Eager, 24 | 128 MiB / 2 | 492 MiB | 17.6 ms | 56.8 MiB |
| Visible + overscan | 128 MiB / 2 | 538 MiB | 17.6 ms | 53.3 MiB |

Footprint uses the kernel's lifetime peak; RSS and queue depth are sampled about
every 100ms. Active cadence combines zoom, scroll and mixed phases. These are
host Render intervals, not GPU presentation or input-to-photon latency. All
24-tile cases are single measurements, not a statistical non-regression claim.
Slightly different frame times can cross a row boundary at settling, so the final
visible tile count varies between runs.

The eager 64 MiB case reduced peak physical footprint by about **70%** versus the
fresh independent control. Visible/64 reduced it by about **54%**. Concurrency
never exceeded two. Accounted peaks were 63.9/61.8 MiB for eager/visible 64 MiB,
and 121.5/127.9 MiB for the 128 MiB pool. Every shared case must satisfy the
runner's reservation/concurrency assertions and drain all reservations on unmount.

All 24-tile cases reported every mounted element Ready during the four-second
settle phase. Visible shared cases first reported all-ready about 0.41–0.56s
after settling; eager shared cases did so within the first 0.08s sample. No
failed statuses occurred. Ready means the **budgeted** detail is ready, not that
the requested deep-zoom resolution was fully rendered.

The current RGBA sums show the cost of the memory reduction: shared/64 eager
retained about 28 MiB versus 384 MiB in the independent control. This is a
substantial resolution tradeoff. Lower bounds do not preserve equal image
quality at 16× zoom. Queued/membership rebalancing can temporarily leave tiles
empty while a smaller preview is admitted.

Final footprint after complete unmount was approximately 355–463 MiB for shared
cases versus 727–871 MiB for independent cases. All pool-accounted memory can be
released while the process still holds renderer, allocator and driver residency.
These figures exclude WindowServer and system-wide GPU allocations.

## Scaling to 96 mounted tiles

The final eager 96-tile case used the same 64 MiB/two-job pool. Peak accounted
memory stayed at **63.8 MiB**, concurrency at **two**, and peak process footprint
was **401 MiB** (181 MiB sampled RSS). Active p95/p99 redraw intervals were
17.6/17.6ms, with a 17.7ms maximum. The sampled queue reached 75 requests.
All 96 reported Ready during settling; there were no failed statuses. On
unmount, members, queued jobs, active jobs and managed bytes all returned to zero.

The same pool divided among four times as many elements produces smaller
rasters (about 31 MiB total current RGBA across 96 elements). Many capped targets
fit within the preview size and can reuse that raster throughout deep zoom.
This verifies admission/accounting behavior under higher membership, not equal
image quality or equal raster work at four times the content count. It does
not establish a hard 401 MiB ceiling for arbitrary SVG filters/documents.

## Recommendation

Use one shared pool plus viewport mounting. Start with 128 MiB/two jobs when
zoom quality matters, or 64 MiB/two jobs for tighter gallery memory constraints.
The scheduler itself reduces memory even with eager mounting, but viewport
mounting avoids spending work on unnecessary previews. Keep whole-process
headroom substantially above the pool number and measure on the target hardware.
A lower-memory device and longer repeated sessions remain necessary before
making broad production guarantees. For sharp deep zoom within small budgets,
viewport tiling would be a separate improvement.

[Raw measurements and full cadence/admission table](REPORT.md) ·
[Reproduction commands](../../README.md#native-gallery-and-shared-pools)
