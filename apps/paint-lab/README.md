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
resizing its paint field. The default window is 1200x700 logical pixels and the
three sources are 320x240. Shadows, glyph paints and explicit shared coordinate
rectangles are not part of this milestone.

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

The headless checks also verify all analytic gradient geometries at known sample
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
let mesh = Arc::new(MeshGradient::new(grid, 18)?);
div().rounded_lg().bg_paint(mesh.clone());
div().border_8().border_paint(mesh.clone());
window.paint_path_with_paint(path, mesh.clone());
```

Existing `.bg(color)` and `.border_color(color)` methods and Background's GPU
layout remain unchanged. Paint values containing producers are runtime-only and
fail serialization explicitly. See the [integration contract](../../extensions/xgpui-ext/docs/extension-paints.md)
for architecture, compatibility impact, coordinate semantics and remaining work.
