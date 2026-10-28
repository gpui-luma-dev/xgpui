# SVG measurements and reproduction

The current experimental contract lives in the [extension README](../../../../extensions/crates/xgpui-svg/README.md).
Reports below preserve the implementation and workload at the time of each run;
older recommendations are historical, not additional unfinished work.

| Evidence | Scope |
|---|---|
| [Initial corpus](results/2026-10-24/ASSESSMENT.md) | Ten SVGs, isolated CPU raster latency/RSS |
| [Bounded policy](results/2026-10-24-policy/REPORT.md) | 80 cases; output budgets and zoom-burst queue behavior |
| [Native gallery](results/2026-10-24-gallery/ASSESSMENT.md) | Atlas lifecycle cleanup; process memory/redraw cadence |
| [Shared scheduler](results/2026-10-24-scheduler/ASSESSMENT.md) | Independent controls, 64/128 MiB pools, 96-element stress case |

Raw JSON, source/binary hashes, environment metadata and readable summaries are
retained beside reports. Empty launcher logs contain no additional evidence.
Native results are single observations on an M4 Max/128 GiB/macOS machine;
repeat and test target hardware before broader claims.

## Corpus and copyright

[corpus.json](corpus.json) pins ten historical inputs by SHA-256 and records their
sources/terms. Only four audited third-party artworks are committed under
[`../samples`](../samples/ATTRIBUTION.md). Their bytes match the historical hashes.
The original ribbon/leaf and portrait fixtures are additional local samples.

Copy the distributable subset without network access:

```sh
python3 apps/xgpui-ext-lab/svg-lab/bench/fetch.py /tmp/svg-bundled --bundled-only
```

Exact historical reproduction requires all ten inputs. Review the
[attribution/exclusion notice](../samples/ATTRIBUTION.md) before fetching optional
sources; this command does not grant any new rights to the artwork:

```sh
python3 apps/xgpui-ext-lab/svg-lab/bench/fetch.py /tmp/xgpui-svg-corpus
cargo build --release --offline --locked --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin svg-lab --target-dir target
```

## Isolated CPU measurements

```sh
python3 apps/xgpui-ext-lab/svg-lab/bench/run.py --corpus /tmp/xgpui-svg-corpus --output /tmp/svg-cpu
python3 apps/xgpui-ext-lab/svg-lab/bench/report.py /tmp/svg-cpu
python3 apps/xgpui-ext-lab/svg-lab/bench/policy_run.py --output /tmp/svg-policy
python3 apps/xgpui-ext-lab/svg-lab/bench/policy_report.py /tmp/svg-policy
```

One worker, optional preview outside timing:

```sh
target/release/svg-lab --benchmark /tmp/xgpui-svg-corpus/rg1024_metal_effect.svg --zoom 4 --density 2 --iterations 5 --preview /tmp/metal.png
```

Workers measure parse/raster/conversion wall time, output bytes, RSS and process
high-water marks. `--sequence` replays the low-level latest-target queue.
`--budget-mib 256` is an old-policy comparison, not a recommended default. These
measurements exclude GPU/UI work. Script-dependent SVGs represent static state.

## Native gallery and shared pools

```sh
python3 apps/xgpui-ext-lab/svg-lab/bench/scheduler_run.py --output /tmp/svg-scheduler
python3 apps/xgpui-ext-lab/svg-lab/bench/scheduler_report.py /tmp/svg-scheduler
```

This runs seven fresh 36-second cases: independent and 64/128 MiB shared pools
with eager/visible mounting, then an eager 96-tile/64 MiB pool. All retain a local
16 MiB policy. Phases cover baseline, zoom, scroll, combined motion, settling and
complete unmount. `--repeat 2` repeats the matrix. The runner verifies corpus
hashes, accounting/concurrency limits and pool drainage. It automatically creates
a temporary macOS bundle and launches with `open -n -W` for native lifecycle
consistency; fresh reports replace old files before each attempt.

For independent 4/16 MiB comparisons use `gallery_run.py` and `gallery_report.py`.
Its `--app-bundle PATH` exposes the same macOS launcher option; `--skip-existing`
resumes completed files. Direct `--gallery-report /tmp/case.json` is also available
on the app. Run in a graphical desktop session; keep the window visible and avoid
other intensive work. Temporary launch bundles are not distribution artifacts.

Host Render intervals are a responsiveness proxy, not GPU presentation or input
to-photon measurements. Memory samples (about 100ms apart) cover whole-process
RSS and macOS physical footprint; peak footprint is kernel recorded. WindowServer
and system-wide GPU memory are excluded. Pool accounting estimates image costs;
filters, atlas packing and allocator/driver retention can exceed those estimates.
Ready/displaying counts are reported element state, not proof of presented pixels.

Lower footprint is purchased partly through lower raster resolution. The 96-tile
case divides the same budget among more elements and often reuses capped previews;
it is not a comparison at equal image quality or equal raster work. See the
[current assessment](results/2026-10-24-scheduler/ASSESSMENT.md) and
[deferred work](../../../../extensions/crates/xgpui-svg/FOLLOW_UPS.md).
