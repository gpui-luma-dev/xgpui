# Display supplied pixels correctly

For this API, an image is supplied pixels plus an explicit interpretation.
Your application or format adapter decodes the file and resolves its metadata.
`xgpui-media` converts and retains the supplied pixels; the color core does not
open files or discover profiles.

## Choose an interpretation

| Supplied data | Interpretation |
| --- | --- |
| Known SDR RGB with named primaries and transfer | `PixelColor::Named(ImageColor)` |
| Encoded RGB with independent curves and explicit colorants | `PixelColor::Custom` with `Arc<RgbDefinition>` |
| Finite linear RGB containing negative or above-one channels | `PixelColor::ExtendedLinear` |
| Supported RGB ICC bytes supplied by your decoder | Parse once with `IccRgbProfile`, then use `PixelColor::Custom` |

Primaries and transfer are separate. P3 source pixels commonly use the sRGB
transfer curve; Adobe RGB (1998) uses its own gamma. Declaring sRGB transfer with
Adobe primaries means exactly that combination. RGBA16F does not imply linear
light unless the interpretation says so.

The application owns the missing-profile policy. It can require metadata,
explicitly assume sRGB, or supply a known interpretation. An unsupported profile
must not silently become sRGB. See [ICC profile scope](icc-profiles.md).

## Load named Adobe RGB pixels

```rust
use xgpui_media::{ImageJob, MediaLoader};
use xgpui_media::pipeline::{GamutMapping, ImageColor, ImagePrimaries};

fn load_adobe(
    loader: &MediaLoader,
    width: u32,
    height: u32,
    pixels: Vec<[f32; 4]>,
) -> anyhow::Result<ImageJob> {
    loader.load_pixel_buffer(
        width, height, pixels,
        ImageColor::ADOBE_RGB_1998.into(),
        ImagePrimaries::Rec2020,
        GamutMapping::Clip,
    )
}
```

This accepts tightly packed RGBA f32 samples and returns a background job.
The source interpretation is straight-alpha, encoded Adobe RGB (1998). Storage
is bounded associated linear Rec.2020 RGBA16F. Observe loading and upload errors
using the normal [media lifecycle](media-images.md); successful conversion alone
does not establish that the image has reached the GPU.

## Choose storage deliberately

| Source you want to retain | Storage choice | Why |
| --- | --- | --- |
| Entire P3 gamut | P3 | P3 red is slightly outside bounded Rec.2020 |
| Adobe RGB (1998) gamut | Rec.2020 | Adobe RGB fits, while some Adobe colors lie outside P3 |
| A deliberately reduced preview | P3 or Rec.2020 plus an explicit mapping policy | Out-of-storage colors are mapped at ingestion |

Neither storage space universally replaces the other. P3 red has a Rec.2020
blue component near `-0.00121034`; bounded Rec.2020 ingestion maps that color.
For an image already stored with a suitable interpretation, use native wide
placement to preserve its extended composition channels. An sRGB preview still
maps or clips those colors for its output.

`GamutMapping::Clip` clips destination channels. `ReduceChroma` performs the
color core's explicit chroma-reduction mapping. Select a policy as part of the
operation; do not infer it from a filename or display label.

## Keep alpha and mapping in the right order

Source alpha is finite and bounded. Straight and premultiplied source samples
have different meanings; declare the actual convention. Transfer-domain
premultiplication is undone before transfer decoding. Transparent pixels
canonicalize to zero RGB.

The pipeline converts in linear light, maps into the selected bounded storage,
then associates alpha. Mipmaps average associated linear values. Intermediate
working RGB may be negative or above one until the explicit storage/output
mapping boundary.

## Supply extended linear values

Use `PixelColor::ExtendedLinear { primaries, alpha }` when your preceding
operation already produced finite linear RGB outside 0–1. Do not pass that
output through a bounded encoded source decoder. Storage remains an explicit
bounded P3/Rec.2020 choice, so this is not a lossless arbitrary-float container.

## Show it in a wide-gamut window

Request `WindowColorMode::WideGamutSdr`, inspect the effective status, and use
`image.wide(...)` or the image view's wide placement API. See
[wide-gamut windows](high-gamut-windows.md) for producer and backend contracts.
The window's presentation space is independent of the original image profile.

Try the [pixels + metadata Wasm demo](../examples/extensions/?demo=pixel-metadata)
for interpretations and CPU packing, or the [native chromaticity lab](color-labs.md)
for native-wide versus sRGB previews and Metal GPU readback.
