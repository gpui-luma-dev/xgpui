# Paint and shader extension

Opt-in downstream extension: `xgpui-shader`. No app-side OS branches.

## GPUI public API changes: additions and compatibility

This crate requires GPUI's `Window::paint_custom`, producer/preparation traits,
geometry and retained custom-draw records, plus backend recording adapters.
It also alters existing public types: `Scene` gains `custom_draws`, and
`Primitive` / `PrimitiveBatch` gain `Custom` variants. Complete struct literals
and exhaustive matches must be updated. Ordinary `div().bg(...)` callers need
no call-site changes; opting out of the crate does not undo these SDK changes.

**[GPUI public API changes: additions and compatibility](gpui-public-api.md#altered-existing-public-types--source-compatibility-risks).**
See also [added paint entry point](gpui-public-api.md#added-window-method),
[producer/preparation traits](gpui-public-api.md#added-producer-trait-and-preparation-request),
[geometry and retained records](gpui-public-api.md#added-geometry-and-retained-records),
and [backend APIs](gpui-public-api.md#added-backend-crate-public-apis).

## Add the crate

From an app under `apps/`:

```toml
[dependencies]
xgpui-shader = { path = "../../extensions/xgpui-ext/crates/xgpui-shader" }

# For user-authored WGSL:
# xgpui-shader = { path = "../../extensions/xgpui-ext/crates/xgpui-shader", features = ["portable-shaders"] }
# Add "wgpu" for the optional wgpu renderer.
```

## Paint a rounded container

```rust
use anyhow::Result;
use gpui::{prelude::*, *};
use std::sync::Arc;
use xgpui_shader::{painted, Gradient, GradientStop};

fn gradient_panel() -> Result<impl IntoElement> {
    let gradient = Arc::new(Gradient::radial(
        [0.5, 0.5],       // Center, normalized to the full outer bounds.
        [0.5, 0.5],       // Ellipse radii as fractions of width/height.
        vec![
            GradientStop::new(0.0, [1.0, 0.2, 0.3, 1.0]),
            GradientStop::new(0.4, [1.0, 0.7, 0.1, 1.0]),
            GradientStop::new(1.0, [0.1, 0.3, 1.0, 1.0]),
        ],
    )?);

    Ok(painted()
        .w(px(320.0)).h(px(240.0))
        .rounded_lg().border_8()
        .bg_paint(gradient.clone())
        .border_paint(gradient)
        .overflow_hidden()
        .child(div().p_4().child("Native GPUI children")))
}
```

Explicit materials override native colors on `painted()`. Rebuild to change them;
custom materials do not participate in native hover/theme refinements.

## Interaction and hover

`Painted` implements `Styled` and `ParentElement`, not `InteractiveElement`.
Put `.id()`, `.on_click()` and `.hover()` on a native `div()`:

```rust
fn clickable_panel(material: Arc<Gradient>) -> impl IntoElement {
    div()
        .id("paint-card")
        .on_click(|_, _, _| println!("Card clicked"))
        .hover(|s| s.opacity(0.85))
        .child(painted()
            .w(px(320.0)).h(px(240.0))
            .bg_paint(material))
}

// Or put interaction on native children inside the painted container.
fn panel_with_action(material: Arc<Gradient>) -> impl IntoElement {
    painted()
        .w(px(320.0)).h(px(240.0))
        .bg_paint(material)
        .child(div()
            .id("apply")
            .on_click(|_, _, _| println!("Apply clicked"))
            .hover(|s| s.bg(rgb(0x334155)))
            .child("Apply"))
}
```

Wrapper hover styles can change native properties such as opacity. Custom
materials stay outside GPUI's `StyleRefinement`: to swap a material on hover,
update view state with the wrapper's `.on_hover(...)`, call `cx.notify()`, and
select `.bg_paint(...)` / `.border_paint(...)` when rebuilding the view.

## Choose a gradient

Same `stops: Vec<GradientStop>`; angles are clockwise degrees, zero pointing up.

```rust
let linear  = Gradient::linear(45.0, stops.clone())?;
let radial  = Gradient::radial([0.5, 0.5], [0.5, 0.35], stops.clone())?;
let conic   = Gradient::conic([0.5, 0.5], 90.0, stops.clone())?;
let diamond = Gradient::diamond([0.5, 0.5], [0.5, 0.5], 30.0, stops)?;
```

2–32 stops; offsets and encoded-sRGB RGBA channels are in `0..=1`.
Duplicate offsets create hard edges. Output is SDR, not HDR/P3.

## Create and update a mesh

```rust
use xgpui_shader::{MeshGradient, MeshGrid};

let grid = MeshGrid::regular(2, 2, vec![
    [1.0, 0.1, 0.2], [1.0, 0.8, 0.1],
    [0.2, 0.3, 1.0], [0.1, 0.9, 0.7],
])?;
let mesh = Arc::new(MeshGradient::new(grid.clone(), 18)?);

// New immutable snapshot; related snapshots share compiled pipelines.
let mut points = grid.points().to_vec();
points[0].position = [-0.15, 0.2];
let next_grid = MeshGrid::new(2, 2, points)?;
let next_mesh = Arc::new(mesh.with_grid(next_grid)?);

let current = painted().bg_paint(mesh.clone());
let next = painted().bg_paint(next_mesh);
```

Animation interpolates control points, then builds a new snapshot. Use paint-lab
for interactive mesh animation and paint editing.

```sh
# Independent CPU Hermite reference; topology, immutable snapshots and retry.
cargo test --locked --config .cargo/validation.toml --manifest-path apps/paint-lab/Cargo.toml native_mesh_matches_cpu
# Solid output: alpha, clipping, replay, reuse, resize and aborted preparation.
cargo test --locked --config .cargo/validation.toml --manifest-path apps/paint-lab/Cargo.toml native_producer_pixels
```

## Reuse one producer across paint sites

Inside an element/canvas paint callback, with an existing `path: Path<Pixels>`:

```rust
use xgpui_shader::WindowPaintExt;

window.paint_path_with_paint(path, mesh.clone());

window.paint_quad_with_paints(
    quad(
        bounds,
        Corners::all(px(16.0)),
        transparent_black(),
        Edges::all(px(8.0)),
        transparent_black(),
        BorderStyle::Solid,
    ),
    mesh.clone(),       // Fill.
    mesh.clone(),       // Border: same field over the full outer rectangle.
);
```

Clipping never remaps the source. Paths use original path bounds; backgrounds
and borders use full outer element bounds. GPUI applies coverage and opacity once.

## Native versus extension APIs

```rust
// Upstream-shaped GPUI styling:
div().bg(rgb(0x223344)).border_1().border_color(rgb(0xffffff));

// Extension-owned values and container:
use xgpui_shader::Paint;
let native = Paint::from(rgb(0x223344));
let custom = Paint::from(mesh.clone());
painted().bg_paint(custom).border_1().border_paint(native);

// Native Paint values serialize. Custom producer values reject serialization.
```

Core hook, called during painting:

```rust
use gpui::CustomGeometry;

window.paint_custom(
    CustomGeometry::RoundedRect {
        bounds,
        corner_radii: Corners::all(px(16.0)),
    },
    mesh.clone(),
);
window.paint_custom(CustomGeometry::Border(border_quad), mesh.clone());
window.paint_custom(CustomGeometry::Path(path), mesh.clone());
```

| GPUI | xgpui-shader |
| --- | --- |
| `CustomContent`, `PreparationRequest` | `Paint`, `painted()`, `WindowPaintExt` |
| Retained geometry, clip, opacity, coverage | Mesh, analytic gradients, WGSL |
| Backend recording, validation, submission | Producer pipelines and output caches |

Against `9b64657`, native `Fill`, `Style`, `Styled` and generated border setters
retain their upstream shape. The independent overflow-axis fix remains in core.
Source/coverage caches are separate; only the host submits. Custom glyph/shadow
paints, shared cross-shape coordinates and high-gamut presentation are deferred.

## Run the examples

```sh
just --justfile apps/paint-lab/justfile radial
just --justfile apps/paint-lab/justfile mesh
just --justfile apps/paint-lab/justfile metal-test
just --justfile apps/paint-lab/justfile portable-test
```

[Producer reference](shader-producers.md) ·
[Preparation and failure contracts](paint-contract-hardening.md) ·
[Portable backend status](portable-paints.md) ·
[User-authored WGSL](portable-wgsl-v1.md)
