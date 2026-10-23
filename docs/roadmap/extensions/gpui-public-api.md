# GPUI public API changes

Compared with pre-feature main `9b64657`. **Opt-in usage does not mean zero SDK changes.**

## ALTERED existing public types — source compatibility risks

`gpui/src/scene.rs` — structural diff, unchanged members omitted:

```diff
 pub struct Scene {
     // Existing primitive collections …
+    pub custom_draws: Vec<CustomDraw>,
 }

 pub enum Primitive {
     // Existing variants …
+    Custom(CustomDraw),
 }

 pub enum PrimitiveBatch {
     // Existing variants …
+    Custom(Range<usize>),
 }
```

| Existing caller | Impact |
| --- | --- |
| `Scene { ... }` naming every field | Must add `custom_draws`; `..Default::default()` already accommodates it |
| Exhaustive `match Primitive` | Must handle `Primitive::Custom` |
| Exhaustive `match PrimitiveBatch` | Must handle `PrimitiveBatch::Custom` |
| Ordinary native `div().bg(...)` | No call-site change |

```rust
// Constructing a low-level scene:
let scene = gpui::Scene {
    custom_draws: Vec::new(),
    ..Default::default()
};

// In an existing exhaustive match, add these arms:
// Primitive::Custom(draw) => ...,
// PrimitiveBatch::Custom(range) => ...,
```

## ADDED opt-in window presentation

```rust
use gpui::WindowColorMode;

// Per-window; call outside drawing. Existing default windows stay unchanged.
let status = window.set_color_mode(WindowColorMode::WideGamutSdr)?;
let current = window.color_status();

// Restore before requesting a transparent top-level window:
window.set_color_mode(WindowColorMode::Default)?;
```

| Surface | Addition |
| --- | --- |
| GPUI | `WindowColorMode`, opaque `WindowColorStatus`, `WindowColorFallback`; two `Window` methods. |
| `PlatformWindow` | Same request/report pair with defaults; no required backend overrides. |
| `gpui_apple::MetalRenderer` | Same pair across the windowing/renderer crate boundary; supports headless checks. |
| `gpui_apple::custom_draw` | Opaque `MetalWideSdrOutput`, explicit linear-P3 constructor and `reusable_for`. |

`MetalCustomOutput`, `CustomContent`, `PreparationRequest`, window-option
structs and native color/style types keep their existing contracts. New
configuration/fallback enums are non-exhaustive. Additive method names can still
collide with downstream extension methods. Unsupported requests preserve state.

[Wide surfaces, composition and lifecycle](high-gamut-windows.md).

## ADDED Window method

`gpui/src/window.rs` — one public recording hook:

```rust
// Signature; body omitted.
impl Window {
    pub fn paint_custom(
        &mut self,
        geometry: CustomGeometry,
        content: std::sync::Arc<dyn CustomContent>,
    );
}
```

Existing native `paint_quad` and `paint_path` signatures remain unchanged.
The new method records during painting; it does not expose command submission.

## ADDED producer trait and preparation request

`gpui/src/scene.rs` — public signatures; constructor/accessor bodies omitted:

```rust
pub trait CustomContent: std::fmt::Debug + Send + Sync + 'static {
    fn prepare(
        &self,
        backend: &dyn std::any::Any,
        size: Size<DevicePixels>,
    ) -> anyhow::Result<Box<dyn std::any::Any>>;

    // Provided method: producers implementing only prepare() keep working.
    fn prepare_with_request(
        &self,
        backend: &dyn std::any::Any,
        request: &PreparationRequest,
    ) -> anyhow::Result<Box<dyn std::any::Any>> {
        self.prepare(backend, request.raster_size())
    }
}

pub struct PreparationRequest { /* private fields */ }
impl PreparationRequest {
    pub fn new(
        logical_size: Size<Pixels>,
        raster_size: Size<DevicePixels>,
        scale_factor: f32,
    ) -> anyhow::Result<Self>;
    pub fn logical_size(&self) -> Size<Pixels>;
    pub fn raster_size(&self) -> Size<DevicePixels>;
    pub fn scale_factor(&self) -> f32;
}
```

The entire trait is downstream-added versus upstream. The request method
defaults to `prepare()`; producers override it only when they need logical geometry.

## ADDED geometry and retained records

`gpui/src/scene.rs` — declarations, derives/docs omitted:

```rust
pub enum CustomGeometry {
    RoundedRect {
        bounds: Bounds<Pixels>,
        corner_radii: Corners<Pixels>,
    },
    Border(PaintQuad),
    Path(Path<Pixels>),
}

pub enum CustomCoverage {
    Border { widths: Edges<ScaledPixels>, style: BorderStyle },
    Path(std::sync::Arc<Path<ScaledPixels>>),
}

pub struct CustomDraw {
    pub order: DrawOrder,
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub corner_radii: Corners<ScaledPixels>,
    pub opacity: f32,
    pub content: std::sync::Arc<dyn CustomContent>,
    pub coverage: Option<CustomCoverage>,
}

impl From<CustomDraw> for Primitive { /* Custom(draw) */ }
```

## ADDED backend-crate public APIs

These are GPUI renderer SDK additions, not `xgpui-shader` exports.
Each renderer crate adds `pub mod custom_draw`.

```rust
// gpui_apple::custom_draw — macOS/Metal
pub struct MetalCustomContext {
    pub device: metal::Device,
    pub queue: metal::CommandQueue,
}
// Public method: cache_key() -> u64
pub struct MetalCustomOutput {
    pub texture: metal::Texture,
    pub commands: Vec<metal::CommandBuffer>,
}
```

wgpu and D3D11 public surface, summarized (not executable declarations):

```text
GPUI renderer crate  Context                Output
───────────────────  ─────────────────────  ─────────────────
gpui-wgpu            WgpuCustomContext      WgpuCustomOutput
gpui-windows         Dx11CustomContext      Dx11CustomOutput

Context public field   device
Context public methods cache_key(), create_output(extent), record(callback),
                       can_reuse(output)
Output public method   texture()

Context constructors, validation, submit(), PreparationBatch and compositor
pipelines are private to the renderer. They are not producer APIs.

Windows test-support only:
  gpui_windows::Dx11HeadlessRenderer
  Dx11HeadlessRenderer::new()
  implements existing PlatformHeadlessRenderer
```

Backend APIs are target/feature dependent. App code normally uses portable
`xgpui-shader` producers rather than these low-level recording contracts.

## UNCHANGED native styling SDK

```rust
// These remain GPUI APIs with their upstream type/field shapes:
div().bg(rgb(0x223344)).border_1().border_color(white());

// Fill still has only its native variant:
let Fill::Color(background) = fill;

// No extension paint field in Style or StyleRefinement.
// No GPUI Paint enum or Styled::bg_paint / Styled::border_paint methods.
```

`Style::overflow_mask` has an independent hidden-axis border-inset correction;
its signature and native style fields are unchanged.

## EXTENSION ONLY — not GPUI public APIs

```rust
use xgpui_shader::{
    Paint, Painted, Shader, WindowPaintExt,
    painted, shader,
    Gradient, GradientKind, GradientStop,
    MeshGradient, MeshGrid, ControlPoint, SolidColor,
};

painted().bg_paint(mesh.clone()).border_paint(mesh.clone());
window.paint_path_with_paint(path, mesh.clone());  // WindowPaintExt trait.
// window.paint_quad_with_paints(...)             // Same extension trait.

// With the portable-shaders feature:
use xgpui_shader::experimental::{ShaderProgram, ShaderParameters};
```

## Inspect the cumulative patch

```sh
git diff 9b64657 -- gpui/crates/gpui/src/scene.rs gpui/crates/gpui/src/window.rs
git diff 9b64657 -- gpui/crates/gpui/src/style.rs
# Empty for restored native styling helpers:
git diff 9b64657 -- gpui/crates/gpui/src/styled.rs gpui/crates/gpui-macros/src/styles.rs
```

[Application examples](extension-paints.md) ·
[Preparation and failure contracts](paint-contract-hardening.md)
