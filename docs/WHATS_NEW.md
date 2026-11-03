# What's new

## Latest

Since the [October 31, 2026 release](https://github.com/gpui-luma-dev/xgpui/releases/tag/release-2026-10-31-ed4469267251),
XGPUI has gained an upstream framework update, more reliable Metal rendering,
retained photo viewports and smoother native fractal navigation. These changes
are included in the [November 3 release](https://github.com/gpui-luma-dev/xgpui/releases/tag/release-2026-11-03-602ab7a59442);
the new crate versions are available on crates.io.

### GPUI 1.26.2

The framework now follows **gpui-unofficial 1.26.2**. The update adds optional
debug hot reload, application lifecycle and memory-warning callbacks, native
text-selection and content notifications, back gestures and improved touch
scrolling. It also brings focus cleanup and sprite batching fixes. XGPUI’s
extensions, wide-gamut rendering and objc2/objc2-metal conversion are preserved.
([Upstream integration #123](https://github.com/gpui-luma-dev/xgpui/pull/123))

**The upstream iOS backend is included.** It lives in `gpui/crates/gpui-ios`
alongside the other framework backends, with UIKit windows, touch, keyboard and
native text input. Its implementation files are unchanged from the captured
upstream source; package wiring is adapted to XGPUI. `xgpui-ios` follows
upstream’s **1.26.2** version while that implementation remains unchanged.
Simulator compilation, linking and app packaging pass; interactive simulator
and device testing remains pending. See [iOS setup](#local-development).

The new `WindowOptions::use_display_p3` option connects to the existing macOS
wide-gamut SDR compositor. Ordinary UI colors keep their sRGB interpretation;
wide-gamut content still needs the appropriate content and window configuration.
iOS wide-SDR presentation remains unsupported.

### Metal rendering and photo viewports

**Dense custom-rendered scenes no longer need a separate pending command buffer
for every producer.** Producers encode through `MetalCustomContext::record` into
one shared preparation batch, and the host validates and submits the work.
This fixes command-buffer exhaustion during dense preparation. A supervised
512-producer regression covers replay, scene replacement and pixel correctness.
The objc2 resource ownership model is retained.
([Rendering update #121](https://github.com/gpui-luma-dev/xgpui/pull/121))

**Large photos refine while keeping an overview visible.** The media extension’s
`RasterSource` builds requested mip pages from bounded decoded pixels and RGB
metadata, using linear-light, alpha-aware filtering. A source replacement keeps
the previous overview visible as new pages become ready. `CanvasPaint` selects
wide SDR or an sRGB preview and applies existing GPU image adjustments without
rebuilding source pages or uploading them again. Admission retry also handles
workers that are still retiring. Applications retain control of decoding and
source limits; page allowances do not represent total GPU memory.

### Smoother Mandelbrot navigation

The native canvas lab now keeps a complete frame on screen and reprojects it
while a replacement prepares offscreen. Shallow views use cached Metal compute
into linear-P3 float textures on supported macOS wide-SDR hosts. Deeper views use
background CPU calculation with preview and detail refinement. Frames replace
each other as complete images, avoiding partial tile boundaries during navigation.
([Canvas update #108](https://github.com/gpui-luma-dev/xgpui/pull/108))

Run `just canvas-lab --mandelbrot`, or add `--mandelbrot-cpu` to compare the CPU
route. Native runtime validation is currently macOS arm64; the browser explorer
keeps its separate CPU implementation. See the [canvas guide](#canvas).

### Fixes and development tools

- **macOS objc2 fixes:** corrected Objective-C argument and callback types,
  native text-input/dialog handling, nullable pasteboards, Services menu wiring
  and layer background clearing. These follow the earlier bindings conversion.
- **Benchmark support:** restored the matching upstream metrics required by
  `bench-support`, with allocation, frame and CPU counters and platform benchmarks.
  These remain opt-in; the existing profiler is retained.
- **Upstream sync:** release comparisons preserve downstream policies, manifests
  and metadata and use the complete upstream source-package inventory.
- **CI coverage:** the update passed Windows, Linux and macOS library builds and
  benchmark checks, iOS simulator compilation, browser shader builds and docs
  checks. These do not establish graphical runtime support on every backend.

### Updating an application

The new core is **xgpui 2.0.0**, with **Apple/macOS 3.0.0** and matching backend
and extension versions. The [release version table and migration notes](https://github.com/gpui-luma-dev/xgpui/releases/tag/release-2026-11-03-602ab7a59442)
cover the published family.

Input matches change from `PlatformInput::ScrollWheel` to `Scroll` and gain
`BackGesture`; complete scroll/window/inset literals need the added fields.
Custom Metal integrations use `MetalCustomContext::new`, `record()` and
`queue_identity()` in place of context struct literals and public queue access.
See [GPUI API changes](#gpui-public-api).

## October 31, 2026

### Color and wide gamut, from source pixels to the display

XGPUI’s expanded color pipeline connects **sRGB, Display P3, Adobe RGB (1998)
and Rec.2020** through explicit conversion, image storage and native wide-gamut
rendering. It covers everyday UI colors, wide-gamut photographs, perceptual
color editing and custom RGB profiles. Applications choose their own image
decoder and supply **pixels plus color metadata** to the library.

- **Choose the space for the task.** sRGB remains the basis for ordinary UI and
  portable previews. Display P3 supports wide-gamut content and P3 image storage.
  Adobe RGB (1998) adds a photographic source space, while Rec.2020 provides
  wider SDR image storage that can retain Adobe RGB colors outside P3. Each
  source keeps its declared primaries and transfer function during conversion.
- **Convert, inspect and interpolate.** Move between the four named RGB spaces,
  inspect XYZ and chromaticity coordinates, and use OkLab/OkLCh for perceptual
  color operations and gradients. Explicit gamut mapping controls how colors
  fit the destination; D50/D65 white-point adaptation supports custom definitions
  and profile interpretation. See the [color math guide](#color-math).
- **Preserve wider colors.** Use Display P3 or Rec.2020 image storage deliberately,
  then retain extended color values through native composition, normal blending,
  blur and linear color filters until output mapping. Ordinary UI colors remain
  sRGB; choosing Rec.2020 storage does not enable HDR.
- **Interpret the source correctly.** Named color spaces, custom RGB definitions
  and extended linear pixels share the supplied-pixel pipeline, with explicit
  transfer curves, alpha handling and gamut mapping.
- **Use embedded RGB profiles.** The optional ICC adapter resolves a bounded
  matrix/TRC subset, including sampled and parametric curves and white-point
  adaptation. It preserves the profile’s actual values and reports unsupported
  profiles explicitly. See [supported ICC profiles](#icc-profiles).
- **Check the numbers and the rendering.** Independent high-precision references
  cover RGB conversion, white-point adaptation, profile curves, near-black values
  and alpha.
  Metal FP16 readback checks native composition before display color management,
  including colors that an ordinary sRGB screenshot cannot retain.

**Wide gamut is a capability; Display P3 is a particular color space.** Source
interpretation, image storage, window presentation and the physical display are
separate choices. The new [practical color guides](#color-workflows),
[pixels and metadata guide](#pixels-and-metadata) and
[lab walkthroughs](#color-labs) explain common tasks without requiring a color
science background. The [color overview](#crate=xgpui-color&tab=overview) includes
small examples for everyday conversions, mapping and image input.

Try the new browser examples:

- [Chromaticity and gamut](examples/extensions/?demo=chromaticity): explore gamut
  boundaries, luminance and mapping with an sRGB chart preview.
- [Pixels + metadata](examples/extensions/?demo=pixel-metadata): keep the samples
  fixed while changing their interpretation; guided lessons show when to inspect
  XYZ values and packed FP16 bytes rather than expect a large visual change.
- [Italy photo comparison](examples/extensions/?demo=italy): compare WebKit’s
  sRGB, Display P3 and highlighted photographs, or switch the selected photo
  between its browser-managed original and a Rust/Wasm sRGB preview.

The [gallery](examples/extensions/) now contains **24 examples**. The Italy
example uses browser image color management; the GPUI web canvas demos remain
sRGB previews. Native wide-gamut rendering has macOS/Metal validation;
Windows/Linux native runtime acceptance remains outstanding. Visible output
still depends on the browser or OS color management and the actual display.

## October 30, 2026

- **Spatial canvas.** Pan and zoom retained scenes with viewport culling,
  hit testing, selection, snapping and procedural grids. Try the
  [card canvas](examples/extensions/?demo=canvas) or
  [Mandelbrot explorer](examples/extensions/?demo=mandelbrot), or start with
  the [canvas guide](#canvas).
- **Spatial Wasm demos.** Explore
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
