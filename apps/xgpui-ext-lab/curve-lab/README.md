# Curve lab

Run `just curve zeta` to open the animated Riemann zeta experiment, or select
**Zeta mapping** in the existing curve lab. The panel is also shared with the
browser gallery (`?demo=curve&panel=zeta`).

- Starts with a straight grid and deforms it into ζ(z) over ten seconds.
  Then the mapped grid fades into the background while the colored critical-line
  image `ζ(½ + it)` for `0 ≤ t ≤ 120` draws on top over thirty more seconds.
  Pause freezes either stage.
- Drag the timeline to inspect any intermediate state. Grid and ζ(z) select
  the grid states; Trace end selects the completed line. Play at the endpoint
  restarts the complete sequence. The first half of the timeline is the grid
  deformation; the second half is the overlaid trace.
- On macOS, three-finger vertical movement zooms at the cursor: up zooms in,
  down zooms out. Two-finger scroll pans; pinch is disabled on this panel.
  Wheel zoom, drag-to-pan, and Reset view remain available. Other platforms
  retain pinch and scroll navigation.
- macOS must deliver the contacts to the app. If three-finger movement invokes
  Mission Control or another system gesture, that system assignment can consume
  the gesture. The lab does not change system preferences.
- Tolerance cycles through 0.2, 0.6 and 1.5 pixel simplification thresholds.
- Sample points displays the retained polyline vertices.
- Source line and Trace thickness buttons independently set their screen-space
  stroke widths. Defaults are 1 px for the inset and 1.25 px for the trace.
- **Bézier trace** (on by default) uses the prototype `fit_parameterized` API,
  with 0.002 model-unit tolerance and a 4096-segment budget. Fitting and
  measurement are cached when σ changes. The crate reports maximum/RMS paired
  sample error and whether it met tolerance or exhausted the segment budget.
- **Constant speed** (on by default) advances by fitted arc length instead of
  source t. Toggle it to compare the two motions while preserving the source
  parameter. Each fitted segment retains its original source-t interval;
  segment arc lengths and `MeasuredPath::locate` map between source t and path
  distance, without the former approximate sampled lookup. Numerical arc-length
  inversion still has finite accuracy. The source marker and zero pulses use
  the recovered source t.
- **Compare samples** overlays the original polyline in white with sparse
  sample markers. Turn Bézier trace off to draw the original sampled trace.
- **Tangent & curvature** shows a cyan tangent and a violet osculating circle
  through `MeasuredPath::geometry_at`, without maintaining a separate segment
  list. The readout gives signed curvature and
  radius in model units; nearly flat or extremely large circles are omitted.
  Curvature is local to the fitted segment and may change at fit boundaries.
  Stationary points with undefined curvature are identified explicitly.
- Tip guides draws orthogonal projections from the mapped tip to both axes,
  with a small right-angle corner marker; toggle it off for an uncluttered view.
- The source-plane inset shows `s = σ + it`. Drag its vertical line left/right
  to vary σ between 0.1 and 0.9 in 0.025 increments while preserving the current
  t. **Critical line σ = ½** restores the original line. Matching yellow markers
  identify the source point and the mapped drawing tip.
- The mapped origin pulses after a numerically detected zero crossing. Detection
  refines local minima of |ζ| and requires a residual below 1e-8; near misses
  away from the critical line are not marked. The zero count covers the full
  selected trace, including the portion not yet drawn. Pulse expansion respects
  reduced-motion preferences; scrubbing also controls the pulse state.

The experiment caches 513 evaluations per wire on a rectangular source domain
`[-3, 5] × [-3, 3]`, with grid spacing 0.25. Each frame linearly interpolates
between source and mapped points, simplifies in screen space using the curve
crate's Ramer–Douglas–Peucker implementation, clips, and paints native paths.
An open disk of radius 0.12 around the pole at `z = 1` is omitted, splitting
paths rather than drawing across the singularity.

The lab-local evaluator uses Euler–Maclaurin summation with 24 as the grid tail
boundary (at least 64, increasing with |t| for the trace) and six Bernoulli corrections. This is a bounded visual experiment,
not an arbitrary-precision zeta calculator. The simplification tolerance bounds
error relative to the sampled polyline, not the continuous mathematical curve;
zooming can reveal the fixed source sampling limit. Adaptive function sampling
and temporal refinement stability are potential follow-up experiments.

Run `just curve test` for numerical reference checks, finite intermediate
geometry checks, navigation tests and panel selection tests. Parameter-preserving
fitting and diagnostics are additive extension API prototypes documented in the
curve crate README. They need no core API changes. The earlier optional macOS
three-finger navigation adds the core APIs described below.


## Core API review: three-finger navigation (approved 2026-11-23)

1. `Window::set_three_finger_zoom(enabled: bool)` opts a window into macOS
   three-finger vertical-drag zoom, delivered to existing `on_pinch` handlers.
   Pinch is suppressed while enabled; two-finger scrolling still pans. The lab
   enables it only on the Zeta panel and disables it when switching panels.
   This is additive, changes no serialized data, and requires no migration.
   Existing windows keep their behavior unless they opt in. Other platforms
   ignore it. It belongs at the native-input boundary: an extension can choose
   when to enable it but cannot safely own GPUIView's native touch callbacks.
2. `PlatformWindow::set_three_finger_zoom(enabled: bool)` is the backend hook.
   Its default implementation is a no-op, preserving source compatibility for
   existing backend implementers. It changes no serialized data and requires
   no migration. An extension cannot implement native input routing for an
   already-created platform window through the existing public interface.
3. `PinchEvent` keeps its existing fields and serialization behavior. Its zoom
   delta may now represent a three-finger drag on explicitly opted-in macOS
   windows. Existing handlers need no migration. This reuses the current zoom
   dispatch and hit testing rather than adding a parallel event/listener API.

Verification: curve-lab tests plus the macOS recognizer's pure state tests cover
finger counts, direction, rebasing, and suppression of scroll tails. Real
three-finger trackpad delivery has not been verified on hardware. Windows and
Linux retain default behavior; they have not been tested locally.

AppKit reference: [Handling Trackpad Events](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/EventOverview/HandlingTouchEvents/HandlingTouchEvents.html).

## Review tasks

`just curve zeta-capture` writes six fixed-state native screenshots plus separate
CPU draw and GPU render/readback timings. `just curve zeta-benchmark` writes a
CPU-only evaluation/fitting/measurement/path-building baseline. Both tasks exit
when complete and put results under `target/zeta-review/`.

See [review/README.md](review/README.md) for reproducible commands, measured
limitations and initial results, and [API_REVIEW.md](API_REVIEW.md) for the
individual public API decisions approved by the user on 2026-11-23.
