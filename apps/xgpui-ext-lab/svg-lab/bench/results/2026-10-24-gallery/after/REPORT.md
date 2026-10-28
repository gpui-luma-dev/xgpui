# Native SVG gallery stress measurements

24 tiles, eight shared parsed complex SVGs, 1140 × 720 logical window. Each case runs in its own process for 36 seconds: baseline 4s, zoom 8s, scroll 8s, mixed 8s, settle 4s, unmount 4s. Visible mode explicitly removes SVG elements outside the viewport plus one overscan row. Eager mode instantiates all 24.

Frame intervals are host Render invocation intervals under continuous frame demand, including scheduling, rendering and competition with workers. They are not GPU presentation or input-to-photon measurements. Memory is whole-process RSS and macOS physical footprint from libproc at approximately 100ms intervals; lifetime peak footprint is kernel recorded. Process-accounted graphics allocations are included, but WindowServer/system-wide GPU memory is not. Displayed RGBA is only the nominal current output size reported by mounted elements, not total memory. Sampling and JSON recording add overhead. Initial parsing is excluded from frame timings. No other benchmark runs concurrently.

| Case | Phase | p50 / p95 / p99 ms | Max ms | >33ms % | RSS MiB | Footprint MiB | RGBA MiB | Mounted |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| eager-16 | baseline | 16.7 / 17.6 / 17.6 | 17.7 | 0.0 | 166 | 361 | 18.4 | 24 |
| eager-16 | zoom | 16.7 / 17.6 / 17.6 | 17.7 | 0.0 | 926 | 1121 | 296.4 | 24 |
| eager-16 | scroll | 16.7 / 17.6 / 17.7 | 19.7 | 0.0 | 1208 | 1410 | 383.8 | 24 |
| eager-16 | mixed | 16.7 / 17.6 / 17.6 | 17.7 | 0.0 | 1209 | 1410 | 383.8 | 24 |
| eager-16 | settle | 16.7 / 17.6 / 17.6 | 17.7 | 0.0 | 1209 | 1410 | 383.8 | 24 |
| eager-16 | unmounted | 16.7 / 17.6 / 17.6 | 17.7 | 0.0 | 844 | 1013 | 0.0 | 0 |
| eager-4 | baseline | 16.7 / 17.6 / 17.6 | 17.7 | 0.0 | 165 | 362 | 18.4 | 24 |
| eager-4 | zoom | 16.7 / 17.6 / 17.7 | 25.8 | 0.0 | 395 | 602 | 88.3 | 24 |
| eager-4 | scroll | 16.7 / 16.9 / 17.0 | 33.2 | 0.0 | 403 | 619 | 95.9 | 24 |
| eager-4 | mixed | 16.7 / 16.9 / 17.3 | 33.3 | 0.0 | 402 | 619 | 95.9 | 24 |
| eager-4 | settle | 16.7 / 16.9 / 17.0 | 17.8 | 0.0 | 402 | 619 | 95.9 | 24 |
| eager-4 | unmounted | 16.7 / 16.9 / 17.6 | 18.3 | 0.0 | 402 | 619 | 0.0 | 0 |
| visible-16 | baseline | 16.7 / 17.6 / 17.6 | 20.8 | 0.0 | 139 | 335 | 9.7 | 12 |
| visible-16 | zoom | 16.7 / 17.6 / 17.6 | 17.7 | 0.0 | 609 | 812 | 177.4 | 12 |
| visible-16 | scroll | 16.7 / 17.6 / 17.6 | 17.6 | 0.0 | 759 | 960 | 195.9 | 15 |
| visible-16 | mixed | 16.7 / 17.6 / 17.6 | 17.6 | 0.0 | 714 | 916 | 195.9 | 15 |
| visible-16 | settle | 16.7 / 17.6 / 17.6 | 17.6 | 0.0 | 784 | 985 | 239.9 | 15 |
| visible-16 | unmounted | 16.7 / 17.6 / 17.6 | 17.7 | 0.0 | 784 | 985 | 0.0 | 0 |
| visible-4 | baseline | 16.7 / 17.6 / 17.6 | 17.7 | 0.0 | 139 | 348 | 9.7 | 12 |
| visible-4 | zoom | 16.7 / 17.6 / 17.6 | 17.6 | 0.0 | 252 | 463 | 42.8 | 12 |
| visible-4 | scroll | 16.7 / 17.6 / 17.6 | 33.4 | 0.2 | 289 | 501 | 60.0 | 15 |
| visible-4 | mixed | 16.7 / 16.9 / 17.0 | 18.0 | 0.0 | 286 | 498 | 52.0 | 15 |
| visible-4 | settle | 16.7 / 16.9 / 17.1 | 17.1 | 0.0 | 294 | 503 | 59.9 | 15 |
| visible-4 | unmounted | 16.7 / 16.9 / 17.5 | 18.0 | 0.0 | 294 | 503 | 0.0 | 0 |
