# Shader producer reference

[Paint integration](extension-paints.md) · [GPUI API](gpui-public-api.md) ·
[Preparation and recording](paint-contract-hardening.md)

## Metal mesh source and pipeline

Use [patskovn/MeshGradient](https://github.com/patskovn/MeshGradient), reviewed at
commit [`f16b544`](https://github.com/patskovn/MeshGradient/tree/f16b544336b7bdaf7e3eba4ac6cba50e1160220e),
as the primary algorithm and Metal source reference. MeshingKit is useful for
component ergonomics and presets, but wraps Apple's built-in mesh renderer.

### Mapping into the extension

| Reference stage | Adaptation in xgpui-shader |
| --- | --- |
| ControlPoint / Grid | Validated immutable mesh snapshot: dimensions, positions, tangents, RGB colors |
| shuffleCoefficients compute kernel | Build per-patch Hermite geometry/color coefficient matrices |
| computeHermitPatchMatrix compute kernel | Evaluate interpolated positions/colors on a subdivided grid |
| computeMeshTrianglePrimitives compute kernel | Expand evaluated vertices into triangle geometry; consider indexed geometry later |
| prepareToDrawShader / drawRasterizedMesh | Draw geometry into the extension's offscreen texture |
| Optional computeNoize | Not implemented |
| MetalMeshRenderer / SwiftUI wrapper | Replace MTKView, default device, frame loop, and presentation with the retained GPUI element and host-device preparation contract |

The producer records compute and render commands, returns output and commands to
the host, and never presents or commits them itself. GPUI composites the result
at scene order with opacity and coverage. The generic integration must support
this sequence without mesh-specific changes in GPUI.

## Mesh producer

`MeshGradient` implements the same `CustomContent` protocol as `SolidColor`.
The Metal implementation lives in `extensions/xgpui-ext/crates/xgpui-shader/src/mesh/`; attribution is in `extensions/xgpui-ext/crates/xgpui-shader/LICENSE-MeshGradient`.

```rust
use gpui::{prelude::*, px};
use std::sync::Arc;
use xgpui_shader::{MeshGradient, MeshGrid, painted};

let grid = MeshGrid::regular(2, 2, vec![
    [1.0, 0.1, 0.2], [0.2, 0.4, 1.0],
    [1.0, 0.8, 0.1], [0.1, 0.8, 0.6],
])?;
let mesh = Arc::new(MeshGradient::new(grid, 18)?);
let element = painted().bg_paint(mesh.clone()).w(px(480.0)).h(px(320.0)).rounded_lg();
```

Keep the Arc for static reuse. To edit/animate, construct a validated grid from
new control points and use `mesh.with_grid(new_grid)`; this creates an immutable
snapshot while sharing compiled pipelines. `with_subdivisions` does the same for
quality changes. Do not call `MeshGradient::new` every animation frame, which
would discard the shared pipeline cache.

Public `ControlPoint` values have normalized top-left-origin positions, u/v
patch tangents, and opaque encoded-sRGB RGB colors. `MeshGrid::regular` initializes
straight, uniformly spaced geometry. For custom geometry, clone `grid.points()`,
edit the points, and validate them with `MeshGrid::new(width, height, points)`.
Positions/tangents can produce folds or gaps; clipping and deterministic triangle
order are supported, but this API does not repair self-intersecting geometry.

The adapted patskovn pipeline computes Hermite coefficient matrices, evaluates
patch-local vertices, expands triangles, then records one triangle-list draw.
Shared-edge samples are duplicated per patch rather than connected by artificial
cross-patch cells. Each compute dispatch is guarded; tracked-resource encoder
boundaries order the stages. The host alone commits the recorded command buffer.

Rust upload records have explicit 16-byte alignment and float4-sized fields;
Metal compile-time assertions and unit tests check strides/offsets. Colors use
the reference's Hermite RGB interpolation with zero color tangents. This is not
Oklab interpolation or high-gamut output. Grain is not included in this slice.

Resource limits: 2..=16 control points per axis; 2..=64 samples per patch edge;
64 MiB maximum geometry buffers and 64 MiB maximum output texture per generation.
Finite positions/tangents must be in -16..=16, RGB in 0..=1. Geometry is immutable
per snapshot, output is cached by queue/extent, and compilation is
shared per current host queue across related snapshots. Animated generations
currently allocate fresh buffers/textures; pooling and total in-flight budgets
remain future performance work.

Real-GPU tests compare against an independent CPU Hermite basis evaluator for
2x2, rectangular 3x2, and deformed 3x3 grids. They also check complete coverage,
replay, resize/opacity, old snapshot preservation, shared pipeline compilation,
minimum subdivisions, shared first-use output, and aborted-frame retry.

## Analytic gradients (Metal)

Linear, elliptical radial, conic and diamond paints use the same `CustomContent`
contract as the mesh. No additional GPUI changes are needed. For example:

```rust
use gpui::{prelude::*, *};
use std::sync::Arc;
use xgpui_shader::{painted, Gradient, GradientStop, WindowPaintExt};

let stops = vec![
    GradientStop::new(0.0, [1.0, 0.1, 0.2, 1.0]),
    GradientStop::new(0.4, [1.0, 0.7, 0.1, 1.0]),
    GradientStop::new(1.0, [0.1, 0.3, 1.0, 1.0]),
];
let radial = Arc::new(Gradient::radial([0.5, 0.5], [0.5, 0.5], stops)?);
painted().bg_paint(radial.clone());
painted().border_8().border_paint(radial.clone());
window.paint_path_with_paint(path, radial.clone());
```

Constructors: `Gradient::linear(angle, stops)`,
`Gradient::radial(center, radius, stops)`,
`Gradient::conic(center, angle, stops)`, and
`Gradient::diamond(center, radius, angle, stops)`. `Gradient::new(kind, stops)`
also accepts a `GradientKind`; `with_kind` and `with_stops` make immutable
snapshots sharing a pipeline cache. `preparations()` counts generated textures;
`pipeline_compilations()` counts compilations shared by related snapshots.

- 2–32 stops, sorted offsets in 0..=1; duplicates create hard edges. The last
  stop at a duplicate offset wins. Values outside the first/last stops clamp.
- RGBA channels are finite encoded-sRGB values in 0..=1. Interpolation uses
  premultiplied encoded sRGB and returns straight-alpha output, avoiding colored
  fringes from transparent stops. Oklab, linear-light interpolation, dithering,
  repeat/mirror spread and HDR are not implemented in these producers.
- Centers are normalized to the bounds (finite -16..=16). Radius components are
  fractions of full width/height (0.0001..=16); [0.5, 0.5] reaches edge midpoints.
- Linear and conic angles are clockwise degrees, zero pointing up. Linear
  projection and conic angles use physical aspect ratio. At a conic center the
  first angle is chosen deterministically; differing endpoint colors create a seam.
- Diamond sampling uses Manhattan distance after rotation in bounds-normalized
  axes; zero degrees is unrotated. Radial sampling uses elliptical distance.
- Each generated output uses one full-target render draw into BGRA8Unorm,
  followed by GPUI's normal custom paint composition. Static output is cached by
  queue and extent; each texture is capped at 64 MiB. Animation still allocates
  immutable output textures; resource pooling remains later work.

Run `just linear`, `just radial`, `just conic`, `just diamond` or `just mesh` in
`apps/paint-lab/`. Every type can paint the same
background, border and path examples. `just headless --effect diamond --frames 30`
runs GPU checks and a deterministic static workload.

## Built-in Metal output cache

```text
SolidColor / Gradient / MeshGradient
  key: queue identity + exact extent
  budget: 64 MiB live textures / snapshot
  eviction: individual LRU entries
  hit before submission: same pending command + texture
  hit after submission: texture, no new command
```

No GPUI API changes. Live-cache bytes exclude geometry buffers, in-flight outputs
and driver allocations. `cached_texture_info()` reports all live cached textures.

Mesh adaptation: MIT, copyright 2022 Nikita Patskov and others; see the crate’s
`LICENSE-MeshGradient` for the retained notice.
