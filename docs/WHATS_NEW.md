# What's new

## October 30, 2026

- **Spatial canvas.** Pan and zoom retained scenes with viewport culling,
  hit testing, selection, snapping and procedural grids. Try the
  [card canvas](examples/extensions/?demo=canvas) or
  [Mandelbrot explorer](examples/extensions/?demo=mandelbrot), or start with
  the [canvas guide](#canvas).
- **Two new Wasm demos.** The browser gallery now has 21 examples, including
  10k/100k-card scenes and a progressive CPU fractal explorer. Both new demos
  support WebGPU and WebGL2. [Open all demos](examples/extensions/).
- **Modern Apple bindings.** Migrated the Apple renderer and macOS backend to
  objc2 bindings, including Metal resource ownership and callback handling.
- **Rendering fixes.** CoreVideo caches now stay alive until pending texture
  work finishes. Empty fractional clips stay empty, fixing rejected typography
  frames caused by stray one-pixel strips.
- **Simpler local development.** Build outputs and staged website files now
  live under `target/`. The canvas guide is shorter, and local docs can still
  be served with Python. [Local development](#local-development).

## Also recently added

[Selectable kinetic typography](#typography), [media filters and tiled images](#media-pipeline),
and a [live Hermite mesh editor](examples/extensions/?demo=hermite).
