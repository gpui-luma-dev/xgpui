# #384: pinch delivery after keyboard input

Individual core GPUI public behavior change for review:

- Surface: `Interactivity::on_pinch` and the declarative `on_pinch` wrapper.
- Purpose: deliver a real pinch to the hit-tested element immediately after a
  keyboard command, without requiring an unrelated mouse move first.
- Change: use the existing hitbox-ID position/capture check that ignores the
  keyboard-only suppression of visual hover. Bubbling and propagation are unchanged.
  Input outside the hitbox remains ignored unless existing capture semantics apply.
- Source/serialization compatibility: no signatures, types, fields or serialized
  formats change. A previously suppressed callback can now run after keyboard input.
- Migration: none. Consumers relying on accidental suppression should implement
  their own explicit gesture policy instead.
- Extension alternative: cannot reliably repair an event that GPUI suppresses
  before invoking the extension's callback. Synthesizing mouse input would change
  unrelated hover/interaction state and is not the appropriate extension workaround.
- Verification: registered real-window `pinch_after_keyboard` probe fails before
  the fix (zero callbacks), passes after it (inside callback delivered and outside
  input ignored). Local macOS only; other platforms rely on integration CI.

The PR presents this core behavior separately for owner review. No release or
final public API approval is implied by implementation.
