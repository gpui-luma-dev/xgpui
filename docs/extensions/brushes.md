# Main-pass brushes

Issue #22's first milestone adds `xgpui_shader::brush` under the existing
`portable-shaders` feature. `Brush` supports solid, linear, and elliptical radial
fills on explicit `Painted` backgrounds and solid-style borders. It uses the
existing instanced main-pass shader pipeline on Metal, D3D11, and wgpu. It does
not allocate an intermediate color texture or a geometry mask texture, and does
not add a render-pass boundary.

```rust
use gpui::{prelude::*, *};
use xgpui_shader::{Brush, BrushStop, painted};

let fill = Brush::linear(45., vec![
    BrushStop::new(0., [0.1, 0.3, 1., 1.]),
    BrushStop::new(0.5, [0.7, 0.2, 0.9, 0.6]),
    BrushStop::new(1., [1., 0.5, 0.1, 1.]),
])?;
let border = Brush::solid([1., 1., 1., 0.8])?;
let card = painted()
    .w(px(240.)).h(px(160.))
    .rounded(px(24.)).border_2()
    .bg_brush(fill.clone())
    .border_brush(border)
    .child("Ordinary GPUI children");
```

`Brush::radial([0.5, 0.5], [0.5, 0.5], stops)` centers an ellipse whose radii
reach the edge midpoints. `Brush::solid`, `linear` and `radial` return validated,
immutable values. Clone and retain them in view state. Unmodulated brushes share one
compiled program, including independently constructed brushes of different
kinds, so adjacent brush instances can batch together. Procedural variants are
described below. Native primitives and
other shader programs retain their original ordering boundaries.

## Coordinates and color

Coordinates cover the **full unclipped outer rectangle**, including for borders.
Clipping reveals the original field; it does not rescale the gradient. Linear
angles are clockwise from up and use logical pixel distances, preserving their
direction on non-square receivers. The gradient spans the extrema of the
rectangle's projection onto that direction. Radial centers/radii are normalized
to the full rectangle, independently on each axis. DPI affects analytic coverage,
not the brush's logical coordinate space.

Colors are finite straight-alpha **encoded-sRGB SDR** values in `[0,1]`.
Interpolation uses premultiplied encoded sRGB, then returns straight alpha to the
host. Transparent stops cannot contribute hidden RGB to neighboring colors.
Inherited opacity and final alpha association remain host responsibilities. This
API does not introduce a new linear-light, wide-gamut, or HDR color contract.

Gradients accept **two or three sorted stops** with offsets in `[0,1]`. Duplicate
offsets make hard edges; the last color wins at the exact shared offset. Colors
clamp outside the endpoint stops. The three-stop limit fits brush colors,
coordinates, independent corner radii and independent border widths into the
existing 128-byte portable instance ABI. Larger gradients continue to use
`Gradient` through `bg_paint` / `border_paint`.

## Geometry and surface behavior

`bg_brush` and `border_brush` override earlier explicit paint/brush setters for
the same surface slot; a later `bg_paint` or `border_paint` overrides them in turn.
As with existing custom paints, ordinary GPUI style refinements do not carry
extension materials. Rebuild the surface to change a brush.

Rounded fills support independent corner radii. Solid-style border rings support
independent widths, zero-width sides and rounded corners, with native GPUI's
coverage profile (including its nearest-ellipse approximation and inner-edge
fade). Native border widths continue to participate in layout. Backgrounds paint
before inset shadows and children; borders overlay children. Existing content
inset clipping, ancestor masks and element opacity remain active.

Low-level callers can use `brush.paint_background(window, bounds, radii)` and
`brush.paint_border(window, &quad)`, which return validation errors for unsupported
or invalid geometry. `Painted::border_brush` requires a solid border style and
rejects a dashed style when the element is built. Use `border_paint` for dashed
borders. Brushes do not currently implement `CustomContent` or convert into
`Paint`, so they cannot accidentally fall back to an offscreen producer path.
Existing native styles and `Paint` serialization are unchanged.

## Demo and verification

```sh
cargo run --locked --config .cargo/validation.toml \
  --manifest-path apps/paint-lab/Cargo.toml --features portable-shaders

cargo test --locked --config .cargo/validation.toml \
  --manifest-path extensions/xgpui-ext/crates/xgpui-shader/Cargo.toml \
  --features wgpu,portable-shaders --lib brush::tests
```

The paint-lab gallery shows fixed solid/linear/radial cards and gradient border
rings below the existing paint previews. CPU references check gradient pixels;
full-image comparisons check native solid fill/border coverage. Tests cover
fractional DPI, hard stops, low alpha, clipping, inherited opacity, child ordering,
program sharing and scene replay. On macOS the pixel test runs wgpu/Metal and
native Metal. On Windows set `XGPUI_BRUSH_NATIVE=1` to include native D3D11/WARP
alongside wgpu/DX12. The native backend workflow sets this automatically.

All backend workflows include the brush tests in `full` and `paint-parity` scope.
Focused runs remain diagnostic. See [GPU validation](paint-validation.md) for
backend dispatch and cold/warm measurements. Windows/DX12/WARP and Linux/Vulkan
results require their actual CI runners; local Metal results do not establish
those backends' runtime correctness.

## Procedural modulation and brush-local blends

```rust
use xgpui_shader::{BlendMode, Noise};

let textured = fill.clone().with_noise(
    Noise::simplex(0.04, 3, 0.35),
    BlendMode::Overlay,
)?;
let cellular = fill.clone().with_noise(
    Noise::worley(0.04, 0.5),
    BlendMode::SoftLight,
)?;
let grain = fill.with_noise(Noise::dither(0.02), BlendMode::Overlay)?;
```

`with_noise` replaces the previous modulation rather than building an unbounded
expression tree. Simplex and Worley frequencies are cycles per logical pixel,
in `[0.0001,4]`, with one to four octaves. Octaves double frequency and halve
weight; the normalized result stays in `[0,1]`.
`Noise::worley(frequency, strength)` measures the nearest feature (F1), while
`Noise::worley_f2(frequency, strength)` measures the second nearest (F2), as
[defined by Worley](https://cedric.cnam.fr/~cubaud/PROCEDURAL/worley.pdf).
Both use the same deterministic feature points, one per unit lattice cell, and
normalize Euclidean distance by sqrt(2), clamping to `[0,1]`. F2 can saturate
at one. This is a bounded jittered lattice rather than a Poisson point process.
Both search 5x5 cells: the own cell and a suitable adjacent cell always supply
two points within sqrt(3.25), whereas cells outside this neighborhood are at
least two units away. This also corrects rare F1 misses from the previous 3x3
search. The implementation is not a diffusion solver.
Strength is in `[0,1]`; zero strength restores the original unmodulated program,
including any specialized larger freeform field.
Noise is deterministic, object-local, and unchanged by clipping. Lattice
coordinates repeat every 65,536 logical pixels to bound integer conversion.
The repeat boundary is not guaranteed seamless.

The gallery shows large grayscale **F1 · closest** and **F2 · second closest**
panels with identical scale and full strength, plus both signals over the same
colored gradient in the procedural brush section. Dense GPU pixel tests compare
both against an independent sorted 9x9-cell double-precision reference.

Modes are `Normal`, `Multiply`, `Screen`, `Overlay`, `ColorDodge`, `SoftLight`,
and `LinearBurn`. They combine procedural grayscale with the **brush's own RGB**
in encoded sRGB, followed by strength interpolation. Brush alpha is preserved;
shape coverage and inherited opacity still apply exactly once. These are local
material operations, with no destination framebuffer sampling or layer blending.

`Noise::dither` samples a deterministic 16x16 blue-noise rank tile at physical
pixel resolution, requiring frequency=1 and one octave. The tile is generated
with toroidal Gaussian void-and-cluster relaxation (sigma=1.5), following
[Ulichney's method](https://cv.ulichney.com/papers/1993-void-cluster.pdf).
Every rank 0..255 occurs once; samples are `(rank+0.5)/256`, with exact mean 0.5.
A direct DFT regression checks low-frequency power below 15% of the white-noise
expectation for both the continuous signal and coverage thresholds at
12.5%, 25%, 50%, 75%, and 87.5%. This checks spatial distribution, not perceptual
banding removal. The tile repeats seamlessly every 16 physical pixels; use small
strengths for subtle grain. It is local material modulation rather than final
framebuffer quantization. No temporal variation or texture binding is added.
Regenerate or verify the 256-byte asset with:

```sh
python3 extensions/xgpui-ext/crates/xgpui-shader/scripts/generate_blue_noise.py --check
```

The existing 128-byte instance size is retained. Frequency and strength are
instance data; signal kind, octave count and blend mode select a cached program.
There are at most 91 noise variants per base field plus its unmodulated program.
The shared noise-program cache retains at most 128 variants across all fields. Numeric edits batch
within a variant; different variants create ordinary ordered shader batches,
with no extra render pass or intermediate texture. Noise adds per-fragment ALU
work, especially Worley's 25 feature samples per octave (previously nine for F1). It has no established
frame-time guarantee; measure the actual receiver size and workload before using
many high-octave materials.

Paint-lab includes procedural cards and a seven-mode blend comparison. GPU tests
compare all signal kinds and modes to independent double-precision references,
including fractional DPI, inherited opacity and clipping. Every bounded variant
is checked through WGSL validation and Metal/HLSL translation. Runtime Windows
and Vulkan results still require their CI jobs.

## Text glyph brushes

```rust
let text = painted()
    .w(px(480.)).min_h(px(96.))
    .text_size(px(32.))
    .text_brush(Brush::linear(90., vec![
        BrushStop::new(0., [0.2, 0.4, 1., 1.]),
        BrushStop::new(1., [1., 0.2, 0.5, 1.]),
    ])?)
    .child("A continuous gradient across wrapped text ✨");
```

`text_brush` scopes a brush over descendant monochrome glyphs. The full containing
`Painted` bounds define one continuous coordinate field, including padding and
all wrapped lines. Each glyph samples that field at its raster position, rather
than restarting the gradient per glyph. Nested text-brush containers override the
scope; outside siblings keep native text color. Use a separate containing surface
when a heading or text run needs its own field. Procedural modulation is supported
with the same `with_noise` API as backgrounds and borders.

Brushed glyphs explicitly use **grayscale atlas coverage**, even when ordinary
text uses subpixel rendering. They reuse the normal shaped glyphs, font fallback
and grayscale atlas tiles. The rasterizer uses the white-text dilation policy;
coverage is sampled directly, without the native wgpu color-dependent contrast
and gamma correction. This is an explicit brushed-text rendering policy, not a
promise of pixel equality with the ordinary text pipeline. Brush alpha, atlas
coverage and inherited element opacity multiply once each. Text color (including
its alpha) is replaced by the brush; underline, strikethrough, selection/background
painting and color emoji retain their native behavior. Emoji are not tinted.

The glyphs remain main-pass instances. No intermediate text image, output texture
or extra mask texture is allocated. The shader samples the existing monochrome
atlas (Metal uses its alpha channel; wgpu/D3D use red). A glyph variant reuses the
shape-only parameter slots for brush coordinates and atlas bounds, retaining the
128-byte brush ABI and 176-byte host transport. Adjacent glyphs batch by program
**and atlas texture**, and normal scene ordering, clipping and replay remain active.
Each material variant has a cached glyph program in addition to its shape program.

Brush scope is retained for deferred children. Cached child views include the
immutable brush snapshot identity and brush bounds in their cache keys: clones
can reuse cached text, while a changed brush or coordinate field invalidates it.
Rebuild the containing surface when replacing its text brush. Low-level extension
code can implement GPUI's `GlyphBrush` hook and use `with_glyph_brush` /
`paint_glyph_shader`; the extension owns parameters and GPUI owns glyph layout,
atlas lifetime and scene recording.

The gallery includes wrapped gradient typography and procedurally textured glyphs
beside native emoji. For focused text checks:

```sh
cargo test --locked --config .cargo/validation.toml \
  --manifest-path extensions/xgpui-ext/crates/xgpui-shader/Cargo.toml \
  --features wgpu,portable-shaders --lib brush::tests::text_tests
```

Synthetic grayscale atlas tests check alpha multiplication, shared coordinates,
fractional DPI, clipping, texture batch boundaries and replay on each enabled
backend. macOS real-font tests additionally exercise wrapping, nested/deferred
scopes, cached brush replacement, native ordinary siblings and emoji.

## Freeform and regular mesh fields

```rust
use xgpui_shader::BrushPoint;

let freeform = Brush::freeform(vec![
    BrushPoint::new([0.15, 0.2], [1., 0.05, 0.08, 1.]),
    BrushPoint::new([0.85, 0.2], [0.05, 0.3, 1., 1.]),
    BrushPoint::new([0.5, 0.85], [0.05, 0.9, 0.25, 1.]),
])?;
let mesh = Brush::mesh([
    [0.05, 0.3, 1., 1.], // top-left
    [1., 0.18, 0.02, 1.], // top-right
    [0.9, 0.05, 0.7, 1.], // bottom-left
    [0., 0.8, 0.7, 1.], // bottom-right
])?;
```

Both fields use the same `bg_brush`, `border_brush`, `text_brush` and `with_noise`
APIs as the other brushes, with full unclipped normalized coordinates. They keep
the 128-byte instance payload. Solid, gradient, mesh and up-to-three-point
freeform fields share the base program and can batch together. Larger freeform
fields specialize a program; identical larger point sets share that program and
can batch, while different sets split ordered batches. No intermediate color
texture, coverage texture or pass break is introduced by analytic spatial fills.
Native vector paths retain their existing coverage pass.

Freeform accepts **one to 16 points**, with finite positions in `[0,1]`, finite
RGBA colors in `[0,1]`, and minimum normalized separation `0.0001`. One point
makes a constant field. Multiple points use normalized inverse-square-distance
weights on associated encoded-sRGB colors. At a control point its color wins
exactly; relative weights avoid an overflowing `1/distance` calculation near
points. Transparent control colors cannot leak hidden RGB. This is an IDW color
field, **not a biharmonic or physical diffusion solve**; equal point distances
blend their associated colors equally.

For four to 16 points, positions and colors are embedded as full-f32 shader
constants. This avoids quantization, texture allocations, and expanding the
portable transport. A changed point set requires shader translation and backend
pipeline compilation. Retain these immutable brushes; this path is intended for
static/reused fields rather than per-frame point animation. The specialization
cache retains the 16 most recently used point sets. Derived shape-noise, glyph,
and path program caches each retain at most 128 entries; eviction leaves live
brushes and scene programs valid. Backends retain pipelines used by the current
scene. These are entry-count caps, not a fixed byte/throughput guarantee.

The gallery has a **Larger freeform** control that cycles four, eight, and 16
points. Compare the larger field's background, border, and typography with the
three-point field beside it.

Mesh accepts **four row-major RGBA corners** and evaluates the regular 2x2
Hermite color patch directly per fragment, with zero color derivatives at the
corners. Its position patch is the unit rectangle; the endpoint basis is
`h(t)=3t²−2t³`. This reproduces the regular existing mesh's color field without
its offscreen target or tessellation. Alpha uses associated interpolation, and
coordinates outside the unit square clamp to edge colors.

`Brush::from_mesh_grid(&grid)` accepts opaque grids with two or three rows and
columns. A regular 2x2 grid with unit u/v tangents uses the inline color-only
mesh; other accepted grids use the certified Hermite inverse described below.
Grids above 3x3 and unsupported distortion return an error; `MeshGradient`
remains available for those cases.

To explore the existing distorted Hermite mesh renderer, select **Mesh** in
Paint Lab's top effect selector. The editor's nine point chips select a control
point; its **U angle/length** and **V angle/length** sliders edit that point's
geometry tangents. Angles are clockwise from right in normalized mesh space;
lengths range from zero to two (the regular 3x3 grid starts at 0.5).
**Reset selected tangents** preserves point position/color and other points.
The three legacy paint previews update together. These controls exercise
`MeshGradient`; the main-pass `Brush::mesh` gallery remains a regular color patch.

The gallery now compares each spatial field as a background, a border ring and
wrapped typography. Focused checks:

```sh
cargo test --locked --config .cargo/validation.toml \
  --manifest-path extensions/xgpui-ext/crates/xgpui-shader/Cargo.toml \
  --features wgpu,portable-shaders --lib brush::tests::spatial_tests
```

Tests compare associated colors against independent double-precision references
on enabled GPU backends, including translucent controls, noise, borders, clipping,
fractional DPI and existing text-atlas coverage. Regular opaque mesh samples are
also compared against the original 64-sample Hermite producer. Pixel tests do
not establish a throughput guarantee for many overlapping spatial materials.

## A distorted Hermite patch in the main pass

```rust
use xgpui_shader::{Brush, BrushMeshPoint};

let patch = Brush::hermite_patch([
    BrushMeshPoint::new([0., 0.], [1., 0.2], [0., 1.], [1., 0., 0., 1.]),
    BrushMeshPoint::new([1., 0.], [1., -0.2], [0., 1.], [0., 0.2, 1., 1.]),
    BrushMeshPoint::new([0., 1.], [1., 0.], [0., 1.], [0., 1., 0.3, 1.]),
    BrushMeshPoint::new([1., 1.], [1., 0.], [0., 1.], [1., 0.6, 0., 1.]),
])?;
```

Each row-major control supplies a position, U derivative, V derivative and
straight encoded-sRGB RGBA. Geometry uses the same bicubic Hermite basis and
zero mixed derivatives as the existing mesh producer. Color derivatives are
zero; RGBA interpolates in associated encoded sRGB. The receiver is normalized
mesh space, and regions outside the curved patch are transparent. This field
works with backgrounds, borders, glyphs, native paths and noise modulation.

The first direct geometry implementation deliberately requires a conservative
certificate. Positions must be finite in `[0,1]`, tangent components in `[-2,2]`,
and RGBA in `[0,1]`. The constructor converts the geometry to a bicubic Bezier
net and bounds all Jacobian components by its Bernstein derivative control
hulls. Both row sums of `abs(I-J)` must stay at or below 0.5 over the entire
patch. This bounds its inverse and excludes folds and singularities. Some valid
strongly distorted patches fail this sufficient condition; they return an error
rather than rendering an ambiguous inverse. Use the tessellated `MeshGradient`
for those and for grids larger than 3x3.

The fragment shader uses six projected inverse iterations, then six bounded
Newton steps, with determinant and residual checks. It derives patch-edge
coverage from the inverse Jacobian in physical pixels; no screen derivatives
or intermediate color/coverage textures are required. Normal brush shape,
clip, glyph/path coverage and inherited opacity still apply. Inverse evaluation
costs more ALU than the regular color-only mesh, and the curved-edge antialiasing
can differ from the legacy tessellated edge.

Full-precision geometry/color coefficients specialize the program, preserving
the 128-byte instance payload. Identical immutable patches share a program;
different patches split ordered batches. A 16-entry LRU retains patch programs;
the existing bounded noise/glyph/path caches handle derived programs. Retain
these brushes rather than recompiling edited control sets every frame.

The app section **Hermite mesh · live 2×2 / 3×3 editor** compares the offscreen
and direct renderers. The **Grid** button switches between four and nine points.
Select a point, then adjust **Point X/Y**, **U angle/length**, and **V angle/length**.
Angles are clockwise from right in normalized mesh space; length zero retains
the chosen angle. Input events coalesce into one material rebuild per rendered
frame; editing geometry recompiles the point-specialized brush.

In 3x3 mode the comparison shares geometry, colors, tangents, and selection with
the main nine-point **Mesh** editor. Editing either control set updates the same
points. Switching into this mode selects Mesh and pauses animation. Preset and
reset actions also update the main mesh. Animation uses the same current-frame
positions in both previews; larger motion may exceed the certificate.

Preset cycling provides **Regular**, **Bend U tangents**, **Bend V tangents**,
**Inset patch**, and (in 3x3 mode) **Move center point**. Reset selected restores
one point from the active preset; reset all restores the entire preset. An
unsupported edit keeps the legacy preview current and replaces the direct
preview with an explicit placeholder and reason. Border/glyph examples return
when the edit is valid again. Gray shows the region outside the mesh.

Tests compare dense interior pixels with the legacy 64-subdivision renderer,
check translucent patch-edge coverage against a CPU reference, and extend the
spatial glyph/noise/clip/DPI and native-path mask checks to distorted patches.

```sh
cargo test --offline --locked --config .cargo/validation.toml \
  --manifest-path extensions/xgpui-ext/crates/xgpui-shader/Cargo.toml \
  --features wgpu,portable-shaders --lib hermite_tests
```

### Multiple connected patches

```rust
let mesh = Brush::hermite_mesh(3, 3, points)?; // nine row-major BrushMeshPoints
```

`hermite_mesh(width, height, points)` accepts each dimension in `2..=3`, for
one, two, or four connected patches. Tangents are **patch-local** derivatives,
as in `MeshGrid`; regular 3x3 points use U `[0.5,0]` and V `[0,0.5]`. An API
call takes one shared point array, so neighbors use identical positions,
geometry tangents, and colors along each common edge.

Each patch's Bezier derivative hull is scaled to the global normalized domain
before checking `||I-J||_infinity <= 0.5`. Together with shared boundaries, this
certifies one continuous, injective global map, rather than allowing independent
patches to fold or overlap. The inverse solver selects the current patch at
each iteration and rescales its derivatives to global coordinates. Color uses
the corresponding patch's Hermite basis. Only the **outer grid boundary**
affects mesh coverage: internal edges never receive a fade or another blend.
The fragment still performs one bounded inverse, rather than one per patch.

The same 128-byte instance transport, full-precision specialization, 16-entry
Hermite program cache, and bounded derived-program caches apply. Different
geometry/color grids need different programs; this is not a dynamic large-grid
control-data buffer. Tests cover all supported dimensions, a moved shared center,
dense comparison to legacy tessellation, translucent seam pixels, and the
border/noise/glyph/path integrations. Grids above 3x3 and stronger distortion
remain outside this milestone.

## Bounded SDR backdrop blur

```rust
use xgpui_shader::{BackdropBlur, painted};

painted()
    .w(px(248.)).h(px(112.)).rounded(px(16.))
    .backdrop_blur(BackdropBlur::new(px(16.))?)
    .bg(rgba(0xffffff24))
    .child("Sharp foreground over a frosted backdrop");
```

This explicit boundary captures pixels **already painted behind the container**,
filters them, then paints the container's own background, border and children
normally. Those children remain sharp. Radius zero, zero effective opacity and
fully clipped surfaces record no effect. Base container opacity multiplies
ancestor opacity once as the filter strength; this is not isolated group opacity.
As with the other extension materials, rebuild the container to change base
styles; native hover refinements do not carry the effect configuration.

Each boundary ends the current pass, copies a bounded full-resolution crop,
runs horizontal/downsample and vertical Gaussian passes at quarter resolution,
then resumes the target with a rounded, clipped composition. Both axes use nine
bilinear taps; radius denotes finite kernel support (approximately three sigma),
not CSS blur sigma. Quarter-resolution reconstruction adds softness at small
radii. Filtering uses associated encoded-sRGB framebuffer values. Composition
mixes original and filtered RGBA directly with fixed-function blending disabled,
so transparent captures do not receive their alpha twice. It does not perform
linear-light or HDR filtering.

The renderer retains working textures and reuses captured/filtered pixels when
all preceding retained command identities and draw orders match. The cache key
also includes the viewport, crop, blur parameters, clear color, and applicable
renderer settings. Repainting any preceding command invalidates the result,
even if it is outside the crop; newly painted equivalent scenes therefore miss.
Scene replay preserves identities. An edit after the panel does not invalidate
its source. Opacity, clipping and corner rounding are composed afresh and may
reuse the same source/filter result when the crop is unchanged.

Preceding live video/surface or custom-rendered content disables result reuse.
Prefixes above 65,536 commands also refresh every frame. This deliberately
conservative cache has no pixel readbacks or probabilistic content hashes;
retained renderer record arrays must remain immutable after scene finalization.
Cache hits skip the source copy and both filter passes, but still retain the
scene-order barrier and composition pass. The same texture budget applies.

The app includes an interactive backdrop visual check at the top of the brush
gallery. **Swap stripe colors** changes both the source and frosted panel;
**Move white marker** moves a sharp foreground shape; **Blur: on/off** lets you
compare the filtered and unfiltered stripes. Run the app with:

```sh
cargo run --locked --config .cargo/validation.toml \
  --manifest-path apps/paint-lab/Cargo.toml --features portable-shaders
```

Automated tests separately assert cache hit/miss counts and compare reused
composition against a fresh render.

Current limits are deliberately explicit:

- Radius is finite and in `[0,32]` logical pixels, at most 128 device pixels.
- At most four visible backdrop boundaries per frame.
- Each full-resolution crop, including radius plus four device pixels of
  reconstruction halo, must fit in **1024×1024 device pixels** after viewport
  clipping. This cap scales with display density; it is not a logical-size cap.
- One capture plus two quarter-resolution textures per boundary retains at most
  4.5 MiB, or 18 MiB for four slots. Replacing a slot can temporarily retain an
  additional 4.5 MiB; driver allocations and in-flight GPU resources are excluded.
- BGRA8/RGBA8 unorm SDR targets only. Native wide-SDR presentation returns an
  explicit error, as do unsupported target copy usage and exceeded limits.

Metal and wgpu/Metal pixel tests cover the filter against a double-precision CPU
reference, both blur axes, filter strength, clips, rounded corners, sharp children,
live background changes, replay ordering, the element API and allocation caps.
D3D11 uses the same translated shader and a deferred command list to restore
native state. Windows code/tests cross-compile locally; D3D11 and Vulkan runtime
validation runs in the existing paint CI lanes.

The gallery includes raw blue/orange stripes beside a frosted panel with sharp
foreground text. Run it using the command at the top of this document. Focused
checks:

```sh
cargo test --locked --config .cargo/validation.toml \
  --manifest-path extensions/xgpui-ext/crates/xgpui-shader/Cargo.toml \
  --features wgpu,portable-shaders --lib brush::tests::backdrop_tests
```

## Captured group opacity

```rust
use xgpui_shader::{LayerEffect, painted};

painted()
    .w(px(176.)).h(px(112.))
    .layer_effect(LayerEffect::opacity(0.5)?)
    .child(overlapping_children);
```

`LayerEffect::opacity` accepts a finite value in `[0,1]`. It isolates this
container's background, border, shadows and ordinary descendants, then applies
opacity **once to the completed image**. An opaque blue child covering an opaque
red child remains blue in the overlap before the whole group fades. Ordinary
`.opacity(0.5)` continues to fade each primitive separately, allowing red to show
through blue. The gallery compares these two cases side by side.

Inherited ancestor opacity multiplies the group's composition strength once.
Inside the boundary, descendants start with local opacity one; their own native
`.opacity()` styles still apply per primitive. Nested group effects compose as
nested isolated images. Captured glyphs use grayscale alpha rather than subpixel
coverage, including cached views moving into or out of an isolation scope.

The capture clips overflow to the container rectangle and ancestor content mask.
Rounded backgrounds retain their shape, but the group boundary itself does not
add a rounded child mask. Deferred overlays paint later outside the capture,
following GPUI's existing overlay behavior. Layout, hit testing and events still
use normal elements. Base hidden/display-none containers omit their capture.
Zero group opacity preserves the isolation/cache boundary so retained children
can reappear correctly when opacity changes; it still uses a bounded capture.

The GPU sequence saves the bounded destination rectangle, clears that rectangle
to transparent, draws the subtree at its existing window coordinates, captures
the isolated RGBA result, and composes it over the saved destination. No CPU
readback or full-window offscreen target is used. Keeping normal coordinates
preserves native paths, glyph atlas positions, brush fields and custom paints.
The composition equation uses associated encoded-sRGB values:

```text
source = filter(captured_group) * effective_opacity
result = source + current_destination * (1 - source.alpha)
```

A retained capture skips descendant GPU draws when their immutable paint-command
identities are unchanged. Scene replay preserves these identities; repainting a
child gives it fresh identities and invalidates the capture. A cached child view
can therefore retain its result while an ancestor changes the background or the
group's opacity or saturation. Nested boundary changes invalidate enclosing group results.
Geometry, device changes and changed capture dimensions also invalidate results.
The destination is sampled afresh even on a cache hit. Use the normal GPUI view
invalidation/`cx.notify()` contract when state changes.

External custom producers and native video surfaces conservatively disable
result reuse for their enclosing groups, since their pixels may change without
new paint commands. They still reuse working allocations. Backdrop sampling
inside a captured group currently returns an explicit error: its relationship
to the external backdrop needs a separate sampling contract.

Limits are **four groups total per frame**, including nested groups; a maximum
**1024×1024 device-pixel rectangle per group** after clipping; and at most
65,536 retained descendant command identities per group. Each group retains two
full-resolution RGBA8 textures, at most 8 MiB, plus two optional blur textures
only while its radius is positive. Four groups retain at most 32 MiB without
subtree blur, or 64 MiB when all four use blur, plus at most 4 MiB of
current/retained command identities. Including four backdrop slots, retained
working texture storage is at most 50 MiB without subtree blur or 82 MiB with
all groups blurred. Replacing a group slot can transiently retain the old slot;
driver allocations and in-flight GPU references are excluded.
A frame with no layer or backdrop boundaries releases all retained effect
textures and cache identities, while preserving compiled pipelines and cumulative
statistics. Restoring an effect recaptures its pixels. Internal native coverage
subframes preserve the owning frame's effect cache.
Captures require SDR unorm presentation and framebuffer copy support. These
limits also apply at zero opacity.

Core scene integrations should record captures and descendants through the
scene insertion/replay APIs. Renderer record arrays are immutable after scene
finalization; directly mutating them bypasses retained-command invalidation.

Tests cover overlapping/translucent children, nested groups, native path pass
breaks, brush/custom interleaving, live external producers, inherited opacity,
overflow clips, real glyphs, cached child invalidation, live destinations, opacity
changes (including zero), command replay and invalid boundary/size/count cases.
Metal and wgpu/Metal run locally; native D3D11 and Vulkan pixel validation use the
existing brush CI lanes.

```sh
cargo test --locked --config .cargo/validation.toml \
  --manifest-path extensions/xgpui-ext/crates/xgpui-shader/Cargo.toml \
  --features wgpu,portable-shaders --lib brush::tests::layer_tests
```

## Saturation and grayscale on captured subtrees

```rust
// Constructors use group opacity 1.
let grayscale = LayerEffect::grayscale(1.)?;
let intensified = LayerEffect::saturation(2.)?;

// Color filtering runs before the completed group's opacity.
let muted = LayerEffect::opacity(0.7)?.with_saturation(0.35)?;
painted().w(px(176.)).h(px(112.))
    .layer_effect(muted)
    .child(colored_background_and_children);
```

Saturation must be finite and in `[0,2]`: zero is fully grayscale, one preserves
color, and two intensifies color. Grayscale strength must be finite and in
`[0,1]`, and maps to saturation `1 - strength`. `with_saturation` and
`with_grayscale` replace the previous color-filter setting; they do not append
a filter chain. Group opacity remains independently configured.

The filter sees the completed isolated subtree, including its backgrounds,
borders, text and overlaps. It runs after descendant composition and before
this group's opacity and destination composition. The outside background is not
color-filtered. Nested groups filter from the inside out, so changing an inner
filter invalidates its enclosing captured image.

For straight encoded-sRGB color `c`, the chosen grayscale value and saturation
transform are:

```text
gray = 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b
filtered_rgb = clamp(gray + saturation * (c - gray), 0, 1)
filtered_alpha = c.alpha
```

This is encoded-sRGB color processing, not linear-light luminance processing or
a promise of exact browser CSS matching. The shader applies the equivalent
matrix directly to associated RGB and clamps to `[0, alpha]`, avoiding division
near zero alpha. Filtering preserves alpha; only group opacity changes it.
Transparent pixels cannot add hidden RGB fringes.

The cache retains **unfiltered** child pixels. Changing this group's saturation,
grayscale amount or opacity only changes composition uniforms when descendant
commands remain retained. It does not allocate another texture, add a pass,
recompile the shader or force descendant drawing. Changing an inner group's
filter refreshes its parent's capture while reusing the inner raw pixels.
All existing capture limits, SDR restrictions, clipping, deferred overlay and
external-producer rules still apply.

The gallery starts with the same colored shapes and yellow text shown as
original, grayscale and switchable inversion/saturation cards. GPU tests compare filtering and
alpha against an independent double-precision straight-color reference at 1×,
1.5× and 2× scale, including translucent overlaps, transparent colors, clipping,
opacity, cache reuse and nested invalidation. Focused checks:

```sh
cargo test --locked --config .cargo/validation.toml \
  --manifest-path extensions/xgpui-ext/crates/xgpui-shader/Cargo.toml \
  --features wgpu,portable-shaders --lib brush::tests::layer_tests::filter_tests
```

## Brightness and contrast on captured subtrees

```rust
let dim = LayerEffect::brightness(0.55)?;
let stronger_contrast = LayerEffect::contrast(1.6)?;
let combined = LayerEffect::opacity(0.7)?
    .with_saturation(0.35)?
    .with_brightness(0.8)?
    .with_contrast(1.4)?;
```

Brightness and contrast each accept finite values in `[0,2]`, with one unchanged.
Brightness zero produces black RGB while preserving coverage. Contrast zero
produces straight RGB mid-gray (0.5) while preserving coverage; it does not turn
transparent areas into gray rectangles.

The processing order is fixed regardless of builder call order:

```text
completed subtree → saturation → brightness → contrast → inversion → group opacity → destination
```

Each color stage clamps independently to `[0,1]` in straight encoded-sRGB space.
After saturation, brightness multiplies RGB. Contrast then evaluates
`(rgb - 0.5) * contrast + 0.5`. These are bounded color operations, not exposure
adjustments in linear light. Setters replace their own setting and preserve the
other settings. The most recent setter for the same operation wins.

The shader performs equivalent associated-color operations, clamping to
`[0, alpha]` and centering contrast on `0.5 * alpha`. This preserves alpha and
avoids division at transparent edges. Color filtering still precedes group
opacity and only affects captured children, leaving the outside destination
unchanged. Existing SDR and capture limits continue to apply.

Brightness/contrast changes can reuse the group's raw cached pixels. Changes
inside a nested group invalidate the enclosing filtered image, while retaining
the inner raw capture when its child commands are unchanged. The operations
share the existing 96-byte composition uniforms, buffers and shader program;
they add no texture or render pass.

The gallery adds a combined brightness-0.55/contrast-1.6 card beside the existing
original, grayscale and switchable inversion/saturation cards. Together with the opacity
comparison, it stays within the four-group frame limit. Tests check each stage
against an independent straight-color CPU reference, including clamping/order,
translucency, transparent edges, fractional DPI, cache changes, nested groups,
validation and the element builders.

```sh
cargo test --locked --config .cargo/validation.toml \
  --manifest-path extensions/xgpui-ext/crates/xgpui-shader/Cargo.toml \
  --features wgpu,portable-shaders --lib brush::tests::layer_tests::filter_tests::tone_tests
```

## Completed-layer inversion

```rust
painted()
    .layer_effect(LayerEffect::invert(1.)?)
    .child(content);

let combined = LayerEffect::opacity(0.7)?
    .with_brightness(0.8)?
    .with_contrast(1.4)?
    .with_invert(0.25)?;
```

Inversion accepts finite strength in `[0,1]`: zero leaves colors unchanged,
one complements straight encoded-sRGB RGB, and 0.5 maps visible pixels to
mid-gray. It runs after saturation, brightness and contrast and before group
opacity, regardless of builder call order. Backgrounds, borders, text and
children are composed first, then inverted together. The destination outside
the group is unaffected.

The shader interpolates between associated RGB and `alpha - RGB`, preserving
alpha and keeping fully transparent areas transparent without dividing by
alpha. The parameter fits in an unused lane of the existing 96-byte uniforms;
no additional texture or pass is needed. Changing inversion can reuse the raw
capture. An inner inversion change invalidates the enclosing group's result.

In the app's **Captured color filters** section, click the **Invert 100%** button
to cycle through full inversion, 50% inversion, and saturation 2. Compare with
the original card: both colored shapes and the yellow text change together.
This reuses an existing captured card and stays within the four-group limit.

```sh
cargo test --offline --locked --config .cargo/validation.toml \
  --manifest-path extensions/xgpui-ext/crates/xgpui-shader/Cargo.toml \
  --features wgpu,portable-shaders --lib invert_tests
```

## Brushes on native vector fills and strokes

```rust
let mut builder = gpui::PathBuilder::stroke(px(7.));
builder.move_to(point(px(12.), px(20.)));
builder.curve_to(point(px(96.), px(48.)), point(px(60.), px(0.)));
let path = builder.build()?;
brush.paint_path(window, path)?; // call during paint, for example in a canvas
```

`Brush::paint_path` accepts the same tessellated `Path<Pixels>` used by native
GPUI painting. Use `PathBuilder::fill()` for filled geometry and
`PathBuilder::stroke(width)` for strokes, including the builder's joins, caps,
curves and dash options. Fill contours retain native winding/hole behavior.
Empty paths are no-ops. Nonempty inputs must contain complete triangles, at most
65,536 vertices, with finite geometry and brush bounds.

Every brush kind (including noise) evaluates over the path's **full unclipped
tessellated bounds**. For strokes, those bounds include stroke geometry rather
than just the centerline. Ancestor clipping does not shrink or restart the field.
The host applies inherited opacity once after brush alpha and path coverage.
Captured groups and their color filters can include brushed paths and reuse the
result when their retained commands remain unchanged.

Native GPUI paths already rasterize analytic/MSAA coverage into a shared path
intermediate before final composition. This implementation reuses that target
and its native coverage pass, paints opaque-white coverage there, then evaluates
the material in the main-target shader while multiplying by the resolved alpha.
It adds no brush color texture or separate custom-producer coverage texture.
**It is not a new zero-pass-break path rasterizer.** Compatible adjacent brushed paths share one coverage/composition batch when
all composite bounds are disjoint, including a one-device-pixel gutter on each
rectangle. Overlapping bounds split batches to prevent coverage leakage. Batches
are limited to 64 paths to bound pairwise overlap checks. The paint-lab path
batching panel shows 24 separate shapes sharing one planned coverage batch;
this is a scene batching count, not a GPU timing measurement.

Path programs reuse the 128-byte brush payload and 176-byte host transport.
Programs share by material variant; retained scene records own their path
geometry. Backends validate mask/program agreement, separate path and atlas
resources, and preserve native sibling ordering. Metal, wgpu and D3D11 bind the
existing path target to the host-owned mask texture slot. Native wide-SDR
presentation currently rejects brushed paths explicitly; use the SDR pipeline.

The gallery starts with a curved blue/orange fill containing a hole, beside a
noisy mesh stroke. Tests compare each field against the corresponding rectangle
field multiplied by independently rendered native path coverage at 1×, 1.5× and
2× DPI, including translucent materials, clipping, curves, holes and strokes.
Further checks cover transport/program identity, independent path batches,
replay, native sibling order, inherited opacity, group filters and invalid input.
Metal and wgpu/Metal run locally; D3D11/Vulkan runtime checks use the paint CI
lanes. Focused checks:

```sh
cargo test --locked --config .cargo/validation.toml \
  --manifest-path extensions/xgpui-ext/crates/xgpui-shader/Cargo.toml \
  --features wgpu,portable-shaders --lib brush::tests::path_tests
```

## Destination blend modes

```rust,ignore
use xgpui_shader::{BlendMode, LayerEffect, painted};

painted()
    .layer_effect(LayerEffect::blend(BlendMode::Multiply)?)
    .child(content);
let filtered_blend = LayerEffect::opacity(0.7)?
    .with_invert(0.3)?
    .with_blend_mode(BlendMode::Screen);
```

`LayerEffect::blend` and `with_blend_mode` apply Normal, Multiply, Screen,
Overlay, ColorDodge, SoftLight or LinearBurn to the completed isolated subtree
and its current saved destination. Brush noise uses the same mode names for
material-local modulation; `LayerEffect` reads actual destination pixels.
Filters run first, followed by group opacity, blending, and source-over alpha
composition. Operations use encoded sRGB. Overlay branches on destination color;
ColorDodge and SoftLight follow the separable functions in
[W3C Compositing and Blending](https://www.w3.org/TR/compositing-1/#blending).
LinearBurn uses `max(0, source + destination - 1)` on straight channels.

For associated source `s`, destination `d`, straight-color blend function `B`,
and alpha values `as`, `ad`, composition is:

```text
rgb = (1-ad)*s.rgb + (1-as)*d.rgb + as*ad*B(d.rgb/ad, s.rgb/as)
alpha = as + ad*(1-as)
```

Zero-alpha terms are handled without division. Transparent destination pixels
retain source color, transparent source pixels retain destination color, and
blending preserves the normal source-over alpha result.

This reuses the existing saved destination and raw source textures, six-float4
uniform block and composition pass on Metal, wgpu and D3D11. Blending shares the final composition pass; group blur working textures are
described below. Mode edits reuse retained raw child pixels, and destination
pixels are captured afresh even on cache hits. Nested mode edits invalidate the
containing group's captured pixels. Existing SDR, four-group, 1024-device-pixel
side and ancestor-clip limits still apply. A nested blend sees its containing
isolated group's pixels, rather than content outside that group.

Paint-lab's captured-color section has a **Blend** button that cycles all seven
modes against a yellow/cyan destination. It replaces one existing captured card
to stay within the four-group limit. Reference tests cover opaque, partial and
zero alpha, opacity, singular color-dodge inputs, nested isolation, cache reuse,
changed destinations, filter order, inherited opacity and ancestor clipping.

```sh
cargo test --locked --config .cargo/validation.toml \
  --manifest-path extensions/xgpui-ext/crates/xgpui-shader/Cargo.toml \
  --features wgpu,portable-shaders --lib destination_blend
```

## Subtree blur

```rust,ignore
let softened = LayerEffect::blur(px(8.))?;
let combined = LayerEffect::opacity(0.7)?
    .with_blur(px(8.))?
    .with_saturation(0.5)?
    .with_blend_mode(BlendMode::Screen);
```

`LayerEffect::blur` and `with_blur` accept a finite Gaussian support radius in
`[0,16]` logical pixels, bounded to 64 device pixels after scaling. Zero disables
blur. The standard deviation is `radius / 3`, floored at 0.1 device pixels for
very small radii; each axis samples the integer offsets through `ceil(radius)`.
Two full-resolution passes filter premultiplied encoded-sRGB RGBA, with an
8-bit intermediate and normalized Gaussian weights. Pixels outside the capture
are transparent, so edge coverage fades instead of repeating edge colors.

The layer's own bounds and ancestor clip define the crop. This first version
does not expand layout or add an outward blur halo, and does not include content
outside the crop. Use padding inside the captured layer if a child needs room
for its blur. Order is completed subtree → blur → saturation → brightness →
contrast → inversion → group opacity → destination blend/source-over.

Group slots retain the saved destination and raw subtree. A positive radius
allocates two additional bounded textures for horizontal scratch and the blurred
subtree; returning to zero releases those textures and their bindings while
preserving the raw capture. Re-enabling blur recreates the working textures and
reruns filtering. With four 1024×1024 BGRA8 groups, this is at most 32 MiB without
blur or 64 MiB with blur on all groups. Zero radius skips filter passes. A positive
radius adds two passes when raw source pixels or radius change; unchanged raw
pixels and radius reuse the blurred result even when opacity, color filters,
blend mode or destination change. Radius-only edits reuse raw source pixels and
rerun the filters; nested radius edits invalidate their containing raw capture.
Existing live-producer and capture limits still apply.

In paint-lab, leave **Blend: Normal** selected and click **Subtree blur** to cycle
0 → 4 → 8 → 16 → 0 pixels. Both colored shapes and text soften, while the
underlying yellow/cyan destination and neighboring cards remain sharp. Returning
to zero restores the original appearance. Blend and blur controls can be combined.

Tests compare both GPU backends against an independent Gaussian reference at
1×, 1.5× and 2×, including translucent overlap, cropped edges, opacity, filter
and destination composition, nested cache invalidation and live radius edits.

```sh
cargo test --locked --config .cargo/validation.toml \
  --manifest-path extensions/xgpui-ext/crates/xgpui-shader/Cargo.toml \
  --features wgpu,portable-shaders --lib subtree_blur
```

## Later milestones

Direct single-pass brushed paths, point sets above 16 or dynamically
transported larger point sets,
biharmonic diffusion, more general distortion and grids larger than 3x3,
backdrop sampling inside groups and further subtree filters remain follow-up work.
Group opacity, subtree blur, color filters
and destination blending use the isolated
subtree boundary and its retained raw-result cache; further filters remain
separate additions.
