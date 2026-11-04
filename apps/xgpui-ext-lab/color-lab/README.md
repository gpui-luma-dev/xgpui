# Color lab

One native app contains **Swatches** and **Chromaticity** workspaces. Switch tabs
without losing either workspace's settings. `lab-gamut` remains the separate
native wide-output/compositing lab.

```sh
just lab-color                         # Swatches, the original default.
just lab-color --chromaticity          # CIE xy/u′v′, storage/ICC inspector.
just lab-color --mode chromaticity --uv
just lab-color --native-fill
just lab-color --icc-sampled
just lab-color --photo                 # WebKit Italy native-wide/sRGB comparison.
just lab-color --help
just lab-color-test             # Numerical, fixtures and native tests.
```

The compatibility recipes `lab-chromaticity`, `chromaticity` and
`chromaticity-test` now use this checkout. No sibling repository, network assets,
Python or Node is needed. The binary remains `color-lab`; its source is now here
rather than under gamut-lab. The source build uses the shared lab manifest,
lockfile and target directory. Override `profile=dev` when iterating.

Chromaticity options automatically select that workspace. An explicit
`--mode swatches` cannot be combined with them. `--effects` enables the existing
nested wide capture/blur diagnostic on macOS Metal; it fails explicitly on other
hosts instead of silently ignoring the request. Controls in the window retain
their existing platform capability checks and report effective output/fallback.

## What to inspect

- Swatches compare the existing encoded/perceptual interpolation presets in
  mapped sRGB and native Display P3, with mapping and alpha controls.
- Chromaticity compares CIE xy/u′v′, RGB primaries and transfer functions,
  simulated gamut/luminance fills, FP16 storage and ICC interpretation.
- Hidden-symbol and Italy photo comparisons remain accessible inside the
  chromaticity workspace. Its Metal GPU check is a composition readback, not a
  display measurement. Native-wide captures/readback remain platform limited.
- Change a chromaticity setting, switch to Swatches, then return: the setting
  should be retained. The chart and previews should load without errors.

The CIE dataset and WebKit photos reuse the exact existing files and attribution
under `../extensions-web/assets/colorimetry` and `../extensions-web/assets/italy`.
ICC fixtures reuse `extensions/crates/xgpui-color/tests/reference`, including the
Compact profiles' CC0 notice and sRGB2014 attribution. The source never fetches
profiles or images at runtime. Errors are printed to stderr and appended to the
system temporary directory's `xgpui-color-lab-errors.log`.

## Integration provenance and verification

The chromaticity workspace and its numerical/photo/GPU reference tests were
integrated from `gpui-luma-dev/xgpui-lab` commit
`9dd64c96c5e04ae694e4d1ad0f33e2aad4312967` (`src/chromaticity.rs` and
`src/bin/chromaticity-lab/*.rs`), under the existing Apache-2.0 code license.
Only app code was brought over; the sibling checkout is unchanged. Launch flags
are parsed by this app and passed explicitly into the workspace. Shared asset
paths and attribution replace duplicate fixture copies.

The integrated app passes 24 tests on macOS, including independent numerical,
ICC/FP16, pinned WebKit asset and Metal photo-composition checks. The color binary
and tests also cross-check for Windows. Live macOS verification covers both tabs,
the chromaticity fill and xy/u′v′ setting retention across tab switches. The owner additionally confirmed all native labs and `just lab-test` passed on
Windows on 2026-11-04; see [Windows validation](../../../docs/LOCAL_DEVELOPMENT.md#windows-validation).
Linux runtime and monitor color accuracy remain separate.
