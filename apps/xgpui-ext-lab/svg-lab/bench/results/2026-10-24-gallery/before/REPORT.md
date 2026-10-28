# Native SVG gallery stress measurements

24 tiles, eight shared parsed complex SVGs, 1140 × 720 logical window. Each case runs in its own process for 36 seconds: baseline 4s, zoom 8s, scroll 8s, mixed 8s, settle 4s, unmount 4s. Visible mode explicitly removes SVG elements outside the viewport plus one overscan row. Eager mode instantiates all 24.

Frame intervals are host Render invocation intervals under continuous frame demand, including scheduling, rendering and competition with workers. They are not GPU presentation or input-to-photon measurements. Memory is whole-process RSS and macOS physical footprint from libproc at approximately 100ms intervals; lifetime peak footprint is kernel recorded. Process-accounted graphics allocations are included, but WindowServer/system-wide GPU memory is not. Displayed RGBA is only the nominal current output size reported by mounted elements, not total memory. Sampling and JSON recording add overhead. Initial parsing is excluded from frame timings. No other benchmark runs concurrently.

| Case | Phase | p50 / p95 / p99 ms | Max ms | >33ms % | RSS MiB | Footprint MiB | RGBA MiB | Mounted |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| eager-16 | baseline | 16.7 / 17.7 / 17.7 | 17.7 | 0.0 | 180 | 369 | 18.4 | 24 |
| eager-16 | zoom | 16.7 / 17.7 / 17.7 | 17.7 | 0.0 | 1126 | 1346 | 340.2 | 24 |
| eager-16 | scroll | 16.7 / 16.9 / 17.7 | 26.4 | 0.0 | 1323 | 1543 | 383.8 | 24 |
| eager-16 | mixed | 16.7 / 16.9 / 17.2 | 18.6 | 0.0 | 1323 | 1543 | 383.8 | 24 |
| eager-16 | settle | 16.7 / 16.9 / 17.3 | 18.7 | 0.0 | 1323 | 1544 | 383.8 | 24 |
| eager-16 | unmounted | 16.7 / 16.9 / 17.0 | 33.6 | 0.4 | 1206 | 1389 | 0.0 | 0 |
| eager-4 | baseline | 16.7 / 17.6 / 17.7 | 27.3 | 0.0 | 178 | 381 | 18.4 | 24 |
| eager-4 | zoom | 16.7 / 17.6 / 17.7 | 26.4 | 0.0 | 436 | 676 | 80.5 | 24 |
| eager-4 | scroll | 16.7 / 16.9 / 17.3 | 34.0 | 0.2 | 486 | 736 | 95.9 | 24 |
| eager-4 | mixed | 16.7 / 16.9 / 17.2 | 32.2 | 0.0 | 487 | 737 | 95.9 | 24 |
| eager-4 | settle | 16.7 / 17.0 / 17.3 | 33.4 | 0.4 | 487 | 737 | 95.9 | 24 |
| eager-4 | unmounted | 16.7 / 16.9 / 17.0 | 18.2 | 0.0 | 487 | 737 | 0.0 | 0 |
| visible-16 | baseline | 16.7 / 17.6 / 17.7 | 17.7 | 0.0 | 141 | 350 | 9.7 | 12 |
| visible-16 | zoom | 16.7 / 17.0 / 17.7 | 31.8 | 0.0 | 614 | 846 | 148.2 | 12 |
| visible-16 | scroll | 16.7 / 16.9 / 17.0 | 34.0 | 0.2 | 1246 | 1494 | 224.7 | 15 |
| visible-16 | mixed | 16.7 / 16.9 / 17.1 | 33.4 | 0.4 | 1591 | 1894 | 195.9 | 15 |
| visible-16 | settle | 16.7 / 16.9 / 18.2 | 31.4 | 0.0 | 1792 | 2094 | 239.9 | 15 |
| visible-16 | unmounted | 16.7 / 16.9 / 17.0 | 17.0 | 0.0 | 1792 | 2094 | 0.0 | 0 |
| visible-4 | baseline | 16.7 / 17.6 / 17.6 | 17.7 | 0.0 | 143 | 339 | 9.7 | 12 |
| visible-4 | zoom | 16.7 / 17.6 / 17.6 | 17.7 | 0.0 | 274 | 509 | 42.8 | 12 |
| visible-4 | scroll | 16.7 / 17.6 / 17.6 | 17.7 | 0.0 | 453 | 724 | 59.9 | 15 |
| visible-4 | mixed | 16.7 / 17.6 / 17.6 | 26.9 | 0.0 | 617 | 937 | 52.0 | 15 |
| visible-4 | settle | 16.7 / 33.4 / 50.0 | 50.0 | 6.6 | 653 | 976 | 48.0 | 12 |
| visible-4 | unmounted | 16.8 / 50.0 / 50.9 | 51.0 | 36.7 | 653 | 976 | 0.0 | 0 |
