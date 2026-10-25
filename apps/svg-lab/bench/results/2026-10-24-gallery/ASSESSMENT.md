# Reusable SVG element: native gallery assessment

Follow-up: [shared scheduler implementation and native comparison](../2026-10-24-scheduler/ASSESSMENT.md).

The element kept host redraws responsive on this machine, but the stress test
found a missing GPU image lifecycle step. Replaced and unmounted raster outputs
now explicitly leave GPUI's sprite atlas. A regression test paints actual atlas
entries and verifies removal after both replacement and entity release.

## Workload and evidence

Apple M4 Max, 128 GiB unified memory, macOS 27.0.1, release binary, 2× window
scale. Eight pinned SVG files are repeated into 24 independent element IDs.
The parsed documents are shared. Coverage includes dense stroked curves,
gradient-heavy car/camera illustrations, filters, organic shapes, a map,
Inkscape artwork and a static text/chart. The map contains a game overlay whose
script is not executed: it exercises only static rendering.

Eight native process runs: four before cleanup, four after. Every run lasts
36 seconds, with baseline, continuous 1×–16× zoom, scrolling, combined motion,
settling and complete unmount. Visible mode instantiates 12–15 elements at most
with a three-column viewport and one row of overscan. There are no SVG-history
caches. Corpus hashes, binary hashes and hardware metadata accompany raw data.

These are single runs per configuration, not statistical trials. A short
compile/test check overlapped the beginning of the first post-fix baseline;
active zoom/scroll phases ran after it completed. Background desktop activity
and display scheduling can affect intervals. The before visible/4 case had
late baseline-independent frame gaps during settle/unmount; raw data retains
these instead of treating them as raster-induced stalls.

## Results after cleanup

Peak footprint below uses libproc's kernel-recorded lifetime peak, not just the
100ms samples. Final footprint is the last sample after approximately four
seconds with no SVG elements mounted.

| Mounting | Output budget per element | Active redraw p95 | Peak RSS (MiB, sampled) | Peak physical footprint (MiB, kernel) | Final footprint after unmount (MiB) |
|---|---:|---:|---:|---:|---:|
| Eager: 24 | 16 MiB | 17.6 ms | 1,209 | 1,414 | 863 |
| Eager: 24 | 4 MiB | 17.3 ms | 403 | 619 | 501 |
| Visible + overscan: ≤15 | 16 MiB | 17.6 ms | 784 | 989 | 735 |
| Visible + overscan: ≤15 | 4 MiB | 17.6 ms | 294 | 511 | 431 |

Active = zoom + scroll + mixed phases. Medians were about 16.7ms. Worst active
intervals were 19.7, 33.3, 17.7 and 33.4ms respectively. No sampled failed
statuses occurred in any run. These host timings are consistent with roughly
60 redraws/second; they do not prove input-to-photon or GPU presentation latency.
The native interactive gallery was also checked visually while wheel scrolling:
newly visible artwork mounted and rendered correctly.

## What the stress test caught

Before cleanup, visible/16 reached **2,094 MiB**, and remained at that footprint
after unmount. After cleanup it peaked at **989 MiB** and finished at **735 MiB**.
Visible/4 changed from **976 MiB** peak/final to **511 MiB** peak and **431 MiB**
final. Recycling elements without atlas eviction made repeated scrolling worse
than eager mounting in the original run. Merely dropping CPU image handles was
insufficient.

Eager/16 before cleanup peaked at 1,544 MiB and ended at 1,389 MiB; after cleanup
those numbers were 1,414 and 863 MiB. This smaller peak improvement is expected:
all 24 current high-resolution outputs still need memory. The nominal maximum
current RGBA sum alone was about 384 MiB. The element's output policy is not a
whole-process memory budget.

Unmount does not return the process to its initial baseline within four seconds.
Allocator/driver retention, parsed documents, app/renderer allocations and any
other residency remain in the measurement. Atlas removal is verified directly;
the remaining footprint has not been attributed by allocation category and is
not claimed to be either fully reclaimable or a proven leak.

## Recommendations

1. Use visible rows plus modest overscan for galleries, and start with a
   **4 MiB per-element policy**. The lab keeps 16 MiB as its single-artwork
   default; this test does not justify changing that default for every app.
2. Budget for whole-process footprint, not just `width × height × 4`.
   The recommended tested gallery still peaked at about **0.5 GiB** here.
3. Add a shared scheduler/aggregate budget before claiming bounded rendering
   across arbitrary numbers of elements. Current jobs and budgets are bounded
   per mounted element; 24 elements can schedule 24 independent workers.
4. Test on lower-memory hardware and repeat longer sessions before production
   claims. This high-end 128 GiB machine does not establish mobile/integrated
   GPU or low-memory behavior. Repeated tests can use `gallery_run.py --repeat 2`.

Physical footprint and RSS cover the process, including process-accounted
GPU allocations; they exclude WindowServer and system-wide GPU memory. No
per-texture GPU allocation attribution, hardware input latency, or long-duration
leak slope is measured. JSON recording and sampling add some overhead.

[Before phase tables](before/REPORT.md) · [After phase tables](after/REPORT.md)

Reproduce with the commands in the [lab README](../../../README.md).
