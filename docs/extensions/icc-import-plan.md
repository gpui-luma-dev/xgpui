# Pixels and color metadata — #85 Stage C

Status: pixels + metadata foundation implemented locally; ICC remains unimplemented. Draft #88/#89 remain separate review units; neither is merged. Windows/Linux native acceptance remains outstanding.

## Boundary

An image entering the rendering pipeline is pixels plus explicit metadata. File formats, file paths, metadata discovery and missing-profile decisions belong to the application or a separate adapter. Do not expand `MediaLoader::load` into a general photo importer.

| Component | Owns | Does not own |
| --- | --- | --- |
| Color core | Validated RGB definitions, transfer evaluation, white-point adaptation, linear conversion and explicit gamut mapping | Files, compressed images, ICC discovery or monitor selection |
| Media core | Pixel layout and precision, alpha semantics, bounded conversion jobs, mip generation, retained textures and rendering | A growing catalog of image decoders |
| Optional ICC adapter | Profile bytes to a validated RGB definition or reusable transform; explicit unsupported-profile errors | Image containers or file access |
| Application adapters | Decode files, extract metadata, choose missing-profile policy, apply orientation and supply pixels | Rendering internals |

The existing PNG/JPEG/WebP convenience loader remains unchanged during this work. Its presence does not establish the future core boundary. Audit its consumers before moving it into an optional adapter; update xgpui-lab together, without compatibility aliases. No additional file formats are planned.

## Foundation implementation

`xgpui-color::RgbDefinition` now validates custom D65 colorants with independent linear, sRGB or gamma channel curves. `PixelColor` resolves named, custom bounded, or extended-linear input. `MediaLoader::load_pixel_buffer` and synchronous `pixels_rgba16f_data` accept tightly packed RGBA f32 buffers and explicit P3/Rec.2020 storage/mapping. Retained images preserve the resolved metadata. No codec or ICC dependency was added.

The foundation accepts already-adapted D65 colorants. A follow-up adds explicit Bradford adaptation between the CSS D50/D65 reference whites and a constructor that adapts declared D50 colorants once. Sampled/ICC parametric curves, arbitrary/encoded profile-white handling, other layouts/precisions and ICC parsing remain future work. Matrix white admission allows 3/65536 per XYZ channel for quantized colorants; conversion tests use much tighter scale-aware f64 or one-step FP16 bounds.

## Original API audit

`ImageColor` already separates named primaries, transfer and alpha association. `load_rgba16f_with_mapping` and `load_rec2020_rgba16f` already accept supplied pixels and explicit mapping on bounded workers. Those are useful foundations.

The original audit identified two seams, addressed by this foundation:

1. `ImageColor` describes only named D65 primaries and shared named channel curves. The new custom definition supplies validated D65 colorants and per-channel curves; the caller explicitly declares the matrix white. ICC tag interpretation remains an adapter responsibility. Profile names must never select conversion behavior.
2. The original supplied-pixel ingestion accepted only bounded source RGB. A reusable ICC transform can produce negative or above-one linear channels. That output must enter a validated extended-linear path before explicit destination mapping; passing it through the bounded source decoder would reject it or encourage premature clipping.

Do not disguise custom profiles as `RgbSpace` enum variants. Keep `ImageColor` as a convenient named interpretation, and design its relationship to an owned, validated custom definition deliberately. Both new routes are exercised by numerical fixtures.

## Implementation order

1. Foundation implemented for tightly packed RGBA f32: dimensions, layout/stride, precision, color interpretation and alpha association. Document ownership and admission limits. Reuse existing types where sufficient; do not add speculative formats.
2. Custom conversion and extended ingestion implemented with synthetic matrix/curve fixtures, independently of ICC parsing. The follow-up defines explicit reference-white adaptation once. Carry finite extended-linear RGB until the caller's explicit storage mapping; associate alpha afterward and generate mips in linear light.
3. Evaluate an existing ICC implementation behind an optional adapter. Accept profile bytes, build a reusable transform once, and process bounded rows on admitted workers. The core must remain usable without that dependency.
4. If the dependency needs patches or a maintained fork, prefer owning a deliberately limited RGB matrix/TRC reader. Support only the validated subset, reject everything else clearly, and document the maintenance cost. Do not implement a general ICC engine by default.
5. Exercise the boundary in xgpui-lab. The app can decode a small fixture set and extract profiles; the library receives pixels + metadata. Report interpretation, storage and mapping separately from display capability.

This design selects no ICC dependency and adds no loader API. Existing backend acceptance remains a separate task.

## ICC adapter scope and policy

The initial target is SDR RGB matrix/TRC profiles. LUT profiles, CMYK, gray and HDR remain explicitly unsupported until a concrete consumer requires them. Choose accepted profile classes, ICC versions, curve types and adaptation tags from fixture coverage before claiming support.

Missing metadata is an application decision: assume sRGB and report it, require a profile, or supply a known interpretation. Malformed or unsupported embedded profiles produce errors; they do not silently become sRGB. The core always receives a resolved interpretation. Monitor ICC profiles remain part of platform presentation.

If we own a reader, it must validate tag bounds/overlap policy, declared sizes, integer overflow, curve monotonicity and finite invertible matrices; bound table sizes and reject unsupported semantics. Parser safety and numerical correctness both become our responsibility. A small parser still needs malformed-input tests and fuzzing.

Budget responsibilities follow the boundary: application adapters own encoded bytes and decoder scratch; an ICC adapter owns profile/table/transform allocations; media owns admitted conversion scratch and retained mips. Count peak simultaneous allocations, check cancellation between rows, and avoid unbounded caches.

## Conversion engine investigation

The locally cached `moxcms` 0.8.1 is a candidate, not selected. Its upstream project documents pure Rust ICC conversion: <https://github.com/awxkee/moxcms>. No product dependency was added.

A standalone exploratory probe is retained in `validation/icc-import-probe`. It creates built-in Adobe RGB and Display P3 profiles, replaces destination TRCs with gamma 1, disables CICP transfer substitution, and requests relative colorimetric floating-point conversion.

```sh
cargo run --offline --manifest-path validation/icc-import-probe/Cargo.toml
cargo run --offline --manifest-path validation/icc-import-probe/Cargo.toml -- --extended
```

Both commands currently fail the Adobe-green sign assertion. Without extended range the result clips negative P3 channels. With extended range, this custom destination setup returned Adobe green approximately `[-1.9405562, 0.9793242, 0.1775280]`, inconsistent with the expected primary conversion. This is an unresolved setup/engine investigation, not a confirmed upstream defect or a reason to patch upstream. Investigate profile construction, chromatic adaptation and transform options first. Passing white or a round trip is insufficient. The probe is intentionally outside product tests and dependencies.

## Numerical acceptance

Use analytical high-precision matrix/TRC references for synthetic RGB definitions and a second established ICC engine for serialized profile fixtures. Cover D50 PCS to D65 adaptation explicitly. Serialized fixed-point colorants and TRCs introduce quantization; derive tolerances from profile bytes and transform precision rather than reusing ideal named-space tolerances.

Cover saturated Adobe green/cyan, P3 red, neutrals, dense near-black ramps, fractional/zero alpha and transparent-edge mipmaps. Test source precision, per-channel curves, invalid custom definitions, malformed/truncated profiles, unsupported profiles, cancellation and admission limits. Inspect pre-mapping float values and final FP16 bytes separately. Negative/above-one intermediates must survive until explicit mapping.

At the application boundary, a tagged/untagged fixture pair with identical pixel bytes must demonstrate different resolved interpretations. File decoding evidence, ICC conversion evidence and native display delivery remain separate. Keep #85 open until remaining backend and color-metadata acceptance is recorded.

## Foundation validation — 2026-10-30

20 color unit tests, 6 independent Adobe numerical tests, 68 serial media tests with Metal/wgpu, and 8 sibling chromaticity-lab tests pass. The custom definition path checks the existing 1,000-vector 120-digit oracle against actual FP16 storage (at most one representable step, exact zeros). Additional synthetic rational XYZ references check per-channel curves, extended input, nonfinite/overflow rejection, singular matrices, alpha association, cancellation and background source metadata. Strict color all-target and media library Clippy pass. Oracle regeneration and diff whitespace checks pass.

These results establish CPU conversion/storage behavior and existing macOS renderer regressions. No ICC engine, ICC white/tag interpretation or new Windows/Linux native validation is claimed.

## Explicit reference-white adaptation follow-up

`WhitePoint::{D50,D65}`, `BradfordAdaptation`, and `RgbDefinition::new_with_white_point` add explicit adaptation without any file/profile dependency. D50/D65 use the CSS chromaticities, not arbitrary encoded ICC PCS illuminant values. Adapters must resolve PCS normalization and any existing adaptation before selecting the matrix white. No profile-name inference is performed.

The 120-digit oracle in `scripts/generate_bradford_references.py` uses a pivoted Gauss-Jordan inverse independent of production's cofactor inverse. It covers 408 signed, zero, near-black and large XYZ vectors in both directions. Scale-aware bounds account for cancellation while constructing matrix off-diagonals; there is no ordinary absolute-error floor that hides dark errors. A second check uses the published CSS matrix. D50 diagonal-colorant fixtures independently verify explicit mapping and associated FP16 storage. Identity is exact; nonfinite inputs and overflow are rejected. Source colorant quantization admission is propagated through the CAT separately from numerical conversion tolerances.

Follow-up validation: 20 color unit tests, 4 adaptation tests, 6 independent Adobe numerical tests, 69 serial Metal/wgpu media tests and 8 sibling lab tests pass. Strict color all-target and media library Clippy, formatting, fixture regeneration and diff checks pass. Windows/Linux native execution remains unverified.
