# Selectable typography

Issue #28, static foundation, kinetic motion and glyph styling milestones: retained, read-only text in
`xgpui_shader::typography`. Selection, clipboard content and accessibility always
refer to the authentic source string. No GPUI core or platform changes.

## Try the demo

Open the browser typography demo through
**Browser extension demos → Text → Selectable typography** (`?demo=typography`).
The same demo runs natively:

```sh
just --justfile apps/xgpui-ext-lab/shader-examples/justfile typography
```

Choose Editorial, Ligatures, Unicode or Overflow. Drag to select, double-click a
word, triple-click the line, or use the keyboard. The source inspector shows the
selected string and UTF-8 byte range. Toggle width, type size and theme; selection
survives these changes and each sample retains its own selection. Copy a selection
and paste into another app to check exact source fidelity. Browser clipboard
writes require localhost/HTTPS and browser permission; the demo reports a copy
request rather than claiming the OS write succeeded.

## Use the element

Retain the entity in your view rather than recreating it during every render:

```rust
use gpui::{prelude::*, *};
use xgpui_shader::typography::SelectableText;

// During view construction, with cx: &mut Context<MyView>:
let text = cx.new(|cx| {
    SelectableText::new("Good typography keeps its meaning.", cx)
        .expect("single-line LTR text")
});

// During render:
div()
    .w_full()
    .text_size(px(27.))
    .line_height(px(42.))
    .child(text.clone());
```

Font, color, size and line height are inherited. The element provides its own
padding, border, focus ring, clipped viewport, selection highlight and steady
caret. Long lines scroll horizontally to keep the active selection endpoint
visible. The element has no animation clock. Static and paused snapshots schedule no animation frames; the host drives changing effects.

`text()`, `selection()` and `selected_text()` expose authentic source and selection.
`selection()` returns UTF-8 byte offsets at extended-grapheme boundaries.
`select_all(cx)`, `clear_selection(cx)` and `copy_selection(cx)` support host
controls. Empty selections leave the clipboard untouched. The entity implements
`Focusable`, so a host can explicitly focus it with `window.focus`.

## Paint-only motion

```rust
use xgpui_shader::typography::KineticEffect;

// Supply immutable snapshots from your own clock or application state.
text.update(cx, |text, cx| {
    text.set_effect(KineticEffect::Typewriter { progress: 0.45 }, cx).unwrap();
});
text.update(cx, |text, cx| {
    text.set_effect(KineticEffect::Wave {
        amplitude: px(8.),
        phase: elapsed_seconds * 2.2,
        cluster_phase: 0.65,
    }, cx).unwrap();
});
```

Progress must be finite and in 0–1. Wave amplitude is bounded to 0–12 logical
pixels; both phase values must be finite. Invalid updates leave the previous
effect intact. `KineticEffect::Static` restores ordinary line painting.

Typewriter progress counts source **extended graphemes**, then emits only complete
shaped clusters. A ligature spanning several graphemes waits until its complete
source span is revealed. Combining marks, fallback glyphs sharing a cluster and
joined emoji appear together. The visible selection highlight is clipped to fully revealed clusters; selecting
unrevealed text retains its logical selection without painting a floating rectangle.
The highlight returns as the selected text is revealed. A caret in unrevealed
text is hidden too. Copy and accessibility still use the original selection.
At zero progress, the full line still takes layout
space, can be selected/copied and remains exposed to native accessibility.

Wave applies `amplitude × sin(phase + cluster_ordinal × cluster_phase)` to each
shaped cluster. Glyphs retain canonical horizontal positions, and emoji use GPUI's
color-glyph path. Vertical space is reserved above/below the original line for the
amplitude. The selection highlight and accessibility geometry follow the original
line, so selection stays stable while letters move. These bounds intentionally do
not chase each animated glyph. Keep amplitude small for comfortable interaction.

The component checks `App::reduce_motion()` and paints the complete static source
when it is enabled. The host must also stop scheduling animation frames. The demo
does this, offers its own reduced-motion preview, and initializes the web app's
preference from the browser media query at startup. Later OS/browser preference
changes need host propagation; the web demo does not install a media-query watcher.

In the demo, choose **Static / Typewriter / Scramble / Wave**. Wave starts playing, typewriter
opens paused at the current progress, and Play reveals over four seconds then
stops. Pause freezes the phase; Restart resets it; −10%/+10% scrub reveal without a
timer. Select all while only a prefix is visible, then Copy to verify that the
complete source is retained. The logical-layer panel always shows the full source
as well as the selected substring. Reduced-motion mode shows the complete source.

## Deterministic scramble

```rust
text.update(cx, |text, cx| {
    text.set_effect(KineticEffect::Scramble {
        progress: 0.45, // resolved prefix, in source graphemes
        seed: 42,
        tick: 9,       // an explicit animation frame, not wall-clock time
    }, cx).unwrap();
});
```

Resolved clusters paint their real glyphs. Each unresolved eligible shaped cluster
paints one deterministic replacement from a small `0123456789ABCDEF` alphabet,
centered in its original advance. Replacement size shrinks to fit narrow cells;
cell clipping prevents overlap with neighbours. Layout, selection, the clipboard
and native accessibility retain the complete original string throughout.

The alphabet is **shaped in the source font**, retained by font ID and size, and
reused across progress/seed/tick changes. Missing glyphs, fallback faces and color
glyphs are rejected as alphabet entries. Whitespace and emoji remain original;
mixed-face clusters and fonts with no usable alphabet conservatively retain the
original cluster too. This version does not promise to visually scramble every
Unicode script or font. The source string is never replaced with symbols.

The same progress, seed and tick reproduce the same glyph choices. Changing seed
or tick changes the unresolved portion; progress 1 restores ordinary text exactly.
Reduced motion restores the complete static source. The host owns tick cadence;
the demo advances it at 12 steps/second while resolving over four seconds, then
stops. It initially opens a paused 45% snapshot so the distinction is inspectable.
**New seed** and **Next frame** freeze playback and step a repeatable snapshot;
−10%/+10% adjust progress in the current reveal/scramble mode.

## Gradient fills, glow and shadows

Styling requires the existing `portable-shaders` feature. Basic selection and
kinetic snapshots remain available without it.

```rust
use xgpui_shader::{Brush, BrushStop};
use xgpui_shader::typography::TextShadow;

let fill = Brush::linear(0., vec![
    BrushStop::new(0., [0.25, 0.65, 1., 1.]),
    BrushStop::new(1., [0.5, 1., 0.8, 1.]),
]).unwrap();
let glow = TextShadow::glow(rgba(0x56dac8e0).into(), px(8.)).unwrap();
text.update(cx, |text, cx| {
    text.set_fill(Some(fill), cx).unwrap();
    text.set_shadow(Some(glow), cx);
});

// An offset, blurred glyph shadow instead of the centered glow:
let drop = TextShadow::new(
    rgba(0x000000b0).into(), point(px(3.), px(5.)), px(8.),
).unwrap();
text.update(cx, |text, cx| text.set_shadow(Some(drop), cx));
```

`set_fill(None, cx)` restores inherited text painting. The brush is anchored to
the complete canonical source line, so its color field does not restart on each
glyph, move with a wave, or stretch to the currently revealed prefix. The existing
brush system supplies gradient/noise coverage; this introduces no new shader or
public compositing engine. Color emoji keep their native colors.

`set_shadow(None, cx)` removes the shadow. One shadow is supported per element.
Radius is Gaussian support in 0–12 logical pixels; each offset axis is bounded to
−12–12 logical pixels. High display scales clamp filter support to the existing
64-physical-pixel limit. Radius zero paints a sharp offset copy without a blur
capture. Nonzero blur uses the bounded layer pipeline at full resolution. On macOS Metal,
glow and drop shadows also work in `WideGamutSdr`: glyph colors keep their sRGB
meaning, then captures, filtering and composition use premultiplied linear P3
RGBA16F. The window can stay in wide mode while typography effects change.
Default presentation retains the existing encoded-sRGB filter behavior.
Wide text output is split into at most four device-aligned
captures, each within the 1024-physical-pixel side limit. The four layer groups
are shared with other layer effects in the frame; keep that total budget in mind.
Footprints that need more than four captures fall back to a sharp offset shadow. This is an opt-in offscreen effect with GPU pass and
memory cost; ordinary gradient fills stay in the main pass.

Foreground and shadow share the same emitted-glyph plan, so reveal, wave and
scramble affect both together. Only monochrome glyphs enter the colored silhouette;
color emoji stay sharp and cast no shadow. Selection highlights, caret, backgrounds
and borders are never included in the silhouette. Foreground text and selection
remain sharp after the shadow is composited.

The element reserves a halo gutter for radius and offset. Enabling a shadow changes
the available text viewport and line box height, while preserving logical source
and selection. The halo remains clipped to that reserved box. Larger blur support
can make the effect softer and less intense rather than increasing opacity.

The demo opens with a cool fill and glow in Static mode. Cycle **Fill**, **Shadow**
and **Softness**, compare light/dark themes, then try each motion preset. The source
inspector remains ordinary text. Setting the fill to native and shadow to off gives
the unstyled reference. The browser canvas accessibility-bridge limitation below
still applies.

## Interaction contract

| Input | Behavior |
| --- | --- |
| Tab / Shift+Tab | Enter or leave the focusable text field |
| Click / drag | Place the caret / extend selection, including outside the viewport |
| Shift+click | Extend from the existing anchor |
| Double / triple click | Select word segment / complete line |
| Left / Right | Move one extended grapheme; collapse an existing selection |
| Shift+Left / Right | Extend or reverse selection by grapheme |
| Home / End, with optional Shift | Move to line endpoints / extend selection |
| Option+arrows on macOS, Ctrl+arrows elsewhere | Move by Unicode word boundaries |
| Cmd+A / C on macOS, Ctrl+A / C elsewhere | Select all / copy source selection |
| Escape | Collapse selection at its active endpoint |

Combining accents, joined emoji, skin tones and regional-indicator flags remain
whole when selecting. Shaped ligatures remain intact visually; interior grapheme
carets divide their advance evenly. These are approximate ligature caret positions,
not font-provided ligature caret tables. The source is neither normalized nor
replaced. There is no editing, paste, cut or IME registration.

## Accessibility

The element exposes a focusable **read-only text input**, the complete source
value, a synthetic `TextRun`, UTF-8 grapheme lengths and geometry, and anchor/focus selection
positions in extended-grapheme units. Character bounds and advances follow the same shaped caret geometry. It accepts AccessKit `SetTextSelection` actions
for its own text run and snaps endpoints to grapheme boundaries. Hidden decorative
copies are not used. Keyboard selection and clipboard logic use the same logical
selection as the accessibility metadata.

This is an accessibility implementation contract, not a claim that every screen
reader/browser pairing has been certified. The native platform adapter exposes these semantics to assistive technology.
**The current GPUI web platform has no canvas accessibility-tree bridge:** the
browser demo verifies mouse/keyboard rendering, not screen reader access to the
canvas text. Use the native demo for accessibility acceptance. VoiceOver, NVDA and browser-specific
screen reader acceptance remain manual platform checks.

## Supported scope and progress

Completed: static shaped text, mouse selection, keyboard navigation, authentic
copy, focus and accessibility metadata, cluster-aware typewriter reveal, bounded
wave motion, deterministic glyph scramble, gradient fills, glyph glow/drop shadows, reduced-motion fallback, and a shared native/browser demo.

This milestone supports **one left-to-right line**. Construction rejects tabs,
line breaks, control characters, RTL characters and explicit bidi controls rather than offering
incorrect selection geometry through the flat-LTR layout APIs. LTR Unicode and
emoji rely on the active text backend's font fallback; unavailable fonts can show
missing glyphs. Empty text is valid. A single grapheme exceeding 255 UTF-8 bytes is rejected because AccessKit character lengths use a byte-sized field.

Deferred: wrapping, bidirectional selection, editable/rich text, stacked shadows
and color-emoji shadow silhouettes. The single-line LTR RFC milestones are
implemented within the supported font and platform limits below. Existing brush
text APIs remain separate. No public compositing API or new crate is introduced.

## Validation

Focused tests cover grapheme-safe endpoints, selection reversal, Unicode word
selection, empty text, ligature caret interpolation, source-preserving accessible
text and supported-subset validation. Build the native demo and browser module,
then check actual mouse/keyboard interactions, resize, clipboard paste and the
accessibility tree. Successful builds alone do not certify screen reader behavior.

```sh
cargo test --offline --config .cargo/validation.toml \
  --manifest-path extensions/crates/xgpui-shader/Cargo.toml \
  --target-dir target --features portable-shaders,wgpu typography --lib
bash apps/xgpui-ext-lab/extensions-web/build.sh
```

Current local checks: fourteen tests pass, including selection/motion tests with real-font shaping, keyboard
dispatch, drag selection beyond the viewport, resize/overflow scrolling, exact
clipboard source and empty-selection preservation. Native macOS and browser
WebGPU/WebGL2 rendering were visually inspected; the native text value is present in the
macOS accessibility tree. Native accessibility selection was exercised on a word
and a joined emoji; both selected the correct source range. Browser clipboard
fidelity was checked through the explicit Copy button; hardware shortcut behavior
should also be checked in the target browser. Windows/Linux screen reader
acceptance remains unverified.

Rendering tests compare full reveal pixel-for-pixel with ordinary native line
painting, verify that zero reveal hides glyphs, and confirm reduced-motion fallback
restores the full source. Motion snapshots preserve canonical horizontal carets,
selection, accessible source and exact clipboard content in interaction tests.

Scramble rendering tests revisit the same seed/tick after different snapshots and
require identical pixels, different output for another seed/tick, full-resolution
parity at progress 1, and static reduced-motion parity. Prepared alphabet identities
must stay stable across these snapshots.

Styling tests compare static/fully resolved motion and reduced-motion rendering,
require real halo pixels, and verify that a hidden selected source creates neither
a highlight nor a blurred selection rectangle. The same logical source remains
available to accessibility.
