# SVG sample attribution

These are artwork samples, licensed separately from the Apache-2.0 Rust code.
Keep this notice with copies of this directory. No endorsement is implied.
Third-party SVGs are copied byte-for-byte; embedded metadata and notices are
preserved. The exact SHA-256 hashes are in [corpus.json](../bench/corpus.json).
Sources and license evidence were checked on 2026-10-24.

| File | Creator / copyright attribution | Terms | Source and evidence |
|---|---|---|---|
| `AJ_Digital_Camera.svg` — “digital-camera” | AJ Ashton | Public-domain dedication recorded in the file's RDF (`http://web.resource.org/cc/PublicDomain`); the file expressly permits reproduction, distribution and derivatives | [SVG Web source](https://dev.w3.org/SVG/tools/svgweb/samples/svg-files/AJ_Digital_Camera.svg); creator, rights holder and dedication are embedded in the SVG; [Openclipart public-domain policy](https://openclipart.org/share) |
| `rg1024_green_grapes.svg` — “green grapes” | rg1024 | Openclipart public domain / [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/) | [Artist's upload](https://openclipart.org/detail/20104/green-grapes); [SVG Web copy](https://dev.w3.org/SVG/tools/svgweb/samples/svg-files/rg1024_green_grapes.svg); [Openclipart policy](https://openclipart.org/share) |
| `rg1024_metal_effect.svg` — “metal effect” | rg1024 | Openclipart public domain / [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/) | [Artist's upload](https://openclipart.org/detail/20070/metal-effect); [SVG Web copy](https://dev.w3.org/SVG/tools/svgweb/samples/svg-files/rg1024_metal_effect.svg); [Openclipart policy](https://openclipart.org/share) |
| `dragon.svg` — “Dragon clip art” (2012) | Copyright Ebaychatter0 | [Creative Commons Attribution-ShareAlike 3.0 Unported](https://creativecommons.org/licenses/by-sa/3.0/), [legal code](https://creativecommons.org/licenses/by-sa/3.0/legalcode.en) | [Author and license declaration](https://commons.wikimedia.org/wiki/File:Dragon_clip_art.svg); copied from the existing GPUI example, including its prior source/license comments. No artwork changes made for this extension. |
| `ribbon-leaf.svg` | Copyright 2026 xgpui contributors | [Apache-2.0](../../../LICENSE) | Original to this lab; exported from `src/scene.rs` with clipping and shadow enabled. |

The portrait fixture at `../tests/fixtures/portrait.svg` is also original to
this lab: Copyright 2026 xgpui contributors, Apache-2.0.

The dragon remains CC BY-SA 3.0; it is not relicensed under Apache-2.0. Retain
credit, source/license links and existing notices, identify modifications if
made, and apply the license's ShareAlike terms to adaptations of that artwork.

## Optional historical benchmark sources, not bundled

The ten-file historical corpus is larger than this redistributable sample set.
`bench/fetch.py` can download the remaining files into a separate local directory
for reproducing recorded measurements. Downloading a file does not change its
terms; the manifest is not a blanket license grant.

- `gallardo.svg`: Michael Grosberg, “Lamborghini Gallardo” (2006),
  [CC BY-NC-SA 2.5](https://creativecommons.org/licenses/by-nc-sa/2.5/), as declared
  in its RDF. Its noncommercial restriction is why it is not bundled here.
- `USStates.svg`: embedded notice credits Jon Relf, with code help from
  johnnysaucepn and Jeff Schiller. It identifies the illustration/map as GNU FDL
  and scripts as GNU GPL, without version numbers or complete original-map
  attribution. Kept fetch-only rather than supplying incomplete licensing notices.
- `tiger.svg`, `ny1.svg`, `displayWebStats.svg`, `Steps.svg`: the tested copies
  do not provide sufficient creator/reuse information to include them here.
  Their source URLs and pinned hashes remain in the manifest for traceability.

The reports retain measurements and source identifiers, not copies of these
excluded artworks or their rendered previews. The standalone lab/gallery can
use the committed samples completely offline; historical benchmark reproduction
requires the full optional corpus.
