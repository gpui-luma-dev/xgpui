# Shader producer reference

[Paint integration](extension-paints.md) · [GPUI API](gpui-public-api.md) ·
[Preparation and recording](paint-contract-hardening.md)

## Portable Hermite meshes

`MeshGradient` is a CPU-only `ContentSource`, like SolidColor and Gradient. Native
allocation, compilation, scheduling, completion, caching and composition belong
to the host. Metal is the current portable executor; unsupported hosts return a
typed capability error. Mesh-specific Metal/D3D11/wgpu producer adapters and
preparation/cache counters have been removed.

The Hermite algorithm is adapted from [patskovn/MeshGradient](https://github.com/patskovn/MeshGradient),
revision `f16b544336b7bdaf7e3eba4ac6cba50e1160220e` (MIT). The retained notice is in
`extensions/crates/xgpui-shader/LICENSE-MeshGradient`. Portable WGSL lives in
`extensions/crates/xgpui-shader/src/mesh/`.

```rust
use gpui::{prelude::*, px};
use gpui::content::{ContentDraw, PresentationSlotId};
use xgpui_shader::{MeshGradient, MeshGrid, painted};

let grid = MeshGrid::regular(2, 2, vec![
    [1.0, 0.1, 0.2], [0.2, 0.4, 1.0],
    [1.0, 0.8, 0.1], [0.1, 0.8, 0.6],
])?;
let mesh = MeshGradient::new(grid, 18)?;
let slot = PresentationSlotId::new(); // retain per destination across edits
let element = painted().bg_paint(ContentDraw::new(mesh.content()).slot(slot))
    .w(px(480.0)).h(px(320.0)).rounded_lg();
```

Retain snapshots in view state. Clones retain content identity. `with_grid` and
`with_subdivisions` create new immutable snapshots; the host shares native
pipelines across sources. `content()` is used for backgrounds, borders and paths;
owned MeshGradient also converts into Content/Paint. Keep a distinct presentation
slot for each changing paint role, preserving completed pixels during updates.

| Portable graph stage | Behavior |
| --- | --- |
| Control upload | Immutable 48-byte records: position/U tangent, V tangent, opaque encoded-sRGB color |
| Tessellation compute | One guarded WGSL dispatch evaluates the Hermite basis and expands a deterministic triangle list into 32-byte position/color records |
| Mesh render | Vertex-stage readonly storage reads, one triangle-list draw, RGBA8 sRGB/straight-alpha output cleared transparent |

Patch-local samples and triangle order are preserved. Shared-edge points have the
same basis; no artificial cross-patch cells connect them. Positions/tangents may
produce folds, overlap or gaps; later triangles replace earlier ones, outside
regions stay transparent, and final destination coverage/opacity remains native.
Colors use Hermite RGB interpolation with zero color tangents, clamped after
triangle interpolation. No Oklab, wide-gamut output or grain is added.

Limits remain 2..=16 points per axis, 2..=64 samples per patch edge, finite
positions/tangents in -16..=16 and RGB in 0..=1. The 64 MiB geometry cap now counts
control upload plus expanded triangles; coefficient and intermediate sample
buffers are no longer allocated. `MeshStats::evaluated_vertices` counts unique
patch-local samples; `buffer_bytes` reports those actual graph buffer payloads.
Host budgets additionally account for native allocation sizes and in-flight
retention. Different output extents share the same uploaded controls and computed
triangles within one graph.

Native acceptance covers independent CPU triangle pixels for regular, curved,
inset and folded geometry at multiple subdivision levels; pending edits,
continuous updates, pipeline/cache reuse, resize, and bounded 512→1→512 workloads
with zero producer readbacks. Paint-lab also checks CPU Hermite samples,
background/border/path coverage, clipping, opacity, immutable replay and failed
frame recovery. See the extension and paint-lab READMEs for commands.

## Portable analytic gradients

Linear, elliptical radial, conic and diamond paints are CPU-only `ContentSource`
snapshots. The host owns portable execution (currently Metal). For example:

```rust
use gpui::{prelude::*, *};
use xgpui_shader::{painted, Gradient, GradientStop, WindowPaintExt};

let stops = vec![
    GradientStop::new(0.0, [1.0, 0.1, 0.2, 1.0]),
    GradientStop::new(0.4, [1.0, 0.7, 0.1, 1.0]),
    GradientStop::new(1.0, [0.1, 0.3, 1.0, 1.0]),
];
let radial = Gradient::radial([0.5, 0.5], [0.5, 0.5], stops)?;
painted().bg_paint(radial.content());
painted().border_8().border_paint(radial.content());
window.paint_path_with_paint(path, radial.content());
```

Constructors: `Gradient::linear(angle, stops)`,
`Gradient::radial(center, radius, stops)`,
`Gradient::conic(center, angle, stops)`, and
`Gradient::diamond(center, radius, angle, stops)`. `Gradient::new(kind, stops)`
also accepts a `GradientKind`; `with_kind` and `with_stops` make immutable
snapshots. Clones retain identity. Native caching, completion and diagnostics belong
to the host; producer preparation/pipeline counters have been removed.

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
- Each output uses a portable fullscreen pass into RGBA8 sRGB/straight-alpha,
  followed by native composition. The host shares structural realizations and owns
  asynchronous GPU execution, cache budgets and in-flight retention.

Run `just linear`, `just radial`, `just conic`, `just diamond` or `just mesh` in
`apps/xgpui-ext-lab/paint-lab/`. Every type can paint the same
background, border and path examples. `just headless --effect diamond --frames 30`
runs GPU checks and a deterministic static workload.

## Host-owned portable realization

SolidColor, Gradient and MeshGradient use the same portable lifecycle. Producers
have no native output cache or `cached_texture_info()` API. Native acceptance
observes host statistics; normal app code retains immutable content and paint
identities. See [portable content lifecycle](portable-content-api-design.md).

Mesh adaptation: MIT, copyright 2022 Nikita Patskov and others; see the crate’s
`LICENSE-MeshGradient` for the retained notice.
