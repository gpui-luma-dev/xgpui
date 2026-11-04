# Selectable typography

Issue #28, static foundation, kinetic motion and glyph styling milestones: retained, read-only text in
`xgpui_shader::typography`. Selection, clipboard content and accessibility always
refer to the authentic source string. The component uses public GPUI APIs;
native acceptance also identified the macOS accessibility adapter fix recorded below.

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
cargo test --offline --locked --config .cargo/validation.toml \
  --manifest-path extensions/crates/xgpui-shader/Cargo.toml \
  --target-dir target --features portable-shaders,wgpu typography --lib
bash apps/xgpui-ext-lab/extensions-web/build.sh
```

Historical local checks: fourteen tests passed, including selection/motion tests with real-font shaping, keyboard
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

### Acceptance baseline — 2026-11-04

Tested source revision: `4f05c321ac4b6773eb7349834cc4cd33132e838d`, on
macOS 27.2 (26B123), arm64. The focused command above passed **15 tests**,
with zero failures or ignored tests. The native demo build also passed:

```sh
just --justfile apps/xgpui-ext-lab/shader-examples/justfile build
```

The secondary browser build, `bash apps/xgpui-ext-lab/extensions-web/build.sh`,
also passed and generated `target/web/extensions`. Browser runtime interaction
was not rerun. The standalone shader lockfile now records the validation config's
unused `async-process` patch; no resolved dependency versions changed.

The initial sandboxed attempts failed because Apple's Metal compiler could not
write its module cache. Both commands passed when rerun with cache access;
this was an execution-environment restriction, not a typography failure.

The suite covers source validation, grapheme selection, accessible source metadata,
real shaping, dispatched keyboard input, test clipboard contents, drag/overflow,
motion snapshots, styling and Metal wide-gamut shadow rendering. The interaction
test uses GPUI's test platform and bundled-font Cosmic Text shaping. Pixel tests
use headless WGPU and Metal with Apple shaping; they are gated to macOS. These
results do not establish desktop clipboard integration, screen-reader behavior,
physical display appearance or sustained resource behavior.

| Acceptance area | Windows | Linux | macOS |
| --- | --- | --- | --- |
| Native build / focused tests | Owner reported native labs and `just lab-test` passing through `efe173d`; standalone typography suite unverified | Unverified | Native shader demo build and 15 focused tests passed at revision above |
| Desktop interaction / exact OS clipboard | Unverified | Unverified | Owner reported native demo working on 2026-11-04; detailed interaction/clipboard coverage not recorded |
| Rendering / scale / reduced motion | Unverified | Unverified | Retina visual and headless pixel checks passed; simulated 1x/1.5x/2x/3x logical geometry passed; other physical scales unavailable |
| Assistive-technology reading / focus / selection | Unverified | Unverified | Owner confirmed VoiceOver focus/reading and complete mouse selection read via VO-Fn-F6; native Unicode and hidden-source selection actions passed |
| Repeated-animation resource behavior | Unverified | Unverified | Offscreen Metal allocations stable; 180s native wave/glow + 30s paused observation showed no continuing RSS growth and stopped animation redraws |

Windows, Linux and macOS are equal acceptance targets. A missing host is a
verification gap. The Windows lab report is recorded in issue #28's November 4
update; it does not certify the standalone component suite or assistive technology.

After launching the native typography recipe on this Mac, the owner reported
"tested working" on 2026-11-04. This records a successful manual demo smoke check;
individual cases, VoiceOver, display scales and resource measurements were not
specified and remain open acceptance items.

The subsequent VoiceOver check reported that only a button could be focused and
Tab did nothing. Investigation found missing demo Tab traversal and a retained
text focus handle without its tab-stop flag. Both are now fixed, with a regression
test that dispatches Tab/Shift+Tab from startup, reaches the text field and checks
navigation in both directions. The owner's native/VoiceOver retest remains pending;
the earlier smoke-check report does not establish accessibility acceptance.

The owner confirmed Tab navigation now works, but VoiceOver still did not reach
or read the message. Native macOS inspection reproduced a focus discrepancy:
the field had keyboard focus while `AXFocusedUIElement` reported the window.
The macOS adapter was attached to the content-view container rather than the
first-responder `GPUIView`. It now attaches to that responder and seeds its current
key-window state. Rebuilt native inspection reports the text field with the full
source as focused, Tab reports the following button, and Shift+Tab returns to the
field with the selected source exposed. These checks used a temporary app bundle
for native inspection. Actual VoiceOver speech/navigation acceptance is still
pending the owner's retest. This is a demonstrated platform-adapter fix, not a
change to the typography shaping/painting architecture.

After the adapter fix, the 17 macOS platform unit tests and the demo's Tab/Shift+Tab
regression test passed, and the native demo rebuilt successfully. The demo root
also has an explicit application role and label for its initial focus target.

The owner subsequently confirmed VoiceOver was working, with an outstanding
report that fast mouse selection could omit the first characters in its display.
A regression reproduced a matching source-selection defect: a right-to-left drag
released at the start of the line retained the last mouse-move endpoint instead,
omitting the prefix. Mouse release now commits its final coordinate and notifies
observers, preserving stationary double/triple-click selections. Tests cover fast
reverse drags, a forward drag without intermediate moves, exact copied source and
stationary word/line selection. All 15 focused tests and native/browser builds
passed afterward. The owner's fast-drag VoiceOver retest is pending; this does not
establish that VoiceOver's display has no additional timing behavior.

The owner clarified that the demo's mouse selection is correct; only VoiceOver's
caption shows ending fragments, while Cmd+A is announced completely. Further
native inspection of the rebuilt demo confirmed that macOS exposes the complete
selected sentence after full mouse drags in both directions, and the exact selected
substring after a partial drag. No further selection-code change was made for this
caption-only report. The earlier release-coordinate fix remains covered by its
separate reproduced regression.

Apple documents that the [caption panel displays current speech](https://support.apple.com/guide/voiceover/unac078/mac),
and provides a separate [read-current-selection command](https://support.apple.com/guide/voiceover/vo2718/mac):
VO-Fn-F6 (Control+Option+Fn+F6 with the default VO modifiers). After a mouse drag,
use that command to check the complete selection rather than using the final
caption fragment as a selection-range assertion. The owner confirmed on 2026-11-04
that this command speaks the entire mouse selection. Slow dragging announces each
character, while fast dragging interrupts the announcements. The reported caption/
speech fragments therefore do not indicate a truncated final accessibility selection;
no additional code change is needed for that report. This completes the reported
mouse-selection reading check, not every Unicode, motion or accessibility-action
case in the native acceptance procedure.

### Remaining macOS checks — 2026-11-04

- Native accessibility selection actions selected `é` and `👩🏽‍💻` exactly,
  and selected the complete Unicode sample with typewriter reveal at 0%.
  The macOS accessibility value retained the complete source while its glyphs
  were hidden. The reduced-motion preview restored the full source. These are
  native accessibility-action checks, combined with the owner's VoiceOver speech
  checks above; speech of every Unicode/effect combination was not separately recorded.
- At the available 2x Retina scale, visual inspection found no obvious clipping
  in the Unicode sample with gradient/glow or its reduced-motion selection.
  Existing Metal/WGPU pixel tests passed. The real-shaping interaction regression
  now changes scale through 1x, 1.5x, 2x and 3x and verifies unchanged canonical
  carets, complete selection and accessible source. This simulates geometry, not
  a physical monitor or pixel-rendering certification at every scale.
- A separate opt-in Metal workload rendered/read back 6,509 frames over 65 seconds
  at a 400x120 logical viewport (2x), cycling wave, scramble and typewriter with
  an 8px glow and selected Unicode text. Each frame used an autorelease pool.
  Metal device allocations stayed at **14,204,928 bytes** at all samples.
  Process RSS was **55,280 KiB at 5s**, then **56,336 KiB at 20/35/50/65s**.
  No continuing growth was observed after warm-up; a static/reduced-motion frame
  rendered successfully afterward. This is a bounded offscreen observation,
  not a long-duration native-window/GPU-profiler certification.

Reproduce the resource observation separately from the normal focused suite:

```sh
cargo test --offline --locked --config .cargo/validation.toml \
  --manifest-path extensions/crates/xgpui-shader/Cargo.toml \
  --target-dir target --features portable-shaders,wgpu \
  sustained_typography_resource_observation --lib -- --ignored --nocapture
```

The normal focused suite passed 15 tests with this resource observation ignored;
the observation was then run explicitly and passed. Windows validation is deferred
at the owner's request; Linux acceptance remains unverified. Additional physical
display scales and exhaustive VoiceOver speech combinations remain explicit limits.

### Native-window resource observation — 2026-11-04

Ran the actual `shader-examples --example typography` desktop window on the same
macOS host, based on local commit `51e1ab4`. Temporary instrumentation started
Wave with the demo's default gradient/glow, counted view renders, paused playback
at 180 seconds, then quit normally after another 30 seconds. The source was
restored and the ordinary demo rebuilt; no monitoring behavior remains enabled.
`ps` sampled process RSS and CPU every 15 seconds. Reduced motion was off.

| Elapsed | View renders | RSS (KiB) | Process CPU (%) | State |
| --- | --- | --- | --- | --- |
| 15s | 924 | 115,936 | 48.0 | Wave/glow |
| 30s | 1,867 | 115,936 | 46.1 | Wave/glow |
| 60s | 3,025 | 116,160 | 26.4 | Wave/glow |
| 120s | 4,159 | 115,696 | 30.2 | Wave/glow |
| 165s | 5,282 | 115,696 | 28.2 | Wave/glow |
| 180s | 5,469 | 115,680 | 0.0 | Pause boundary |
| 195s | 5,470 | 115,712 | 0.7 | Paused |
| 210s | 5,470 | 115,744 | 0.6 | Paused |

RSS across all post-startup samples ranged from 115,680 to 116,416 KiB (about
113–114 MiB), without a continuing upward trend. CPU readings are instantaneous
`ps` percentages relative to one core, not GPU usage. Frame cadence varied during
ordinary desktop operation; this was not a controlled frame-rate benchmark.
Pausing produced one final redraw and then no additional view renders through
210 seconds. The process exited normally with code 0. No native-window resource
regression was observed in this workload. Native GPU allocations were not profiled;
the separate offscreen Metal measurements above remain the GPU-allocation evidence.

### Native acceptance procedure

On each platform, record the revision, OS, renderer, display scale, fonts and
assistive-technology version. Run the focused suite and native build, then launch
the `typography` recipe shown above. Record each area as passed, failed or
unverified, with reproduction steps for failures.

1. Try Editorial, Ligatures, Unicode and Overflow in Static, partially revealed
   Typewriter, Wave and Scramble modes. Check Tab/Shift+Tab focus, click/drag,
   Shift+click, double/triple click, grapheme/word navigation, selection reversal,
   Home/End and platform Select All/Copy shortcuts. Paste into a separate native
   editor and compare exact source, including combining marks and joined emoji.
   Hidden source must remain selectable/copyable. Resize and change type size
   while keeping the active endpoint visible.
2. Check complete reveal against Static, repeat the same scramble seed/tick, and
   toggle fill, shadow, softness and light/dark themes at available display scales.
   Look for clipped glyphs/halos or selection artifacts. Enable the reduced-motion
   preview and the OS preference through the host's supported propagation path;
   confirm complete static source and stopped playback.
3. With VoiceOver (macOS), NVDA (Windows) or Orca (Linux), check focus, complete
   source reading at zero reveal, selection reporting and selection actions,
   including a combining grapheme and joined emoji. Record unavailable actions
   and adapter limitations explicitly; inspecting metadata alone is insufficient.
4. Record process memory/GPU observations after warm-up, during repeated animation
   and after pausing. Record the duration and workload so measurements can be
   repeated. Investigate continuing growth rather than treating retained caches
   alone as a leak.

Browser build and rendering/interaction smoke checks are secondary regressions.
The missing canvas accessibility bridge remains a separate platform follow-up.
Close the single-line LTR milestone only when the native matrix is satisfied or
remaining blockers are explicitly agreed and recorded. Wrapping, BiDi, editing,
stacked shadows and emoji silhouettes remain separate work.
