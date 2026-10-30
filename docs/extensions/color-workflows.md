# Color and wide-gamut workflows

Start with the task you need to complete. XGPUI separates the meaning of your
pixels from their storage and from the window that presents them.

| I want to… | Start here |
| --- | --- |
| Display supplied RGB pixels with the right interpretation | [Pixels and metadata](pixels-and-metadata.md) |
| Preserve Adobe RGB or P3 colors through storage | [Choose image storage](pixels-and-metadata.md#choose-storage-deliberately) |
| Enable wide-gamut presentation in a native window | [Wide-gamut windows](high-gamut-windows.md) |
| Use an embedded RGB ICC profile supplied by my decoder | [ICC profiles](icc-profiles.md) |
| Explore the behavior before integrating it | [Color labs and browser demos](color-labs.md) |
| Convert a color or interpolate a gradient | [Color math API](color-math.md) |

## Wide gamut is not another name for P3

**Wide gamut** describes the ability to represent colors beyond a smaller
reference gamut, commonly sRGB. **Display P3**, **Adobe RGB (1998)** and
**Rec.2020** name particular color spaces. A wide-gamut display does not
necessarily cover every color in any of them.

Use “wide-gamut window” when discussing the presentation capability. Use
“Display P3” when declaring P3 source pixels, storage, producer output or the
Metal composition coordinates. There is no Adobe RGB window mode to select.
The native renderer converts supported sources into its composition space;
the OS display pipeline and the panel determine the visible result.

| Term | What it tells you | What it does not establish |
| --- | --- | --- |
| Source color space / ICC profile | What supplied RGB numbers mean | The display's capabilities |
| P3 or Rec.2020 storage | The linear primaries used for stored pixels | That a window is wide gamut |
| RGBA16F | Channel representation and precision | Primaries, transfer, ICC interpretation or HDR |
| `WideGamutSdr` | Requested native wide-SDR presentation | Full Adobe RGB/P3 coverage or increased brightness |
| Display wide-gamut status | Platform-reported capability | Calibration or physical output accuracy |
| Browser `color-gamut: p3` | Browser-reported gamut at least approximately P3 | The canvas's configured color space |

## Follow the pixel through the boundaries

1. **Interpret.** Supply the source primaries, transfer and alpha meaning, or a
   resolved custom definition from supported ICC bytes.
2. **Decode and adapt.** Evaluate the declared curves in linear light and adapt
   the declared XYZ reference to D65 where needed.
3. **Choose storage and mapping.** Select P3 or Rec.2020 and a mapping policy.
   Associate alpha and generate linear-light mipmaps after that choice.
4. **Compose.** Native wide placement converts storage primaries into the
   renderer's composition space. Metal uses extended linear P3; Windows uses
   extended linear sRGB/scRGB.
5. **Present.** Request the window mode and inspect its effective status. The
   OS maps the result for the actual display.

Changing the source interpretation changes the color represented by identical
RGB numbers. Changing storage or output mapping changes how that color is
represented or reduced. Changing a display-capability flag does not reinterpret
the source numbers.

## Which parts run where?

| Task | Native application | Browser demos |
| --- | --- | --- |
| Named/custom color math, curve evaluation, XYZ and mapping | CPU Rust | The same Rust compiled to Wasm |
| Bounded RGB ICC parsing | Optional `xgpui-color/icc` | Bundled-profile demo uses the same optional parser |
| Explicit P3/Rec.2020 CPU pixel packing | Media pipeline | Pixels + metadata demo shows actual CPU FP16 bytes |
| Wide-gamut composition/presentation | Metal implemented and runtime checked; Windows implemented with native acceptance still outstanding | Current GPUI demo canvas is an explicit sRGB preview |
| GPU FP16 composition readback | On-demand Metal validation | Not provided by these browser demos |
| Physical display measurement | Requires external measurement | Not established by a browser capability query |

The browser platform has color-space APIs, but enabling P3 output in this GPUI
web renderer is a separate implementation task. These demos do not advertise
that capability. Read the actual canvas report beside the browser's display
report. See [WebGPU canvas configuration](https://developer.mozilla.org/en-US/docs/Web/API/GPUCanvasContext/configure)
and [WebGL drawing-buffer color space](https://developer.mozilla.org/en-US/docs/Web/API/WebGLRenderingContext/drawingBufferColorSpace).

## A practical first check

Open the [pixels + metadata demo](../examples/extensions/?demo=pixel-metadata),
keep Mid gray selected and change the interpretation. Then open the
[chromaticity demo](../examples/extensions/?demo=chromaticity) and compare the
outlines and hatched mapping regions. Use the [native lab](color-labs.md#run-the-native-chromaticity-lab)
when you need to test wide composition or GPU readback.

For mathematical definitions and numerical contracts, continue to the
[color math reference](color-math.md). The workflows here require no derivation
of color-matching functions or chromatic adaptation matrices.
