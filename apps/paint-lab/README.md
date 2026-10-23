# paint-lab

Local Metal app proving that one extension producer can paint different GPUI
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
`just --justfile apps/paint-lab/justfile ui`.

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

let mesh = Arc::new(MeshGradient::new(grid, 18)?);
painted().rounded_lg().bg_paint(mesh.clone());
painted().border_8().border_paint(mesh.clone());
window.paint_path_with_paint(path, mesh.clone());
```

`painted()` is an explicit extension container. Materials are set when building
the element; native hover/theme refinements do not carry custom materials.

Existing `.bg(color)` and `.border_color(color)` methods and Background's GPU
layout remain unchanged. Paint values containing producers are runtime-only and
fail serialization explicitly. See the [integration contract](../../docs/roadmap/extensions/extension-paints.md)
for architecture, compatibility impact, coordinate semantics and remaining work.

## Portable headless backend

With the opt-in `wgpu` feature, `just wgpu-test` runs the same coverage and gradient
reference checks through wgpu. `just wgpu-headless --effect mesh --frames 60`
renders the portable mesh; add `--png /tmp/wgpu-mesh.png` for a capture.
`WGPU_BACKEND=vulkan` or `dx12` forces a platform backend; on this Mac the local
portable tests use wgpu/Metal. Interactive windows use native Metal on macOS and D3D11 on Windows.
See [portable paint status](../../extensions/xgpui-ext/docs/portable-paints.md)
for validation limits. The [native Windows D3D11 adapter](../../extensions/xgpui-ext/docs/native-dx11-paints.md)
uses the same app API; native headless checks use WARP on Windows.

## Experimental user-authored WGSL

`just portable-test` exercises one user shader through native Metal and wgpu.
`just portable-headless --frames 2 --png /tmp/portable-metal.png` and
`just portable-wgpu --frames 2 --png /tmp/portable-wgpu.png` render the same shader
into the three paint sites. The app authoring example in `src/portable.rs` contains
no OS/backend branches or binding declarations. The interactive editor continues
to edit built-in gradients; the user WGSL experiment is headless only.
See [the experimental v1 contract](../../extensions/xgpui-ext/docs/portable-wgsl-v1.md).
