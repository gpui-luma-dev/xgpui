# Explore and verify color behavior

Use browser demos to understand the choices. Use the native lab to test the
native composition path. Both distinguish a wide-gamut capability from a named
color space. Start with [color workflows](color-workflows.md) if you are choosing
an API rather than investigating a specific sample.

## Browser demos

| Demo | What to try | What the result establishes |
| --- | --- | --- |
| [Chromaticity and gamut](../examples/extensions/?demo=chromaticity) | Keep the pixel fixed, change interpretation; compare simulated gamuts, Y and mapping | CPU color relationships, locus outlines, mapping and an sRGB fill preview |
| [Pixels + metadata](../examples/extensions/?demo=pixel-metadata) | Keep Mid gray fixed, cycle named/ICC interpretation, alpha and storage | Decoding, XYZ changes and actual CPU-packed FP16 bytes |
| [Interpolation](../examples/extensions/?demo=color) | Compare encoded/linear/perceptual rows and transparent stops | Interpolation and mapping differences in an sRGB preview |

The runtime panel reports the actual canvas color space where queryable and
reports browser display capability separately. A report of P3 display capability
does not turn an sRGB canvas into a wide-gamut canvas. GPU readback and native
wide-SDR composition are not part of these browser demos.

Each route offers forced WebGPU and WebGL2 links in the
[gallery](../examples/extensions/). Source calculations are Rust/Wasm in both
backends. No image-file decoder, arbitrary file chooser or network profile fetch
is required. The chromaticity locus is attributed CIE data, not monitor samples.

## Pixels + metadata: a short walkthrough

Click the numbered lesson buttons to set up repeatable comparisons. The left
swatch always interprets the same samples as named sRGB; the right uses the
selected interpretation. Both are sRGB previews.

1. **Gray + Adobe:** compare brightness and source XYZ Y. Adobe's transfer
   curve decodes 0.18 differently, so the right swatch should look slightly darker.
2. **Green + Adobe:** compare XYZ and packed storage values. Saturated green
   may look identical on both sides because the browser clips to sRGB.
   Equal previews do not establish equal source colors.
3. **Sampled ICC:** inspect XYZ and FP16 words. The sampled sRGB profile's
   differences are small; a visible tint is not expected. Half alpha makes the
   white background contribute to the preview.

Then cycle **Pixels** to try Red, and **Storage** or **Mapping**. A storage
change changes coordinates/packing; mapping matters only when a color exceeds
those storage bounds. Neither necessarily changes the final sRGB preview.
The native lab is needed to compare actual wide output or inspect GPU readback.

## Run the native chromaticity lab

The standalone app lives in the sibling `xgpui-lab` checkout, outside the
unpublished in-repository `xgpui-ext-lab` package. Keep them distinct when choosing
commands. From `xgpui`:

```sh
just profile=dev chromaticity
just profile=dev chromaticity --icc-sampled
just chromaticity-test
```

From the sibling `xgpui-lab` directory:

```sh
just chromaticity
just chromaticity --icc-parametric
just chromaticity-test
```

The root recipe defaults to a release build; `profile=dev` selects a development
build. Clone the sibling repository beside xgpui for root delegation to work.
These commands require the matching local API changes in both repositories.

## Read the controls in order

1. **Source / Metadata / Transfer:** establish what the supplied numbers mean.
   Metadata cycles named input, synthetic probes and published profiles.
2. **Storage / Map:** choose bounded P3 or Rec.2020 storage and its mapping policy.
3. **Wide-gamut window:** inspect effective native mode and fallback status.
4. **Presentation:** choose no fill, sRGB preview or native-wide fill output.
5. **Simulated gamut / Fill Y:** set the bounds and relative luminance used to
   construct the fill; these do not measure your display.

The enlarged chart sits beside vertically stacked native and sRGB sample cards.
Decode tracking is below it. The white cross is D65; the gold cross is the
selected source color. At black, chromaticity is undefined. A profile's neutral
ramp can be close to D65 without being mathematically identical to it.

## Interpret the fill

The outlines are sRGB, Display P3, Adobe RGB (1998) and Rec.2020 reference
gamuts. Hatching identifies colors outside the selected simulated RGB bounds
at the selected relative Y. The reported percentage counts sampled cells in
that diagram, not the fraction of colors your monitor can display.

An sRGB presentation adds sRGB clipping. A native-wide presentation can retain
extended composition channels, but the OS profile and actual panel still limit
the visible result. This is why changing simulated gamut and changing
presentation are separate controls.

## Inspect CPU values and check the GPU

Decode tracking shows source RGB, XYZ, coordinates and actual CPU-packed FP16
values/bits. Predicted linear-P3 composition is a calculation.

**Check GPU (Metal FP16)** re-renders the painted scene and reads nine center
pixels from the extended linear-P3 FP16 composition target. Show the full native
sample, enable wide mode and wait for upload. It compares the measured snapshot
with an independent reference and reports per-channel budgets. Control changes
invalidate the previous result.

The check starts at packed source FP16 bytes; source/ICC interpretation has
separate precision tests. A constant center checks color preservation through
blur, not the blur's edge shape. Neither the check nor a screenshot establishes
physical display accuracy. Windows/Linux floating-point readback is unsupported.

## Validation status

macOS Metal live checks pass for all four published profile selectors at
half-alpha Green, and for sampled sRGB2014 Mid gray directly and through nested
blur. Its decoded values, XYZ and small neutral offset match the displayed
reference precision. The full metadata cycle and returning to named input also
work. Detailed fixtures and results remain in validation documents beside the
implementations; runtime evidence is kept separate from cross-compilation.
