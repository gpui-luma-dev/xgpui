# Follow-ups after the experimental SVG extension

This iteration is complete for experimental use: reusable element, local output
limits, progressive background rendering, shared admission/accounting, attributed
offline samples, tests and reproducible measurements. The following are deferred,
not blockers or claims of existing support:

- **Reusable browser workers:** the demo's Hybrid comparison now uses an isolated
  module worker. Integrating workers into `SvgImage` scheduling/accounting remains
  follow-up work; bounded elements/gallery still rasterize on the browser thread.
  Validate browser performance and the separate Wasm memory footprint.
- **Viewport tiling/cropped rasterization:** preserve sharp deep zoom within
  small budgets instead of rasterizing the entire artwork at reduced resolution.
  The browser demo now compares a separate `SvgSize::Viewport` raster, but this
  mode is not integrated into `SvgImage` scheduling/accounting or a tile cache.
- **Hard filter/process limits:** evaluate isolated rendering or filter-aware
  allocation controls. Current reservations cannot cap arbitrary resvg filter
  intermediates, parser allocations or whole-process footprint.
- **Graphics attribution and presentation:** distinguish actual per-resource GPU
  residency/atlas overhead from driver retention; measure presentation and
  input-to-photon latency separately from host redraw cadence.
- **Broader validation:** lower-memory and non-macOS hardware, longer repeated
  sessions, allocation-category profiling and adversarial documents. Existing
  performance evidence is a bounded M4 Max experiment, not a universal guarantee.
- **Pressure UX and scheduling policy:** evaluate preserving previews while
  membership rebalances, prioritizing visible tiles over overscan, and safe live
  pool-limit changes. Current FIFO/fair-share policy is deliberately simple.

New features such as an editable SVG DOM, scripting/animation and automatic
SVG-to-native-path conversion would require separate proposals. Remaining
copyright/provenance research for optional, excluded benchmark artwork does not
block the bundled, license-audited sample gallery.
