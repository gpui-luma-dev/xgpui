# Font feature introspection and variation contract

Issue: <https://github.com/gpui-luma-dev/xgpui/issues/27>

The [web font-controls guide](extensions/font-controls.md) covers usage, support
and core 3.0 migration. This document retains the implementation contract,
measurements and individual API rationale.

## Feature introspection

Resolve a font, then query the resolved face:

```rust,ignore
let id = text_system.resolve_font(&gpui::font("Inter"));
match text_system.opentype_features(id)? {
    gpui::FontQuery::Unsupported => { /* Hide the feature inspector. */ }
    gpui::FontQuery::Supported(features) => {
        // An empty list is a successful query with no GSUB/GPOS features.
        for feature in features {
            println!("{}: GSUB={} GPOS={}",
                feature.tag, feature.substitution, feature.positioning);
        }
    }
}
```

The result is a sorted union of the face's GSUB and GPOS FeatureLists. It
records table provenance, merges duplicate tags, and returns errors for
unreadable feature metadata or invalid font IDs on supporting backends.
The reader validates feature records, not the complete shaping tables.
It does not enumerate AAT features, named stylistic-set labels, allowed values,
or script/language associations. Feature presence does not establish that a
feature applies to a particular script, language, text, or variation instance.
It describes this face only; fallback runs can use other faces. As with other
font queries, font IDs belong to the text system that created them.

Apple's font-kit backend, Parley, and DirectWrite implement feature and axis
queries. DirectWrite static axes require the runtime `IDWriteTextLayout4` probe
to succeed. Native/Parley font-controls labs and tests are verified on macOS
and Windows. Cosmic/web
currently return `Unsupported` through the default platform methods.
`TextSystem::font_capabilities()` reports support explicitly. Unsupported backends
reject nonempty axis requests in their `font_id` entry point rather than silently
pretending to apply them. Note that GPUI's existing `resolve_font` convenience
method can fall back on resolution errors; callers should check capabilities
before presenting axis controls. `Err`,
`Unsupported`, and `Supported(vec![])` must remain distinct in inspectors.

Use existing `FontFeatures` to request settings. Values are numeric OpenType
parameters, not exclusively booleans. For example, `("cv02", 2)` must retain 2;
0 disables a feature, while nonzero values can select different alternates.
Enumeration does not change the descriptor or enable any features.
Backend-specific shaping limitations still apply.

## Precise variation values

`FontTag` accepts exactly four printable ASCII bytes, including spaces.
`FontVariations::new` accepts `(FontTag, f32)` pairs in font design units:

- Coordinates remain signed and precise to f32; negative slant is preserved.
- Reject NaN and infinities, even when a later duplicate would overwrite them.
- Normalize negative zero to positive zero for consistent equality and hashing.
- Sort tags and retain the last occurrence of each duplicate.
- Do not quantize author values or clamp before the actual face is known.
- Empty settings preserve existing font behavior.

`Font::variations` now carries these values through Apple, Parley, and the
DirectWrite implementation. For normal
element styling, use `Styled::font_variations(...)` or `Styled::font(...)`;
`TextStyle::font_variations` propagates them into text runs. Values serialize as
a JSON map, such as `{"slnt": -10.125, "wght": 500.25}`. Deserialization validates
every entry before canonicalizing duplicates.

```rust,ignore
let mut font = gpui::font("Roboto Flex");
font.variations = gpui::FontVariations::new([
    ("slnt".parse()?, -10.125),
    ("wght".parse()?, 500.25),
])?;
```

## Static-axis resolution rules

Apple, Parley, and the DirectWrite implementation use the following rules in shaping, metrics, glyph bounds,
caret/selection geometry, fallback, and rasterization:

1. Use existing family, weight, and style selection to choose the face. Explicit
   axes do not select another family or substitute for static-face matching.
2. On that face, apply explicit coordinates after implicit weight/style axis
   settings. Explicit `wght`, `ital`, and `slnt` override the corresponding
   implicit coordinate. Other implicit coordinates remain as before.
3. Clamp each supported axis to that resolved face's advertised minimum and
   maximum. Unsupported tags are ignored for that face; they do not fail font
   selection. Preserve the original author settings separately from effective
   coordinates so fallback faces can apply their own support and ranges.
4. Apply this process independently to every fallback face. Do not transfer
   normalized coordinates between different faces. Resolved instance identity
   must describe the coordinates actually used throughout the pipeline.
5. Explicit `opsz` is in font design units and follows the same rules. There is
   no new automatic optical sizing. Preserve existing backend defaults when
   `opsz` is omitted. Automatic sizing requires a later size-dependent identity
   design, including units, precedence, clamping, and fonts without `opsz`.
6. Keep precise author coordinates distinct from any future preview policy.
   Preview quantization must occur before resolution and affect shaping and
   rendering together. The optional extension preview policy is described below; global budgets and
   frame-safe eviction remain separate.

Apple keeps requested descriptors separate from resolved native identity, then
applies author coordinates to resolved fallback runs before reshaping. Its cascade
now uses actual descriptors for in-memory user fallback fonts, which family-only
cascade descriptors could not reliably resolve. Parley passes explicit variations
through its shared flat-line/paragraph style builder and retains the resulting
normalized coordinates in its existing face registry. Retained IDs are never
recycled. Layout and raster keys continue to use `FontId`; no duplicate axis arrays
were added to glyph or layout cache keys.

DirectWrite first resolves the native faces using normal selection/fallback.
For each variable run, it clamps explicit coordinates against that face's font
resource, creates an instance collection, and applies effective coordinates to
an `IDWriteTextLayout4` range. The final callback retains the actual shaped face
for metrics/rasterization instead of selecting another face by family name.
Numeric typography settings remain associated with the requested descriptor.
In-memory user fallbacks use a collection containing custom and system fonts;
owned font buffers remain alive for the text-system lifetime. Static faces
ignore unsupported axes. The implementation and its five fixture tests have
been cross-checked for MSVC but not executed on Windows yet.

## Pinned fixtures and validation

Tests use repository font bytes, without host-font discovery for metadata:

| Fixture | License | SHA-256 |
| --- | --- | --- |
| `gpui/crates/gpui/assets/fonts/ibm-plex-sans/IBMPlexSans-Regular.ttf` | SIL OFL, adjacent `license.txt` | `975dcda37d80f038dcd143c22e33ca2d97a0cc5a929aace1c749153b0fe1afa5` |
| `extensions/crates/xgpui-parley/tests/fonts/InterVariable.ttf` | SIL OFL, adjacent `Inter-LICENSE` | `4989b125924991b90d05b2d16e0e388c48f7d5bb8b30539bbf9c755278d0ccaf` |

Core tests cover canonical settings, signed precision, invalid values/tags,
empty results, unsupported queries, malformed/truncated feature metadata,
merged GSUB/GPOS provenance, numeric feature serialization, and real feature
lists. Apple and Parley tests exercise resolved-face queries and invalid IDs;
Parley also exercises numeric controls through shaping. Static-axis tests cover negative slant with Roboto Flex, axis-order equivalence,
explicit weight precedence, range clamping, instance isolation, fallback
coordinates, retained raster output, size agreement with explicit optical size,
and paragraph wrapping/caret/selection geometry. The Roboto Flex source revision,
license, and SHA-256 are recorded beside the fixture in the Parley fonts README.


## Preview policy and measured baseline

The extension-owned `xgpui_parley::VariationPreview` accepts a fixed base font,
per-axis ranges and intervals, and an explicit maximum number of admitted
preview descriptors. It snaps coordinates before font resolution. Once the
limit is reached, old settings still work and new settings return an error;
the caller can keep displaying its last successful preview. Author coordinates
remain precise and must be committed separately after editing. Keep the same
controller for the inspector's lifetime. This controller does not evict IDs or
atlas tiles and does not claim a global CPU/GPU memory bound.

A debug CPU diagnostic on this macOS/Apple Silicon host ran an 8 × 8 × 4
`wght`/`wdth`/`slnt` sweep three times, using the pinned Roboto Flex font,
`Hamburgefonts 012345`, 28px, scale 1, and one subpixel variant. Raster masks were
built once per distinct raster key. Coarse preview intervals (400/50/5 design
units) illustrate a policy; these are not recommended production defaults.

| Policy | Requested descriptors | Registered faces/aliases | Raster keys | Mask payload bytes | First sweep | Three sweeps |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Precise | 257 | 515 | 5,120 | 1,361,381 | 1,145ms | 1,460ms |
| Coarse preview | 28 | 57 | 540 | 150,204 | 238ms | 508ms |

Counts include initial probes. Repeated identical sweeps stabilized after the
first sweep in both cases. Timings are local debug measurements, not production
performance targets. These CPU counts do not include all GPUI caches; mask payload bytes are not
atlas allocations or GPU memory. The diagnostic is
reproducible with:

```sh
cargo test --manifest-path extensions/crates/xgpui-parley/Cargo.toml --lib multi_axis_sweep_baseline --features native-test,gpui-platform/runtime_shaders --offline -- --ignored --nocapture
```

A separate native Apple/Metal diagnostic uploads the same specimen to a real
atlas. It uses native CoreText masks, so its timings are not comparable to the
Parley CPU-only timings above. GPU access required running outside the sandbox.

| Policy | Requested instances | Live tiles | Allocated pages | `MTLResource::allocatedSize` total | First sweep | Three sweeps |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Precise | 256 | 5,120 | 4 | 4,325,376 bytes | 381ms | 510ms |
| Coarse preview | 27 | 540 | 1 | 1,081,344 bytes | 91ms | 212ms |

Repeated sweeps stabilized. Removing all keys with no scenes or GPU commands
referencing them left zero active pages and tiles. A retained texture handle
still held 1,081,344 allocated bytes until dropped: active allocator counters
are not the same as total resource lifetime. This diagnostic does not measure
rendered frames, completion fences, driver-level reclamation, or cached-scene
invalidation. It exposed and now guards against loss of variation coordinates
when CoreText resizes an embedded font with implicit optical sizing. Effective coordinates and the sizing policy are retained explicitly in native
identity for the static-axis path. Shaping/drawing preserve these coordinates;
bounds and metrics use the same underlying instance. Empty-variation descriptors
retain the existing sizing path, separately from static-axis instances.

```sh
cargo test --manifest-path gpui/crates/gpui-apple/Cargo.toml --lib static_axis_sweep_measures --features font-kit,runtime_shaders --offline -- --ignored --nocapture
```

## Core GPUI public API review ledger

The user approved the API changes below, including the backend table-access
hook. Raw-table parsing is private inside core GPUI. Default inspection methods
read table bytes through `PlatformTextSystem::font_table`; Apple, DirectWrite,
and Parley implement only native table access. There is no helper package,
copied parser source, synchronization script, or public parser API.

| Public API | Compatibility | Why it is in core / extension alternative |
| --- | --- | --- |
| `Font::variations: FontVariations` | **Breaking:** exhaustive `Font` literals and patterns require updating | Ordinary GPUI text descriptors must carry coordinates into every backend. An extension-only descriptor cannot pass through existing `Font`-based styling and runs without another core hook. |
| `TextStyle::font_variations: FontVariations` | **Breaking:** exhaustive style literals and patterns require updating | Carries inherited coordinates into `font()` and `to_run()`. Could be omitted only if ordinary element styling does not expose axes. |
| Generated `TextStyleRefinement::font_variations: Option<FontVariations>` | Additive setting and serialized key; **breaking for exhaustive literals/patterns** | Required by normal style refinement/inheritance. Missing serialized values remain `None`. |
| `Styled::font_variations(FontVariations)` | Additive default trait method | Convenience over the style field; can be moved to an extension trait independently. |
| `FontTag` | Additive type; `FromStr`, `Display`, `Clone`, `Copy`, `Debug`, equality, ordering, hash; `as_bytes()` | Used by core variation settings and inspection results. Could move with the owning contract types, at the cost of a core dependency on that crate. |
| `FontVariations` | Additive type; `new()`, `settings()`, default, clone/debug, equality/hash, serde, schema, `TryFrom<BTreeMap<String,f32>>`, conversion back to that map | Needed by the new core descriptor/style fields. Validated immutable storage protects cache equality/hash invariants. Could move to a small shared font-types crate; not dependent on Parley. |
| `FontQuery<T>` with `Unsupported` / `Supported(T)` | Additive enum, clone/debug/equality | Makes unsupported inspection distinct from success and failure. Could move to an inspection extension if the query methods move. |
| `OpenTypeFeatureInfo` with public `tag`, `substitution`, `positioning` | Additive type, clone/debug/equality | Cross-backend inspection return type. Can move with an extension-owned inspector interface. |
| `VariationAxisInfo` with public `tag`, `min_value`, `default_value`, `max_value`, `hidden` | Additive type, clone/debug/partial equality | Cross-backend axis inspection return type. Can move with an extension-owned inspector. Localized names are not included. |
| `FontCapabilities` with public `feature_introspection`, `axis_introspection`, `static_variations` | Additive type, default/copy/clone/debug/equality | Makes incomplete backend support visible. Could move with an inspector interface, but static support still needs a discoverable contract. |
| `TextSystem::opentype_features(FontId)` | Additive inherent method | Entry point on the text system applications already own. Moving to an extension trait requires a backend inspection handle or another core access hook. |
| `TextSystem::font_axes(FontId)` | Additive inherent method | Same inspector-handle alternative. |
| `TextSystem::font_capabilities()` | Additive inherent method | Same inspector-handle alternative. |
| `PlatformTextSystem::opentype_features(FontId)` | Additive default trait method; existing implementations compile | Default implementation reads GSUB/GPOS via `font_table` and parses privately in core. Backend extension point. Can move to a separate optional inspector interface with corresponding access/ownership plumbing. |
| `PlatformTextSystem::font_axes(FontId)` | Additive default trait method | Default implementation reads `fvar` via `font_table` and parses privately in core. Same optional inspector-interface alternative. |
| `PlatformTextSystem::font_capabilities()` | Additive default trait method | Defaults to no new support; implementations opt in explicitly. Same interface alternative. |
| `PlatformTextSystem::font_table(FontId, FontTag) -> Result<FontQuery<Option<Vec<u8>>>>` | Additive default trait method; defaults to `Unsupported`, so existing implementations compile | Backend hook for resolved-face table bytes. `Supported(None)` means absent, `Some(bytes)` means read successfully, and errors mean invalid ID/read failure. Lets core parse metadata privately. An extension-owned reader would require another access hook to backend font handles. |
| `xgpui-windows --no-default-features` | Feature opt-out behavior fix; ordinary defaults unchanged | The dependency now has `default-features = false`; the existing default feature forwards `gpui/default`. This lets explicit opt-out avoid the core manifest compiler. Cargo feature behavior belongs to the backend crate, not an extension. |

No existing `FontFeatures` signatures or serialized behavior changed; its numeric
values were already supported. Apple implements the trait methods without new
public inherent methods or types. The preview controller and its constructor,
`font()`, and `admitted_instances()` are extension APIs, not core APIs.

### Migration for exhaustive literals

Prefer `gpui::font(family)` or `Font { ... , ..Default::default() }`. Existing
exhaustive font literals need `variations: Default::default()`. Existing
exhaustive text-style literals need `font_variations: Default::default()`;
refinement literals need `font_variations: None`. Exhaustive patterns can name
the new fields or use `..`. Empty coordinates preserve existing axis behavior.
The in-memory Apple fallback correction described above is a separate behavior
fix: requested embedded fallbacks can now resolve to their actual font.

## Released scope and follow-ups

The release includes Apple, DirectWrite and Parley static axes, inspection, and
measured previews. macOS and Windows labs and unit tests are confirmed working.
The public API is reviewed and raw parsers remain private in core. Follow-ups:

- [#139: measured CPU/GPU budgets and frame-safe eviction](https://github.com/gpui-luma-dev/xgpui/issues/139)
- [#140: DirectWrite static-axis and inspection parity](https://github.com/gpui-luma-dev/xgpui/issues/140)
- [#141: Cosmic/Linux and web static-axis and inspection parity](https://github.com/gpui-luma-dev/xgpui/issues/141)

Those follow-ups retain these gates:

- Extend the baselines to all CPU caches, real frame times, multiple sizes,
  scale/subpixel variants, and drag-completion behavior. The current Metal
  diagnostic measures isolated atlas allocations, not rendered frames.
- Design budgets and frame-safe invalidation/eviction. Cached scenes retain
  atlas tiles directly; removing glyph keys alone cannot guarantee cached views
  repaint or protect in-flight texture use. Font IDs must remain valid for old
  layouts. Existing removal hooks are not a complete policy.
- Windows lab and unit-test acceptance is confirmed. #140 retains broader
  DirectWrite compatibility/host validation; the five standalone backend fixture
  tests are implemented and cross-checked for MSVC.
  Cosmic arbitrary-axis integration and native/browser validation remains #141;
  the bundled Cosmic interface does not expose arbitrary axis inputs.
- Automatic optical sizing remains explicitly deferred until size-dependent
  instance identity and coordinate mapping are specified and tested.

Do not advertise globally bounded scrubbing based on these isolated diagnostics.
Later stages are tracked in the linked issues.


## Verification on the implementation host

- Core text-system tests: 52 passed, including six private parser/query tests;
  style/refinement serialization test: passed.
- Parley library: 36 passed, one diagnostic ignored in normal runs; that CPU
  diagnostic was run explicitly and passed.
- Apple text system: 9 passed. Metal allocation diagnostic: passed with native
  GPU access, outside the sandbox.
- Text-lab: 10 tests passed on macOS, including editing/IME, paragraph
  interaction, native/Parley slider events, keyboard/presets, and preview limits.
  The native and Parley headless comparison workloads also ran successfully.
- WebAssembly backend: `cargo check` passed for `wasm32-unknown-unknown` with
  default features disabled.
- Windows library and tests: MSVC cross-check passed with `--no-default-features
  --features test-support`. Windows lab and lab-unit-test execution passed by user confirmation on
  November 5, 2026; standalone backend fixture execution is not separately recorded.
  The Windows crate's existing default feature still forwards `gpui/default`;
  its dependency now disables implicit defaults so explicit opt-out works and
  cross-checking does not require the Windows manifest resource compiler.

Tests used the existing runtime-shader feature to avoid writing Metal compiler
cache files outside the sandbox. No dependencies were added.


## Windows implementation and API impact

The Windows backend implements the existing reviewed interfaces; this change
adds no further core GPUI public types, fields, or methods. Its internal native
face handling and metadata queries are new. The `xgpui-windows` feature opt-out
fix described above changes `--no-default-features` behavior deliberately;
ordinary default builds keep forwarding GPUI's defaults.

For repeatable Windows and macOS commands, expected output, fixture tests, and
patch transfer instructions, see [font-controls testing](FONT-CONTROLS-TESTING.md).


## Interactive text-lab inspector

The comparison workload now includes application-local live sliders for `wght`,
`wdth`, `slnt`, and `opsz` on each supporting backend. Presets, keyboard and
accessibility actions share the same `VariationPreview` path as pointer drags.
A fixed 512-descriptor admission limit holds the previous preview on exhaustion;
there is no eviction, font-ID reuse, or unbounded reset. The specimen font and
its advance measurement use the same effective settings. No new core GPUI
public API was added for these controls.


## Release versions

The font-controls release uses these versions and exact internal dependency
requirements. No new crate is published; the parser module is private inside `xgpui`.

| Crate | Previous | Version | Rationale |
| --- | --- | --- | --- |
| `xgpui` | 2.0.1 | **3.0.0** | Breaking fields on `Font`, `TextStyle`, and `TextStyleRefinement`; other APIs are additive. |
| `xgpui-apple` | 3.0.1 | **3.1.0** | New static axes and introspection capabilities; existing public signatures remain intact. |
| `xgpui-windows` | 2.0.1 | **2.1.0** | New static axes and introspection capabilities; existing public signatures remain intact. |
| `xgpui-parley` | 0.3.1 | **0.4.0** | New axes/introspection capabilities and additive `VariationPreview` API; no removed APIs. |
| `xgpui-wgpu` | 2.0.1 | **2.0.2** | Reject unsupported explicit axes and update core pin. |
| `xgpui-web` | 2.0.1 | **2.0.2** | Reject unsupported explicit axes and update core/backend pins. |
| `xgpui-macos` | 3.0.1 | **3.0.2** | Core and Apple dependency-pin updates only. |
| `xgpui-ios` | 1.26.3 | **1.26.4** | Core and Apple dependency-pin updates only. |
| `xgpui-linux` | 2.0.1 | **2.0.2** | Core and wgpu dependency-pin updates only. |
| `xgpui-platform` | 2.0.1 | **2.0.2** | Matching core and backend dependency-pin updates only. |
| `xgpui-media` | 0.5.1 | **0.5.2** | Matching core/backend/shader dependency-pin updates only. |
| `xgpui-shader` | 0.4.1 | **0.4.2** | Matching core/backend dependency-pin updates only. |
| `xgpui-gamut` | 0.3.1 | **0.3.2** | Matching core/backend/shader dependency-pin updates only. |
| `xgpui-canvas` | 0.3.1 | **0.3.2** | Matching core/shader dependency-pin updates only. |
| `xgpui-svg` | 0.3.1 | **0.3.2** | Core dependency-pin updates only. |

`xgpui-color` remains 0.3.0 and `xgpui-macros` remains 1.0.1. The repository's
versioning policy does not automatically propagate major bumps to dependent
crates: these dependents retain their public members/signatures. Their GPUI
inputs, outputs, traits, and re-exports now resolve to the matching 3.0 type
universe, so consumers must upgrade the pinned stack together; mixing this
release family with core 2.x is unsupported. The exhaustive-literal migrations above
remain necessary wherever applications directly construct the changed core types.
