# paint-lab

Native Metal / Windows D3D11 app proving that one extension producer can paint different GPUI
shapes. The three examples are a rounded background with normal GPUI children,
a rounded dashed border, and a curved path fill. Effects live in xgpui-shader;
GPUI owns coverage, clipping, ordering and opacity. The app offers Hermite mesh
and multi-stop linear, radial, conic and diamond
producers. Animation changes immutable snapshots in application state; the
renderer has no animation clock.

Run here with `just ui`. Select a gradient type in the first row (radial is the
default). Controls animate/pause the shared producer, switch to native
white coverage, set inherited opacity to 50%, and clip each example without
resizing its paint field. The default window is 1500x940 logical pixels and the
three sources are 320x240. Shadows, glyph paints and explicit shared coordinate
rectangles are not part of this milestone.

## Editor

The left panel edits the selected type; background, border, and path previews
update together. Each type keeps its own settings for the current session.

- Select a color swatch to edit a stop. Drag its position slider to move it;
  stops stay sorted and the selected stop follows when it crosses another stop.
- **Add** inserts a stop in the largest gap, interpolating its color. **Duplicate**
  makes a coincident stop for a hard edge. **Remove** keeps at least two stops;
  the maximum is 32.
- RGB sliders use 0–255 readouts; alpha uses percent. The hexadecimal color
  readout reflects the selected stop. Small −/+ buttons allow precise steps.
- Linear exposes angle; radial exposes center X/Y and radius X/Y; conic exposes
  angle and center; diamond exposes angle, center, and radius.
- Centers use 0–1 coordinates in the paint bounds. Radius sliders span 0.01–2
  times the full width/height; 0.5 reaches the corresponding edge midpoint.
- Mesh exposes nine control-point RGB colors and X/Y positions. Mesh colors are
  opaque, and its control points replace the stop/angle/center/radius model.
- Editing pauses animation at the currently displayed geometry. **Reset** restores
  only the selected type. Settings are in memory and reset when the app closes.

The editor and previews scroll independently on smaller windows. Headless runs
continue to use deterministic presets and validate rendering independently of
interactive editor state.

```sh
just                       # List recipes
just args                  # App options
just ui                    # Open radial paint examples
just linear                # Multi-stop linear gradient
just conic                 # Conic gradient
just diamond               # Diamond gradient
just mesh                  # Hermite mesh gradient
just check                 # Format check, tests, Clippy
just metal-test            # Metal API validation
just capture               # /tmp/paint-lab.png
just headless --effect conic --frames 120 # Checks plus CPU wall/readback timing
just --set profile release headless --frames 120
```

Paths resolve from the repository root. From there:
`just --justfile apps/xgpui-ext-lab/paint-lab/justfile ui`.

Editor model tests cover sorted stop edits, selection, stop limits and freezing
animation without a geometry jump. The headless checks also verify all analytic gradient geometries at known sample
positions in non-square bounds, stop clamping, hard stops, premultiplied color
interpolation with transparent stops, snapshot/resize behavior and pipeline reuse.
They compare every pixel of native and custom-painted dashed,
asymmetric rounded borders and curved path fills/strokes (tolerance 3/255), and
check transparent source alpha times inherited opacity, clipping without field
remapping, scene replay, resize, runtime serialization rejection, and producer
sharing. Equal-size examples generate one source texture and compile one shared
producer pipeline set. Reported timing includes GPU wait/readback; it is not GPU-only
time or FPS. Coverage is cached separately from the producer with a 64 MiB cap.

## Application API

```rust
use xgpui_shader::{painted, MeshGradient, WindowPaintExt};

let mesh = MeshGradient::new(grid, 18)?;
painted().rounded_lg().bg_paint(mesh.content());
painted().border_8().border_paint(mesh.content());
window.paint_path_with_paint(path, mesh.content());
```

`painted()` is an explicit extension container. Materials are set when building
the element; native hover/theme refinements do not carry custom materials.

Existing `.bg(color)` and `.border_color(color)` methods and Background's GPU
layout remain unchanged. Paint values containing producers are runtime-only and
fail serialization explicitly. See the [integration contract](../../../docs/extensions/extension-paints.md)
for architecture, compatibility impact, coordinate semantics and remaining work.

## Portable headless backend

The wgpu portable executor runs MeshGradient, analytic Gradient, SolidColor and
procedural ShaderPaint. Use `--headless --wgpu`; it validates exact-generation
pixels before readback. See [wgpu checks](../../../docs/extensions/portable-paints.md).

## Experimental user-authored WGSL

Procedural ShaderProgram/ShaderPaint now use the portable content host. Select
**User WGSL** or launch with --portable-shader. Angle drives progress; selected
stop RGB/alpha drives tint/transparency. The source uses core UniformValues and
contains no backend branches or binding declarations. Its native adapters and the
Shader/shader compatibility aliases have been removed. Metal and wgpu execute
the same source; use --headless --wgpu for the latter.

```sh
cargo run --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin paint-lab \
  --features runtime-shaders,portable-shaders -- --portable-shader
MTL_DEBUG_LAYER=1 cargo run --manifest-path apps/xgpui-ext-lab/Cargo.toml \
  --bin paint-lab --features runtime-shaders,portable-shaders -- \
  --headless --portable-shader --frames 3 --png /tmp/paint-portable-wgsl.png
```

What to look for: continuously scrub angle and tint/alpha, animate/pause, change
clip/opacity and resize. The background, border and path should remain aligned
and update smoothly without flashing or waiting for mouse release. Headless
checks cover logical/raster independence, clipping/alpha, unused uniforms and
immutable snapshots. See [procedural paints](../../../docs/extensions/portable-wgsl-v1.md)
for direct native pixel, pending-frame and dense/continuous acceptance commands.

## Pastel waves

Open the standalone pastel ribbon composition:

```sh
cargo run --locked --config .cargo/validation.toml \
  --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin paint-lab --features portable-shaders -- --pastel-waves
```

The composition uses a pale freeform background and nine overlapping translucent
mesh-gradient ribbons, with fine highlights and optional grain. **Animate** starts
gentle forward/reverse motion with eased turns, **Pause** freezes it,
**Reverse** changes direction, **Grain** toggles dithering and fine threads,
and **Reset** restores the initial still composition. A small bottom-right label
shows the render frame count, measured render FPS, and direction. FPS is not a
GPU presentation measurement. Resize the window to check
that the artwork follows its bounds. Ribbon geometry uses native path coverage
to reuse shader programs while animating. The demo lives in `src/pastel_waves.rs`.

### Visual reference attribution

The pastel waves demo was visually inspired by
[Abstract pastel wave background — pink, blue and purple gradient](https://pngtree.com/freebackground/abstract-pastel-wave-background-pink-blue-purple-gradient_15809178.html)
on PNGTree. Credit for the reference artwork belongs to its original creator.
The demo recreates the visual style with procedural Rust geometry and shaders;
the reference image is not bundled or used as a texture.

## Portable solid migration checks

The shared white/half-alpha sources in native coverage checks now use portable
`SolidColor` content. Strict validation calls the backend-neutral
`PlatformHeadlessRenderer::prepare_content_snapshot`; pending generations are
retried with a deadline outside UI drawing. Protocol-specific request/uniform
checks use explicit legacy Background fixtures so those tests remain independent
of the production SolidColor implementation. Cache assertions for the migrated
producer live in the extension's `portable_solid` acceptance target.

```sh
MTL_DEBUG_LAYER=1 cargo run --manifest-path apps/xgpui-ext-lab/Cargo.toml \
  --bin paint-lab --features runtime-shaders,portable-shaders -- \
  --headless --frames 3 --png /tmp/paint-portable-solids.png
```

What to look for: a successful JSON report covering native coverage, alpha, clip
mapping, replay, resize and source sharing; the saved image contains all three
painted shapes. Meshes now also use portable content. The complete
headless checks currently require Metal for their portable solid portions;
`--headless --wgpu` validates the same portable producers on wgpu.
Wgpu-specific legacy brush/cache tests remain available and use native fixtures.

For direct solid pixels/host-cache acceptance, run the shader extension's
`--test portable_solid` target (see its README); expect four passing tests.


## Portable analytic gradient checks

Native analytic gradients now use retained portable content and strict snapshot
readiness, including background, border and path coverage. Analytic producer
counters in JSON are null: allocation and pipeline caching now belong to the host.
The extension's `portable_gradient` test target checks those host statistics.

```sh
cargo run --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin paint-lab \
  --features runtime-shaders,portable-shaders
MTL_DEBUG_LAYER=1 cargo run --manifest-path apps/xgpui-ext-lab/Cargo.toml \
  --bin paint-lab --features runtime-shaders,portable-shaders -- \
  --headless --frames 3 --png /tmp/paint-portable-gradients.png
```

What to look for: switch among linear, radial, conic and diamond; edit colors,
angles and stops; resize repeatedly. All three painted shapes should update,
with correct clipped edges and transparent interpolation, without freezes or stale
textures. The headless command must finish successfully with its geometry,
hard-stop, alpha, replay, resize and coverage checks passing.


## Portable mesh validation

The main mesh editor and the offscreen Hermite comparison now use portable content
with retained presentation slots. Producer counters in JSON are null for meshes
and analytic gradients; native resource diagnostics live on the host.

```sh
cargo run --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin paint-lab \
  --features runtime-shaders,portable-shaders -- --effect mesh
MTL_DEBUG_LAYER=1 cargo run --manifest-path apps/xgpui-ext-lab/Cargo.toml \
  --bin paint-lab --features runtime-shaders,portable-shaders -- \
  --headless --effect mesh --frames 3 --png /tmp/paint-portable-mesh.png
```

What to look for: continuously drag point position and U/V angle/length controls;
colors and geometry should follow smoothly, without flashes or waiting for mouse
release. Animate/pause, move between points, change the Hermite editor grid and
presets, resize, and toggle clipping/opacity. Background, dashed border and curved
path must stay aligned with the same mesh field. The offscreen comparison must
update alongside the direct brush where the direct brush accepts the geometry;
folds remain supported by the tessellated producer. Headless validation must pass.
