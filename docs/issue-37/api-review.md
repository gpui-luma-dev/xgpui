# Issue 37: experimental core API review

Status: implemented for branch experimentation; final public API approval pending.
This is the concrete review inventory required by `AGENTS.md`. Approval of the
implementation does not approve these signatures for finalization or release.
No merge to the epic or `main` is authorized by this report.

## 1. Rounded input scope

Crate: `xgpui`; file: `gpui/crates/gpui/src/window.rs`.

```rust
pub fn with_rounded_hit_test_mask<R>(
    &mut self,
    bounds: Bounds<Pixels>,
    radii: Corners<Pixels>,
    f: impl FnOnce(&mut Self) -> R,
) -> anyhow::Result<R>
```

Purpose: retain exact rounded geometry for descendant input before rectangular
hitbox occlusion is applied. Prepaint only. Does not clip painting. Rejects
nonfinite/negative geometry, uses GPUI's existing per-corner clamp, intersects
nested input masks, and preserves escaped deferred behavior. Empty bounds and
zero radii follow rectangular input masking without extra shape allocation.
The normal rectangular bound semantics remain in effect at outer edges; curved
boundaries use geometric containment, never an antialias-alpha threshold.

Source impact: additive inherent method; no required trait method or new field
on `Hitbox`, `Style`, `ContentMask` or primitives. Method-name collisions with
existing downstream extension methods need consideration. No changes at existing
call sites. The paired experimental helper opts in explicitly.

Serialization/GPU ABI: none. Frame-owned immutable side metadata and a private
view cache key are changed; no persisted records or primitive layout changes.

Migration: none for existing users. Experimental users pair this with the paint
scope in both phases using the same inner geometry.

Extension alternative: convenience methods belong in `xgpui-shader`, but the
actual input scope must participate in core frame dispatch, hitbox reuse and
cached-view invalidation. A wrapper that merely swallows clicks cannot correctly
implement hover, scrolling and occlusion through excluded corners.

## 2. Rounded captured-output scope

Crate: `xgpui`; file: `gpui/crates/gpui/src/window.rs`.

```rust
pub fn with_rounded_layer_clip<R>(
    &mut self,
    bounds: Bounds<Pixels>,
    radii: Corners<Pixels>,
    program: Arc<SceneShaderProgram>,
    f: impl FnOnce(&mut Self) -> R,
) -> anyhow::Result<R>
```

Purpose: record a bounded subtree and clip its completed premultiplied output to
circular per-corner radii. Paint only. Uses the existing capture/opacity rules,
conservative rectangular bounds and private per-capture shape metadata. Nested
captures involving this mode are rejected. The extension supplies its shared
composition program; callers cannot use an arbitrary legacy shader and expect
the new protocol to work. This is an explicit experimental integration hook,
not the final generic overflow API.

Source impact: additive inherent method. Existing `with_layer_color_filter`,
`with_content_mask`, `ContentMask`, `LayerCapture`, `Hitbox`, `Style`, primitive
records and custom producer traits retain their signatures/layouts. No migration
for ordinary overflow/effects. Renderer error/size limits remain those of bounded
capture (four groups, 1024 device pixels per side).

Serialization/GPU ABI: no record-size, alignment or binding-layout change. The
composition parameter protocol described below is extended. Metadata is retained
through scene replay; previous-frame GPU IDs are not stored.

Migration: experimental callers supply a compatible shared compositor program and
pair prepaint with the input scope. `RoundedOverflow` handles that pairing. No
new obligations for legacy `CustomContent` implementations.

Extension alternative: the convenience helper is in the extension; recording
private scene metadata and preserving it through core replay requires a core
hook. A general builder on `Styled` is deliberately deferred, avoiding a public
style-field/serialization change in this milestone.

## 3. Existing LayerPlan parameter protocol extension

Crate: `xgpui`; implementation: `gpui/crates/gpui/src/scene.rs`.
Consumer: `extensions/crates/xgpui-shader/src/backdrop.wgsl`.

The Rust signature stays `LayerPlan::parameters: [[[f32; 4]; 6]; 4]`. For the
composition pass only (`parameters[1]`), a negative saturation tag in `[4][3]`
selects rounded coverage. Slot `[2]` then carries original shape bounds xywh,
and `[5]` carries circular radii TL/TR/BR/BL. The vertex shader uses the entire
capture's source UVs in that mode. Rectangular final output bounds in `[3]` and
group opacity in `[4][2]` retain their meaning. Ordinary clear/filter/backdrop
passes and all legacy records retain their prior interpretation.

Compatibility: unchanged 96-byte stride and bindings on Metal, wgpu and D3D11;
new rounded-mode producers and the shared composition shader must be updated
together. An old shader cannot interpret the tag correctly. No serialized data
migration; no change to primitive transport or generated instanced shader ABI.
This is a real protocol extension despite unchanged buffer size and is included
for explicit review. A dedicated typed capture-composition descriptor would be
clearer for a final API; this bounded experiment reuses the existing protocol.

## Extension surface (not core API)

`xgpui_shader::RoundedOverflow::{new, set_radii, prepaint, paint}` provides the
experimental pairing. It is available under `portable-shaders`. It accepts inner
circular radii and leaves uniform border inset resolution to the caller. It does
not promise arbitrary paths, mixed-axis rounded overflow or nested effect support.

## Before finalization

Review the two method signatures independently and the parameter protocol
extension. Decide whether to keep these low-level hooks, feature-gate them, or
replace them with a combined scope before any final public API/release commitment.
Production performance, nesting/fallback design and the final styled builder
remain separate work under issue #37.
