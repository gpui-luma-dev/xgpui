# Wide-gamut content surfaces

**Private Metal prototype; public high-gamut APIs remain design only.**
[Issue #8](https://github.com/gpui-luma-dev/xgpui/issues/8).
Local branch: `codex/high-gamut-windows`, based on paint commit `1331bdf`.
[Paint PR #9](https://github.com/gpui-luma-dev/xgpui/pull/9) stays draft and unmerged.

## One window, two content contracts

```text
GPUI controls                         wide-gamut surface
Rgba / Hsla / Style unchanged          explicit color interpretation
existing sRGB UI rendering             extension-owned image/processing
             \                        /
              ordered host composition
              color-managed presentation
              one native window
```

The window carries the combined output; only designated content surfaces produce
wide colors. Buttons, labels, native gradients, icons and panels keep their sRGB
meaning. A surface is a retained GPUI element, not another OS window.

```text
first platform   macOS / native Metal
first content    SDR Display P3; no HDR brightness/headroom
extension        proposed xgpui-gamut: surface, conversion, validation, policy
host             ordered composition, layer configuration, readback
local app        apps/gamut-lab (implemented private prototype)
later           wider working spaces, HDR, Windows/Linux presentation, media
```

No native P3 button/border API is needed for this milestone. Existing
`xgpui-shader` mesh, gradient and WGSL output remains sRGB; enabling presentation
does not reinterpret it. A future wide producer can opt into the surface contract.

## Run the private prototype

```sh
just --justfile apps/gamut-lab/justfile ui
just --justfile apps/gamut-lab/justfile baseline
just --justfile apps/gamut-lab/justfile headless --frames 60
just --justfile apps/gamut-lab/justfile capture
just --justfile apps/gamut-lab/justfile metal-test
just --justfile apps/gamut-lab/justfile lint

# Capture the actual controls, explicitly converted/clipped to sRGB:
just --justfile apps/gamut-lab/justfile ui --ui-png /tmp/gamut-lab-ui.png
```

```text
gate        macOS + debug_assertions + gpui-apple/test-support
activation  XGPUI_GAMUT_LAB=1; lab re-execs itself before initializing threads
baseline    --sdr removes the switch and uses existing sRGB producers
window      opaque only; tagged linear P3 RGBA16Float; EDR disabled
controls    overlays, surface opacity, light/dark backdrop, reverse surface order
content     two immutable linear P3 color patches, not photo/profile decoding
readback    existing image APIs return an explicit clipped sRGB preview
```

The temporary wire is a checked `(MetalCustomOutput, [u32; 4])` tuple with an exact
private version/color/alpha tag. It is recognized only inside the gated host
experiment; the existing `MetalCustomOutput` contract stays sRGB/8-bit. This tuple
is not an SDK contract. No `Window`, platform trait, core color, preparation,
scene or public export additions were made.

Implemented privately in `gpui-apple/src/metal_renderer/gamut_lab.rs` and its
Metal shader; content fixtures/UI live in `apps/gamut-lab`. Layer configuration
uses typed Core Animation bindings. The additional binding feature is enabled
through `test-support` only.

Acceptance checks executed on macOS Metal:

- Apple backend library suite: 30 tests pass, including seven prototype tests: Float16 P3 preservation, sRGB UI parity within one code
  value, interleaved overlays/order, queued scratch reuse, low-alpha edges,
  clip/radii/opacity, border/path masks, resize/budget, invalid tag/format rejection
  without submitting pending work, and layer color-space/EDR configuration.
- Existing paint-lab suite: 21 tests pass with the switch absent.
- Actual control UI captured through the explicit sRGB preview path.

Prototype limits: full-viewport conversion per UI run; an extra Float16 copy for
presentation; patch producers allocate on preparation; no GPU timing or optimized
pooling yet. Scratch + composition targets are capped at 256 MiB per live pair,
excluding drawable, producer, path/atlas and in-flight retention. Headless elapsed
time includes initialization, encoding and readback; it is not a GPU benchmark.
Physical display/profile acceptance and transparent windows remain outstanding.
The preview image cannot prove physical wide-gamut output.

## App API sketch

Illustrative names, **not shipped APIs**:

```rust
// Proposed extension API; one ordinary GPUI window.
let status = xgpui_gamut::enable(window, WideGamutSdr)?;

// A retained immutable surface snapshot, with explicit source interpretation.
let photo = GamutImage::from_display_p3(source)?;
div()
    .child(div().child("Exposure"))          // Ordinary sRGB controls.
    .child(gamut_surface(photo).child(       // Normal layout/clip/scene ordering.
        div().id("overlay").child("100%"),   // Native sRGB overlay.
    ));
```

`GamutImage` is extension-owned; initial accepted storage, upload ownership and
constructors still need a separate concrete sketch. Decode embedded image profiles
before this boundary; untagged bytes must not silently become P3. Do not promise
ICC decoding, RAW or video in the first slice.

## Compositing contract

Implemented private prototype: **ordered sRGB UI runs + linear wide composition**.
A UI run is a maximal consecutive sequence of legacy draws between wide draws.
Existing sRGB custom paints belong to UI runs too.

```text
scene: UI background → wide photo A → controls → wide photo B → tooltip

1. legacy renderer → transparent sRGB UI scratch → convert/composite
2. wide photo A    → linear wide composition target
3. legacy renderer → cleared UI scratch         → convert/composite
4. wide photo B    → linear wide composition target
5. legacy renderer → cleared UI scratch         → convert/composite
6. tagged presentation, preserving the final scene order
```

Do not flatten all UI into one overlay or put all wide surfaces behind it.
Reuse a scratch target in GPU command order; each run is consumed before clearing
it for the next run. Scene layers, replay and inherited clip/opacity remain host
responsibilities. Surface children paint at their normal position after the
surface background; subsequent siblings can cover either.

| Boundary | Proposed first contract |
| --- | --- |
| Legacy UI scratch | Existing `BGRA8Unorm` shaders/blending, transparent clear; resulting RGB is premultiplied encoded sRGB. Audit every contributing pipeline before relying on this. |
| Wide producer output | Distinct versioned backend output: `RGBA16Float`, linear Display P3/D65, premultiplied alpha, finite SDR RGB and alpha. No texture-format inference. |
| Content bounds | Straight linear P3 RGB in `0..1`, alpha in `0..1`; associated RGB in `0..alpha`, zero RGB when alpha is zero. Wider/HDR values require a later contract. |
| Wide composition target | Linear Display P3/D65, Float16, premultiplied source-over. Separate pipeline from legacy straight-alpha compositor. |
| Layer tag | Must describe the actual linear P3 values. Candidate: extended-linear Display P3; extended storage does not enable HDR intent. |
| SDR white | Relative white = 1.0 for both controls and content; no implicit exposure, brightness boost or tone mapping. |

Display P3 has P3 primaries, a D65 white point and the sRGB transfer function; Apple's
[extended-linear Display P3](https://developer.apple.com/documentation/coregraphics/cgcolorspace/extendedlineardisplayp3)
uses the same primaries with a linear transfer function. See also
[Display P3](https://developer.apple.com/documentation/coregraphics/cgcolorspace/displayp3).

Conversion of the UI scratch, expressed per sample:

```text
A       = sampled scratch alpha
C       = A > 0 ? sampled encoded RGB / A : 0
L       = srgb_decode_piecewise(C)               // Not pow(C, 2.2).
P       = linear_srgb_to_linear_p3(L)
source  = (A * P, A)                            // Associated linear RGB.

k       = host coverage * inherited opacity
src     = (k * source.rgb, k * source.a)         // Apply once.
out.rgb = src.rgb + dst.rgb * (1 - src.a)
out.a   = src.a   + dst.a   * (1 - src.a)
```

Wide output is already associated linear P3: sample/filter that representation,
then multiply both RGB and alpha by host coverage/opacity. Producers do not bake
in host clip, placement, rounded coverage or element opacity. Use the full local
logical source and exact raster extent from existing `PreparationRequest`.

The UI conversion pass uses aligned texel reads for an unscaled UI run, avoiding
an extra resample. Validate low-alpha Float16/8-bit behavior independently; no
arbitrary epsilon cutoff or saturation clamp as a substitute for a correct alpha
contract. Avoid applying element opacity twice when it is already in a UI run.

## What “normal sRGB controls” guarantees

```text
opaque panel + its controls   legacy rendering within the run; same color meaning
opaque sRGB patch            explicit conversion; should visually match default UI
translucent overlay on photo linear P3 source-over at the cross-content boundary
```

Runs preserve legacy blending internally. They cannot guarantee pixel-identical
legacy blending across a wide-surface boundary: encoding conversion is nonlinear,
and flattening a transparent run loses individual draw/backdrop interactions.
This is a proposed, testable behavior—not a claim of exact equivalence.

Grayscale text coverage can compose over wide content. Backdrop-dependent effects,
LCD/subpixel text and destination-reading blends need explicit handling; do not
silently claim that transparent scratch preserves them. Current macOS Metal
batch dispatch marks `SubpixelSprites` unreachable; verify that remains true for
all supported text paths. Transparent top-level windows are deferred until their
layer/desktop alpha contract is tested.

A whole-scene linear renderer remains an alternative if run splitting proves too
expensive or visually unsuitable. It changes more UI rendering internals. A
separate native child layer is not the first choice: arbitrary interleaving,
rounded coverage, overlays and captures would need another ordering contract.

## Smallest GPUI surface

| Area | Proposed change |
| --- | --- |
| `Rgba`, `Hsla`, `Background`, `Fill`, `Style`, `Styled` | None. No color tags hidden in padding or existing values. |
| `WindowOptions`, `WindowParams` | No new required fields; preserve complete struct literals. |
| `CustomContent`, `PreparationRequest`, `CustomDraw`, scene primitive enums | Reuse unchanged. Surface records use existing `Window::paint_custom`. |
| `Window` | Candidate additive presentation setter/report. GPUI-owned request/status types; defaults preserve existing behavior. |
| `PlatformWindow` | Matching default methods report unsupported; no new required implementations. |
| Metal backend | Add a separately named/versioned wide-output type accepted through existing checked `Any`; retain existing `MetalCustomOutput` unchanged. |
| Renderer internals | Detect wide records, split ordered runs, validate/submit transactions, convert, compose and present. |

```rust
// Candidate GPUI boundary: types live in GPUI, never depend on xgpui-gamut.
window.set_color_presentation(request)?;
let report = window.color_presentation();
```

Keep the new output's fields private with validating constructors. It must use
the host device/queue, describe immutable full-extent content, and follow the
existing pending-command reuse, validation and discardable-transaction rules.
Preparation/validation of the complete batch happens before any submission;
adding a wide record must not submit an earlier UI producer on a rejected frame.

Fixed canonical surface output avoids adding destination-color fields to the
preparation request. Host output mapping changes independently of producer data.
Cache by retained queue/device identity, extent and immutable content revision;
output contract/version is fixed by type. Display-profile changes alone need not
regenerate canonical content. Any future destination-dependent output must add
that destination to its cache key explicitly.

## Negotiation, fallback and lifecycle

The report distinguishes requested presentation, effective format/color space,
backend support, display evidence and fallback reason. Unknown is not unsupported.
Reconfigure atomically before a frame; an error keeps the last valid configuration.
An extension must not mutate the native layer behind the renderer's pipelines.

| Situation | Proposed behavior |
| --- | --- |
| Default window, no wide content | Existing renderer path; no Float16 composition or scratch passes. |
| Enabled window, no wide content | SDR UI remains sRGB; any necessary final conversion is explicit and measured. |
| SDR display / forced reference fallback | Map linear P3 to linear sRGB, clip to `0..1`, then encode sRGB. This is a deterministic baseline, not perceptual gamut mapping. |
| Color-managed wide presentation | Tag output correctly; assess OS display mapping against references. Do not apply the explicit SDR transform as well. |
| Unsupported backend | Return inspectable unsupported status; do not silently omit the surface. A portable SDR preview is separate work. |
| Display move / profile change | Refresh mapping/report at a frame boundary; rebuild only format-dependent resources. |
| Resize / scale / device reset | Recreate invalid targets/resources; preserve compatible immutable snapshots. |

The final OS-managed SDR mapping policy still needs validation. A window spanning
displays cannot promise identical physical output everywhere. HDR/EDR enablement,
reference luminance in nits and tone mapping remain outside this SDR milestone.

## Implementation touchpoints

Repository-relative paths; inspected constraints:

| File | Relevant constraint |
| --- | --- |
| `gpui/crates/gpui/src/platform.rs` | No presentation negotiation; default-method approach avoids mandatory backend changes. |
| `gpui/crates/gpui-apple/src/metal_renderer.rs` | Fixed 8-bit main pipelines/path intermediates; retain them for UI scratch. Add wide composition/presentation internally. |
| Same file: `read_texture_to_image` | Four-byte BGRA assumption; Float16 capture needs a separate explicit conversion/readback path. |
| `gpui/crates/gpui-apple/src/custom_draw.rs` | Current output validator/compositor is sRGB/8-bit; keep old producers working and recognize the new output separately. |
| `gpui/crates/gpui-apple/src/shaders.metal` | Preserve native UI shader meaning; verify scratch alpha for every primitive. |
| `gpui/crates/gpui-macos/src/window.rs` | Existing display/scale callbacks; same-display profile notifications still need investigation. |

## Gamut-lab acceptance before committing the API

```text
layout      normal toolbar + inspector + wide photo/patch canvas + tooltips
controls    mode, alpha, light/dark backdrop, overlay, two interleaved surfaces
diagnostics request/effective format/space, display evidence, fallback
exports     raw Float16 + color metadata; explicit sRGB PNG conversion
perf        UI-only baseline; one surface; alternating UI/wide stress case
```

1. Independent transfer/matrix reference; sRGB round-trip target ≤1 eight-bit code
   value for opaque patches. P3 outside-sRGB patches survive the wide target.
2. Default rendering pixel regressions unchanged. Compare opaque UI panels in
   both modes, then separately assess cross-boundary overlay differences.
3. Red/green/blue low-alpha edges over light/dark backgrounds; masks, opacity,
   rounded corners, paths, text, cached snapshots and replay.
4. UI → wide A → UI → wide B → UI ordering, including overlapping tooltips and
   translucent overlays. Verify scratch clearing/reuse and multiple frames in flight.
5. Rejected wide output leaves no submitted batch; existing paint tests still pass.
6. Move/profile/resize/device recovery; tagged reference on a wide-gamut display.
   An untagged screenshot is not proof of gamut correctness.
7. Measure run count, conversion passes, target memory and frame-time impact.
   Float16 is eight bytes/pixel; UI scratch is four. Bound scratch/target reuse
   and in-flight retention before accepting an implementation.

Next design gate: settle cross-boundary blending, the first surface upload API,
window request/report types and measured performance budgets. Prototype private
composition in gamut-lab before expanding the GPUI API.

[Existing paint contract](extension-paints.md) ·
[Preparation and recording](paint-contract-hardening.md) ·
[Layer color-space configuration](https://developer.apple.com/documentation/quartzcore/cametallayer/colorspace)
