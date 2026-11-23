# Issue 430: public API review

Status: **approved by the user on 2026-11-23** following review of draft PR #431.
Approval covers the API designs and semantics documented here, through code
commit `37528a3`. Further API/behavior changes require review. This records API
approval only; it does not authorize a pull-request merge or release.

## Non-extension GPUI changes (review individually)

### 1. `Window::set_three_finger_zoom(enabled: bool)`

Purpose: opt a window into macOS three-finger vertical movement as zoom instead
of pinch. The zeta panel enables it; changing panels disables it. Zoom is anchored
at the cursor when three contacts begin. Two-finger scrolling retains pan.

Source compatibility: additive method; existing windows remain opt-out.
Serialization: no fields or serialized formats change.
Migration: none for existing applications. Other platforms ignore the setting.

Extension alternative: gesture policy can live in an extension, but the current
public interface does not give an extension safe ownership of GPUIView's native
touch callbacks. A more general input bridge could permit this later; review
whether a narrowly named window method is appropriate before accepting it.

- [x] Approved: this opt-in window API.

### 2. `PlatformWindow::set_three_finger_zoom(enabled: bool)`

Purpose: backend hook for the window method. The macOS implementation requests
indirect/resting contacts and translates a three-contact vertical delta into zoom.

Source compatibility: default no-op method; backend implementers need no changes.
Serialization: no changes. Migration: none; unsupported backends keep their input.

Extension alternative: the native backend currently owns the view and dispatch
callback. An extension cannot provide that hook through the existing interface.
This is the same architectural decision as item 1, but a separate public trait
addition that needs review.

- [x] Approved: the default no-op backend hook.

### 3. Opt-in `PinchEvent` semantics

Purpose: reuse existing magnification dispatch/hit testing for synthesized
three-finger zoom. Event fields and the `on_pinch` listener APIs do not change.
A caller opting in receives magnification deltas from three-finger motion;
native pinch delivery is suppressed while enabled. The mode is window-wide.

Source/serialization compatibility: unchanged type shape and formats. Behavior
changes only for opted-in windows. Migration: listeners can retain existing zoom
math, but must not assume that every event came from a physical pinch.

Extension alternative: a distinct trackpad event would preserve the literal
meaning of "pinch" but require a larger event/listener API. Review that tradeoff,
OS gesture conflicts, and whether window-wide opt-in is the right scope.

- [x] Approved: opt-in three-finger magnification through `PinchEvent`.

## Extension additions

These belong in `xgpui-curve` and need no additional core APIs. All are prototypes
under existing `fit` / `measure` features. There are no new dependencies, no
serialization changes, and no migration for existing callers.

| Addition | Purpose / contract |
| --- | --- |
| `fit::fit_parameterized` | Fit ordered samples at caller-supplied source parameters, without geometric reparameterization or simplification. |
| `ParameterizedFitOptions` | Sample tolerance and maximum segment budget (1–8192); non-exhaustive configuration. |
| `ParameterizedSegment` | Cubic and its source interval; non-exhaustive result. |
| `FitDiagnostics` | Input/segment counts, paired maximum/RMS error, tolerance status, and segment-limit exhaustion; non-exhaustive result. |
| `ParameterizedFit::path`, `segments`, `diagnostics` | Immutable views of consistent fit data. |
| `ParameterizedFit::segment_at`, `point_at`, `parameter_at` | Bidirectional conversion between source parameter and segment-local parameter; finite queries clamp, invalid queries return `None`. |
| `measure::PathGeometry` | Location/tangent plus normal and optional signed curvature; non-exhaustive result. |
| `MeasuredPath::geometry_at` | One measured lookup for differential geometry; finite distances clamp. Undefined stationary curvature is `None`, distinct from regular straight curvature `Some(0.)`. |

Important limits: fit errors are paired **sample** errors, not continuous-function
or derivative guarantees. Budget-limited fits must be checked before use.
Source-preserving fitting can need more segments than a geometric fit. Joins
share estimated tangents where regular but do not guarantee continuous curvature.
Measured geometry uses path units and the existing segment-boundary selection;
it does not smooth corners or implicitly transform curvature to screen space.

Existing `FitOptions`, `fit_cubic_path`, `Location`, `locate`, and `point_at`
contracts remain unchanged. `CurveError::NotIncreasing` documentation now also
covers source parameters; the variant and its existing display text are unchanged.

- [x] Approved: parameter-preserving fit API and its budget/error contract.
- [x] Approved: measured geometry's degeneracy and join semantics.
