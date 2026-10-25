# Shared scheduler: native gallery measurements

24 tiles unless marked 96, 16 MiB local output policy, same 36-second workload as the earlier gallery. Shared pools allow two concurrent jobs. Retained images charge 2× RGBA, in-flight work 3× RGBA. Limits apply to these reservations, not total process memory. Peak footprint is kernel recorded; RSS is sampled approximately every 100ms. Active cadence combines zoom, scroll and mixed phases. All-settled timing uses reported statuses, not GPU presentation.

| Case | p95 / p99 (ms) | Max (ms) | >33ms (%) | Peak RSS / footprint (MiB) | Peak managed (MiB) | Peak jobs / sampled queue | Settled ready | First all-ready after settle (s) |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| independent/eager-16 | 17.5 / 17.6 | 33.6 | 0.07 | 1218 / 1426 | 0.0 | None / None | 24/24 | 0.06 |
| independent/visible-16 | 16.9 / 17.5 | 33.4 | 0.07 | 772 / 981 | 0.0 | None / None | 15/15 | 0.49 |
| shared-128/eager-16 | 17.6 / 17.7 | 17.7 | 0.00 | 262 / 492 | 121.5 | 2 / 10 | 24/24 | 0.08 |
| shared-128/visible-16 | 17.6 / 17.6 | 17.7 | 0.00 | 328 / 538 | 127.9 | 2 / 13 | 15/15 | 0.41 |
| shared-64/eager-16 | 17.7 / 17.7 | 25.3 | 0.00 | 191 / 423 | 63.9 | 2 / 14 | 24/24 | 0.02 |
| shared-64/visible-16 | 17.0 / 17.7 | 33.4 | 0.07 | 224 / 447 | 61.8 | 2 / 13 | 12/12 | 0.56 |
| shared-64-96tiles/eager-16 | 17.6 / 17.6 | 17.7 | 0.00 | 181 / 401 | 63.8 | 2 / 75 | 96/96 | 0.03 |
