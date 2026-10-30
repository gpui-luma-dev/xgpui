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
  objc2, with typed Metal handles and explicit resource ownership.
  [Why it changed and what to watch](WHATS_NEW.md#apple-bindings-why-what-and-impact).
- **Rendering fixes.** CoreVideo caches now stay alive until pending texture
  work finishes. Empty fractional clips stay empty, fixing rejected typography
  frames caused by stray one-pixel strips.
- **Simpler local development.** Build outputs and staged website files now
  live under `target/`. The canvas guide is shorter, and local docs can still
  be served with Python. [Local development](#local-development).

## Also recently added

[Selectable kinetic typography](#typography), [media filters and tiled images](#media-pipeline),
and a [live Hermite mesh editor](examples/extensions/?demo=hermite).

## Apple bindings: why, what and impact

**Why.** The legacy Cocoa/metal-rs dependency paths brought in `block 0.1.6`,
which triggers a Rust future-incompatibility warning. Moving to the maintained
objc2 framework bindings removes those paths from normal macOS rendering and
windowing, and makes Objective-C object ownership explicit in Rust types.

**What changed.** Metal rendering, atlas textures, custom producers and shader
caches now use objc2 Metal handles. The macOS window, event, display, clipboard
and display-link code uses objc2 AppKit/Foundation bindings. CoreVideo sampling
also retains its texture views and cache until GPU work finishes, releasing
views before the cache so queued frames can survive renderer teardown.

**Impact on applications.** Ordinary GPUI views largely keep the same APIs.
Custom Metal integrations need source changes: public device/queue fields and
texture/command handles now use objc2 types instead of metal-rs types. This is
why `xgpui-apple` and `xgpui-macos` moved to **2.0.0**, and media's exposed Apple
types moved with **xgpui-media 0.3.0**. Update related crate pins together and
review conversions at your native producer boundary.

**Watch for:**

- **Resource lifetime:** closing windows or destroying renderers with queued GPU
  work; rapid video/frame replacement; stale cached textures. Keep ownership
  explicit, and record producer work on the host's device and queue.
- **Native behavior:** keyboard shortcuts and IME, clipboard operations, focus,
  resizing, display changes and wide-gamut output. Exercise these in your app;
  successful compilation does not establish runtime correctness.
- **Optional legacy dependencies:** screen capture can still bring older bindings
  through `zed-scap`. Check enabled-feature dependency trees before assuming
  every optional path is free of `block 0.1.6`.

The release passed package verification and a registry-only macOS consumer check.
Native Windows/Linux runtime coverage and app-specific lifecycle testing remain
separate checks. See [GPUI API changes](#gpui-public-api) for integration contracts.
