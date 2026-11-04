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
| [Italy photo comparison](../examples/extensions/?demo=italy) | Select sRGB, Display P3 or highlighted; compare Original / browser color with sRGB preview on the same selected photo | Browser-managed original JPEGs compared with Rust/Wasm clipped sRGB derivatives; not GPUI canvas wide output |
| [Interpolation](../examples/extensions/?demo=color) | Compare encoded/linear/perceptual rows and transparent stops | Interpolation and mapping differences in an sRGB preview |

The runtime panel reports the actual canvas color space where queryable and
reports browser display capability separately. A report of P3 display capability
does not turn an sRGB canvas into a wide-gamut canvas. GPU readback and native
wide-SDR composition are not part of these browser demos.

The canvas routes offer forced WebGPU and WebGL2 links in the
[gallery](../examples/extensions/). Source calculations are Rust/Wasm in both
backends. No image-file decoder, arbitrary file chooser or network profile fetch
is required. The chromaticity locus is attributed CIE data, not monitor samples.

## Italy: compare the same photograph

Select **Display P3**, then switch **Original / browser color** and **sRGB preview**.
Look at the orange/red umbrellas. Original mode uses WebKit’s unmodified JPEG
with its embedded profile, presented by a browser image element. The preview
uses the desktop lab’s prepared linear P3 pixels and Rust/Wasm `xgpui-color`
clipping to sRGB. Neither control changes your monitor mode. A wide-gamut display
and browser color management affect visible differences; on an sRGB display they
may be small. These controls do not configure GPUI WebGPU/WebGL2 presentation.

The three thumbnails select separate WebKit image versions. The highlighted
photograph contains diagnostic markings, visible in both presentation modes.
All assets are served locally, with [WebKit attribution](https://webkit.org/blog-files/color-gamut/).

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

The native chromaticity workspace is integrated into this repository's
`color-lab` binary. Run it from the root, then switch between **Swatches** and
**Chromaticity** with the tabs:

```sh
just lab-color
just lab-color --chromaticity
just profile=dev lab-color --icc-sampled
just lab-color --uv --native-fill
just lab-color --photo
just lab-color --help
just lab-color-test
```

The launch recipes default to release; `profile=dev` selects a development
build. Chromaticity flags select that workspace automatically. Existing
`lab-chromaticity` / `chromaticity` recipes forward to the local color lab;
no sibling checkout is needed. State is retained when switching tabs.
`lab-gamut` remains the separate native wide-output/compositing lab.
See the [color-lab README](../../apps/xgpui-ext-lab/color-lab/README.md)
for source provenance, fixtures and platform/test scope.

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

The integrated color lab passes 24 macOS tests covering independent numerical
references, ICC interpretation, FP16 storage, pinned WebKit assets, photo loading,
and native-versus-preview Metal composition. Live macOS verification covers both
tabs, chromaticity fill, and xy/u′v′ state retention when switching workspaces.
The binary and tests cross-check for Windows; Windows/Linux runtime acceptance
and physical monitor accuracy remain unverified. Source provenance, shared asset
attribution and exact test scope are recorded in the color-lab README.
