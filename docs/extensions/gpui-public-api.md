# GPUI public API changes

Compared with pre-feature main `9b64657`. **Opt-in usage does not mean zero SDK changes.**

## Changes by extension

| GPUI change | Required by | Role |
| --- | --- | --- |
| `ImageTextureData`; backend `create_image_output`; `MetalImageOutput` | [xgpui-media](media-images.md) | Validated full mip chains and opt-in source-sized textures; existing output extent contracts remain strict |
| `SvgSize::Viewport { size, view_box }` | Browser SVG comparison | Rerasterize a visible document rectangle at exact device-pixel dimensions |
| `ParsedSvg::size()` | [xgpui-svg](svg-rendering.md) | Read the resolved SVG viewport size without reparsing |
| Backend construction override; optional paragraph traits, geometry types, cache and paint entry points | [xgpui-parley](parley-text.md) | Install the text backend; retain shaping/reflow and paint BiDi runs |
| `Scene::custom_draws`, `Primitive::Custom`, `PrimitiveBatch::Custom`, `Window::paint_custom` | [xgpui-shader](extension-paints.md); reused by xgpui-gamut | Record ordered custom content in the native scene |
| `CustomContent`, `PreparationRequest`, `CustomGeometry`, `CustomCoverage`, `CustomDraw` | xgpui-shader; reused by xgpui-gamut | Producer preparation, retained geometry and host coverage |
| Renderer `custom_draw` contexts and ordinary outputs | xgpui-shader; shared contexts also serve xgpui-gamut | Metal/wgpu/D3D11 producer recording and host submission |
| Window color mode/status; typed Metal/D3D11 wide-SDR outputs | [xgpui-gamut](high-gamut-windows.md) | Negotiate and compose explicit linear-P3 surfaces |
| `Style::overflow_mask` border-inset correction | Independent fix | Behavior only; no signature change |
| macOS variable-font selection/identity correction | Native comparison baseline for the Parley lab | Behavior only; no public API addition |

Paint recording is shared infrastructure; window color negotiation is specific
to wide-gamut presentation. Parley uses the existing [glyph atlas ↗](https://en.wikipedia.org/wiki/Texture_atlas), not custom-draw
or wide-gamut APIs. The color math crate is standalone. Gamut depends on color and optionally uses
shader for instanced drawing; shader does not depend on gamut or color.

## ADDED text backend and paragraph seams

**Required by:** xgpui-parley.

- `Application::with_platform_and_text_system(platform, text_system)` selects
  an `Arc<dyn PlatformTextSystem>` before constructing GPUI caches; skips
  `Platform::text_system()`. Existing construction/native initialization is unchanged.
- `PlatformTextSystem::shape_paragraph` is optional and defaults to Unsupported.
- `WindowTextSystem::layout_paragraph` caches immutable
  `PlatformParagraphShape`/`PlatformParagraphLayout` results.
- `paint_paragraph` paints visual glyph runs through the existing atlas.
  `ParagraphPosition` carries logical UTF-8 offset and `TextAffinity`.

[Paragraph contracts](parley-text.md#contracts).

## ALTERED existing public types — source compatibility risks

**Required by:** xgpui-shader; shared custom-content recording is reused by xgpui-gamut.

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

**Required by:** xgpui-gamut.

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

**Required by:** xgpui-shader; also records xgpui-gamut producers.

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

**Required by:** xgpui-shader; reused by xgpui-gamut producers.

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

**Required by:** xgpui-shader; shared recording/coverage for xgpui-gamut.

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

**Required by:** xgpui-shader for ordinary outputs; xgpui-gamut for typed wide-SDR outputs and shared contexts.

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

Windows also adds an explicit wide-output contract:

```rust
// gpui_windows::custom_draw — additive backend SDK, no GPUI core changes
pub struct Dx11WideSdrOutput { /* private fields; Clone */ }
// Dx11CustomContext methods:
// create_wide_sdr_output(extent) -> Result<Dx11WideSdrOutput>
// can_reuse_wide_sdr(&output) -> bool
// record_wide_sdr(&output, callback) -> Result<T>
// Dx11WideSdrOutput::texture() -> &ID3D11Texture2D
```

[Typed Windows output and recording example](high-gamut-windows.md#typed-windows-output).
The Windows renderer accepts this output only while `WideGamutSdr` is active.
Its existing `PlatformWindow` overrides implement negotiation/status; no extra
GPUI core API is added. Existing producer signatures and [sRGB ↗](https://en.wikipedia.org/wiki/SRGB) outputs are unchanged.

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
use xgpui_shader::shader_paint::{ShaderProgram, ShaderParameters};
```

`xgpui_gamut::WideColor` and `xgpui_parley::ParleyTextSystem`, `FaceRegistry`,
`GlyphRasterizer`, `ParagraphShape` and `ParagraphLayout` are also extension-owned.

## Inspect the cumulative patch

```sh
git diff 9b64657 -- gpui/crates/gpui/src/scene.rs gpui/crates/gpui/src/window.rs
git diff 9b64657 -- gpui/crates/gpui/src/style.rs
# Empty for restored native styling helpers:
git diff 9b64657 -- gpui/crates/gpui/src/styled.rs gpui/crates/gpui-macros/src/styles.rs
```

[Application examples](extension-paints.md) ·
[Preparation and failure contracts](paint-contract-hardening.md)


## ADDED SVG viewport size

**Required by:** [xgpui-svg](svg-rendering.md).

`ParsedSvg::size() -> Size<f32>` returns the resolved document viewport in SVG
user units. The extension uses it for aspect-preserving sizing without parsing
the document a second time. Existing parsing/rasterization signatures remain
unchanged. `SvgImage`, policy, scheduler, atlas lifetime handling and aggregate
image accounting live in the extension and depend on existing GPUI element,
executor, state and image APIs.

## ADDED SVG viewport rasterization

`SvgSize::Viewport { size: Size<DevicePixels>, view_box: Bounds<f32> }` lets
`SvgRenderer::render_parsed` rasterize a visible rectangle in resolved document
coordinates into an exact output. The browser comparison uses this to keep curves
sharp while zooming without growing the whole-document output. It retains the
parsed tree and caches unchanged camera/viewport output in the demo.

Each output edge must be 1..=8192; invalid, nonfinite or nonpositive rectangles
return `InvalidSize`. Existing sizing modes retain their behavior. Adding an
enum variant requires downstream exhaustive `SvgSize` matches to handle it.
This output bounds the destination buffer, not resvg filter intermediates or
whole-process memory. It does not add cropped scheduling to `SvgImage` or make
browser rasterization run on a worker.

## Experimental instanced shader additions

`Scene::shader_draws`, `Primitive::Shader`, `PrimitiveBatch::Shaders`,
`SceneShaderProgram`, `ShaderDraw` and `Window::paint_shader` support the
[instanced shader extension](instanced-wgsl-v2.md). Exhaustive scene literals
and enum matches must accommodate these additions. Ordinary styled elements
retain their existing call sites. Programs and backend pipeline ownership are
separate: the extension validates/translates sources, and each host renderer
creates pipelines on its own device.

### Explicit instanced shader output encoding

`ShaderOutputEncoding` is a new non-exhaustive enum with `EncodedSrgb` and
`LinearDisplayP3`. `SceneShaderProgram::new` retains its signature and sRGB
behavior; additive `new_with_output_encoding` and `output_encoding` methods
allow extension compilers to declare immutable output meaning. Its fields remain
private. A wide program's generated wrapper must follow the host color/alpha
contract; metadata alone does not transform arbitrary user shader source.

`MetalHeadlessRenderer::set_color_mode` (macOS) and
`Dx11HeadlessRenderer::set_color_mode` are additive test-support APIs for native
offscreen validation. Window APIs and existing scene/primitive layouts are
unchanged by this addition.

## ADDED source-sized media textures

`ImageTextureData` validates an immutable complete [RGBA8 ↗](https://en.wikipedia.org/wiki/RGBA_color_model) [mip chain ↗](https://en.wikipedia.org/wiki/Mipmap) within 64 MiB
and bounded dimensions. Backend `create_image_output` methods record uploads
without submitting or waiting. Metal uses a distinct `MetalImageOutput` type;
wgpu/D3D11 outputs carry a private source-sized tag set only by these constructors.
Their old `create_output` methods still validate exact raster extents.

The shared custom compositor uses [trilinear image sampling ↗](https://en.wikipedia.org/wiki/Trilinear_filtering); single-level producer
textures retain their previous filtering behavior. No existing public method
signature or public scene-record layout changes in this addition. Decoding,
mip generation, admission policy and image elements belong to `xgpui-media`.
See its [resource and color contracts](media-images.md).

### Worker staging handoff

The media extension now stages full images on bounded native workers. Metal's
`stage_image_upload` owns a worker autorelease pool. wgpu/D3D11 contexts expose
owned `image_upload_context` handles with `stage`; their opaque staged-image types
are bound to the original host queue/context and accepted by `adopt_image_upload`.
Adoption records copy commands without allocating or copying the source pixels.
Output `is_submitted` distinguishes host acceptance from pending/abandoned work;
GPU completion remains a separate concern. Existing context layouts, recording
transactions and synchronous image constructors remain compatible.

`Window::element_opacity()` is now a public read-only getter (previously
crate-visible). Media painting combines it with bounds/content-mask checks to
avoid requesting loading frames for empty, fully clipped or zero-opacity content.
Its calculation and element-drawing-only contract are unchanged.
`Window::with_element_opacity` is also public so external custom elements can
multiply their own style opacity into that inherited value while painting or
prepainting. The media viewport uses it to apply its opacity style exactly once.

### Completion-safe staging pools and scene-frame admission

`ImageStagingPool`, `ImageStagingLease` and their counters provide bounded resource
reuse for backend SPI. Metal/wgpu opaque staging pools retain exclusive leases
through GPU completion; wgpu recording transactions drop unsubmitted guards when
abandoned. The D3D11 staging path remains unpooled pending native validation.

`PreparationFrame` shares loader-keyed count/byte counters for one scene
preparation. `CustomContent::prepare_in_frame` is an optional trait method whose
default calls `prepare_with_request`; old producers need no changes. Window
preparation adapters forward the frame alongside original logical geometry.
Media's policy and defaults live in `xgpui-media`, not in GPUI's renderer types.

### Custom-content image color transforms

`CustomContent::image_color_transform()` is a defaulted method returning three
encoded-sRGB affine RGB rows. Identity is the default, preserving existing
implementations and outputs. The logical-geometry adapter forwards it. Compositors
apply it after sampling/unassociation and before coverage and opacity, preserving
alpha. Native wide-color background adapters retain their separate color contract.
`xgpui-media::ImageFilter`, `FilteredImage`, `MediaImage::filtered`,
`MediaImage::paint_filtered` and `MediaImageView::filter` expose ordered [SDR ↗](https://en.wikipedia.org/wiki/Standard-dynamic-range_video) color
adjustments while sharing source texture uploads. Private compositor parameter
blocks grow from four to seven float4 rows; backend context/output layouts and
public scene record fields are unchanged.

### Contour path effects

`xgpui-svg::path_effects` adds `Contour`, `Segment`, `MeasuredPath` and `PathEffect`.
The measurement cache retains Bézier curves and supports total-length trim plus
per-contour dash phase before native stroke tessellation. This adds no core GPUI
API or scene-layout changes; existing `PathBuilder` and SVG raster APIs retain
their behavior. Measurement and effect fragmentation have explicit work limits.

### Native video snapshots and bounded presentation

`xgpui-media` adds `VideoFrame`, `VideoStream`, `VideoSender`, `VideoStats`,
`VideoView`, `video` and `video_frame`. These are additive APIs; existing image
loaders and scene records are unchanged. The macOS re-export `CoreVideoFrame`
provides a checked BGRA/IOSurface import with an unsafe external synchronization
and immutability contract. Source-sized Metal outputs now accept [BGRA8 ↗](https://en.wikipedia.org/wiki/RGBA_color_model) as well as
RGBA8. The Metal compositor retains custom-content producers through sampling
completion, allowing immutable external frames to outlive their source scenes.

Legacy Metal `PaintSurface` rendering accepts BGRA plus the existing full-range
[NV12 ↗](https://en.wikipedia.org/wiki/YCbCr) format. Unsupported formats propagate errors and close active encoders;
CoreVideo views/pixel buffers remain alive until GPU completion. This does not
change public `PaintSurface` fields or `Window::paint_surface` signatures.
This macOS slice supplies no decoder. Windows DXGI and Linux DMA-BUF adapters
are described separately below; system playback remains separate issue #40.

### Windows native BGRA snapshots

`gpui_windows::custom_draw::dxgi::DxgiFrame` imports dedicated immutable same-device
BGRA8 D3D11 render targets for source-sized sampling; `xgpui-media` re-exports it
and adds `VideoFrame::dxgi`. Construction is unsafe: callers synchronize producer
writes and preserve immutability through the native resource lifetime. The adapter
direct constructor rejects keyed mutexes and foreign devices. The separate unsafe
`try_from_shared_bgra` constructor opens an NT handle on the host device and
nonblockingly acquires a keyed read lease; clones/prepared outputs retain it until
submission/discard, with the last reference handing ownership back to the producer.
Timeout returns no frame; abandonment requires resource recreation. No decoder or playback API is added. Windows
compilation is checked; native runtime verification is outstanding.

### Linux Vulkan DMA-BUF snapshots

The opt-in `gpui-wgpu/dma-buf` module adds `DmaBufLayout` and `DmaBufFrame`;
`xgpui-media/wgpu` re-exports them and adds `VideoFrame::dma_buf`. The initial
unsafe adapter owns an FD for a dedicated immutable linear BGRA snapshot with
producer synchronization already complete and EXTERNAL/GENERAL Vulkan ownership.
Preparation imports the native allocation, records host queue acquisition and
caches a source-sized output. Device capability is enabled only when advertised.
No decoder, sync-file import, pool-recycle signal or foreign-family GBM/EGL import
is provided. Linux compilation and host layout tests pass; native GPU validation
is pending. See the media guide for exact layout and lifetime requirements.

### macOS NV12 snapshots

`CoreVideoFrame::from_nv12(buffer, Nv12Matrix)` adds opaque 8-bit two-plane
CoreVideo NV12 import to the existing styled video element. `xgpui-media`
re-exports `Nv12Matrix::{Bt601, Bt709}`. Range is selected from the buffer's 420f /
420v format. Construction remains unsafe: producer synchronization, immutable
pixels/attachments, matching SDR matrix and centered chroma are required.

Metal imports R8/RG8 plane views and samples/converts them in the existing custom
compositor pass. Internal prepared output/uniform handling changes; public
`MetalCustomOutput` fields and existing generic output contracts remain unchanged.
Legacy `PaintSurface` behavior is preserved. No decoder, playback clock, transfer
conversion or [HDR ↗](https://en.wikipedia.org/wiki/High_dynamic_range) support is added. All four range/matrix combinations have native
release pixel/lifetime coverage and an interactive generated-source lab mode.


### Synchronous mip-chain preparation

`xgpui_media::mipmapped_rgba8(width, height, rgba)` is an additive extension helper
returning validated `gpui::ImageTextureData`. It uses the native loader's same
alpha-aware linear-light area filtering, dimension and complete-payload bounds.
It performs synchronous CPU work without loader reservations or worker scheduling;
use it for small already-decoded fixtures or with a host-managed executor. The
browser media examples use it with the existing wgpu image-output SPI; the native
loader's threading contract is unchanged. Existing GPUI call sites are unaffected.

## Media pipeline CPU contracts (issue #26 Phase 1)

`xgpui_media::pipeline` adds explicit SDR source/working color metadata, an
alpha-preserving linear `ColorMatrix` and `Vignette` reference, progressive
publication states with stale-result protection, tile identities/accounting and
future scheduler budgets. These are extension APIs; this phase adds no GPUI
renderer SPI or new GPU path. `ImageFilter` retains its encoded-sRGB contract.

`PipelineCapabilities::current()` reports the new GPU features as unsupported
until their respective phases land. See [media pipeline contracts](media-pipeline.md)
for the public types, examples, validation and remaining implementation work.

### Linear image sampling parameters (issue #26 Phase 2)

`CustomContent::image_linear_adjustments` is a default-None hook that forwards
through preparation geometry wrappers. `LinearImagePaint` validates finite f32
matrix/vignette parameters for encoded-sRGB source images. Metal, wgpu and D3D11
carry those per-placement parameters separately from retained source resources;
the existing `image_color_transform` method keeps its encoded-sRGB behavior.
The linear hook replaces it when present. The private compositor uniform ABI
expands consistently in Rust, MSL, WGSL and HLSL.

The extension exposes `pipeline::ImageAdjustments`, `AdjustedImage`,
`MediaImage::adjusted`/`paint_adjusted` and `MediaImageView::adjustments`.
The last filter/adjustments setter selects the interpretation explicitly. P3
working matrices and unsafe f32 conversions are rejected by the rendered path.


### Phase 3 source texture color support

`ImageTextureData::new_linear_p3_rgba16f` validates full [premultiplied ↗](https://en.wikipedia.org/wiki/Alpha_compositing),
bounded linear-P3 [binary16 ↗](https://en.wikipedia.org/wiki/Half-precision_floating-point_format) mip chains. `format()` and `bytes_per_pixel()`
drive upload size/format on Metal, wgpu and D3D11; staging accounting includes
the 8-byte pixel size. `CustomContent::image_color_mode` defaults to the old
encoded-sRGB interpretation; float preview/wide modes are explicit.
`LinearImagePaint::with_p3` declares P3 working primaries.

The native wide placement uses the existing wide-SDR compositor and requires
window opt-in. wgpu rejects wide placement; it offers an explicit sRGB preview.
Core source metadata must match the GPU texture format. Existing producers keep
their default hooks. See [media pipeline](media-pipeline.md) for examples and limits.


### Phase 4 tiled preparation and lifetime support

`ImageTextureData::page` validates a single immutable mip page. Existing
constructors still require complete whole-image mip chains.
`CustomContent::image_source_size` declares source-sized preparation independently
of a large virtual destination; geometry remains finite and clipped normally.
`image_sample_opacity` separates preparation from sampling visibility, allowing a
page to warm up before replacing its progressive fallback. Defaults preserve
existing producers.

wgpu retains source snapshots until queue completion; D3D11 event queries retire
them after sampling. Metal already retains them through command completion.
Media payload guards also follow upload completion. These lifetimes let tile
budget leases count evicted but still-used resources. Per-frame image transfer
accounting now charges aligned staging bytes instead of only unpadded payload.
See [progressive tiled canvas](media-pipeline.md) for examples and QA limits.

### Phase 5 effect reuse

`xgpui_shader::LayerEffect::paint` is public for bounded paint closures.
`xgpui_media::pipeline::ImageBlur` validates the existing layer capture/support
limits and reuses that compositor rather than adding another blur engine.
Assembled tiled previews share one kernel across page edges. This path filters
premultiplied encoded sRGB and does not provide wide-SDR or source-space blur.
`VideoFrame::adjusted` and `VideoView::adjustments` forward immutable native
preparation/ownership with linear placement uniforms; `VideoView::blur` uses
the same bounded preview path. See the media pipeline guide for errors, fallbacks,
separate effect memory and runtime QA limits. No core ABI change was needed.

## Canvas reuses existing GPUI APIs

The `xgpui-canvas` crate uses existing containers,
content masks and the xgpui-shader instanced pipeline. It adds no GPUI public
signatures or fields. Its own camera, scene, pan and grid APIs are experimental;
callers project children explicitly and own application input/selection policy.

## Supplied pixels, custom RGB and native color checks

Start with [color tasks](color-workflows.md) for practical integration. `PixelColor` distinguishes named `ImageColor`, custom `RgbDefinition` and extended-linear source samples. `MediaLoader::load_pixel_buffer` accepts that resolved interpretation, an explicit storage space and a mapping policy. Storage can be bounded P3 or Rec.2020 RGBA16F; native-wide placement converts it into the compositor's extended coordinates.

`RgbDefinition` supports reference-white adaptation and independent channel curves. `from_relative_xyz_matrix` preserves relative colorants whose sum differs from the declared XYZ reference white. The optional `xgpui-color/icc` parser constructs definitions from a bounded RGB matrix/TRC subset; it does not add image-file decoding or a general ICC transform engine. See [pixels + metadata](pixels-and-metadata.md) and [ICC scope](icc-profiles.md).

The Metal test-support API `Window::render_to_linear_p3` reads FP16 composition values before OS display mapping. The ordinary 8-bit screenshot remains an sRGB preview. [Color labs](color-labs.md) separates CPU conversion, GPU composition, browser preview and physical display claims.
