# Test font controls

Run commands from the repository root. Fonts are bundled fixtures; no font
installation is needed. The comparison has live axis sliders and static samples;
it is not a frame-time stress test.

## macOS: visual comparison

```sh
cargo run --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin text-lab --features runtime-shaders -- --backend both
```

Two windows compare native CoreText and Parley. The live sliders are at the top:
weight, width, slant, and optical size. Each window has independent controls;
use the same presets to compare matching settings. Drag the tracks, or focus a
slider and use arrows (Shift for larger steps), Home, and End. Confirm both
specimen sizes and the advance measurement change without clipping. Default
restores the original settings.

There is also a justfile shortcut and an interaction-test recipe:

```sh
just --justfile apps/xgpui-ext-lab/text-lab/justfile axes
just --justfile apps/xgpui-ext-lab/text-lab/justfile axes-test
```

The live inspector quantizes design coordinates in steps 10/1/0.25/1 and admits
512 combinations per window. On exhaustion it holds the last preview; revisiting
old settings still works. Restart for a fresh session. This does not claim a
global CPU/GPU memory budget.

Scroll farther down for the static Roboto Flex rows:

- `wdth=50`, upright, `opsz=14`: condensed.
- `wdth=100`, `slnt=-10`, `opsz=14`: visibly slanted.
- `wdth=150`, `slnt=-5`, `opsz=72`: wider, with larger optical-size design.
- Inter `cv02=2`: exercises a numeric feature value.

Axis ranges and available GSUB/GPOS tags appear above the samples. Feature
presence does not guarantee an effect on every string. Confirm glyphs are not
clipped, rows differ, and the editor still supports selection and typing.

For a headless smoke test, append `--timings-only --iterations 3`.

## Windows: fixture tests first

Use a current Windows 10/11 host with Rust's MSVC toolchain and the Visual Studio
C++ Build Tools/Windows SDK. The tests explicitly require DirectWrite's
`IDWriteTextLayout4`; older runtimes report unsupported capabilities.

In PowerShell, from the repo root:

```powershell
cargo test --config .cargo/validation.toml --manifest-path gpui/crates/gpui-windows/Cargo.toml --no-default-features --features test-support --lib font_variation -- --nocapture
```

Expected: **five tests pass**:

- Owned in-memory font bytes survive registration and later resource use.
- GSUB/GPOS features, axis ranges, invalid IDs, and numeric feature parameters.
- Negative slant, axis-order equivalence, explicit weight precedence, clamping,
  requested/resolved coordinates, advance agreement, and retained raster output.
- Explicit `opsz=72` stays fixed at different sizes; signed fractional axes and
  an unsupported tag retain the specified semantics.
- A static primary passes axes to a bundled variable fallback; unsupported
  primary axes remain ignored.

These tests use native DirectWrite and grayscale rasterization without a D3D
GPU. They have been cross-checked for MSVC on macOS; native execution on Windows
has not yet been performed. Please retain the complete output if a test fails.

## Windows: native/Parley comparison

```powershell
cargo run --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin text-lab -- --backend both
```

Use the same sliders, presets, and visual checks as macOS. The native window
exercises DirectWrite axes; the other window uses Parley. Small glyph positioning/raster differences
between backends are expected. A message saying explicit axes are unavailable
means the DirectWrite runtime probe failed; the axis rows will be omitted.

For headless output:

```powershell
cargo run --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin text-lab -- --backend both --timings-only --iterations 3
```

Keep debug builds for the initial Windows test. Release builds additionally use
the repository's existing shader-compilation requirements.

## Portable patch for another checkout

The accompanying `xgpui-font-controls-windows.patch` contains the entire local
font-controls change, including the core contract, both existing backends,
DirectWrite, fixtures, documentation, and tests. It excludes the unrelated
untracked `docs/COMPARISON.md`. It is based on commit:

```text
3d2a8484255fdd18473f2f98bf7572a858a9fe64
```

Apply it to a clean checkout of that commit, rather than a checkout that already
contains portions of these changes:

```powershell
git switch -c codex/font-controls-test 3d2a8484255fdd18473f2f98bf7572a858a9fe64
git apply --check C:\path\to\xgpui-font-controls-windows.patch
git apply C:\path\to\xgpui-font-controls-windows.patch
```

## Automated tests on macOS

The private core parser/query tests check table parsing and preserve unsupported,
missing-table, read-error, and malformed-table results. The pinned-font backend
tests exercise the complete native-table-access → core-parser query path.

```sh
cargo test --config .cargo/validation.toml --manifest-path gpui/crates/gpui/Cargo.toml --features gpui_platform/runtime_shaders --lib font_inspection
cargo test --config .cargo/validation.toml --manifest-path gpui/crates/gpui/Cargo.toml --lib text_system --features gpui_platform/runtime_shaders
cargo test --config .cargo/validation.toml --manifest-path extensions/crates/xgpui-parley/Cargo.toml --lib --features native-test,gpui-platform/runtime_shaders
cargo test --config .cargo/validation.toml --manifest-path gpui/crates/gpui-apple/Cargo.toml --lib text_system::tests --features font-kit,runtime_shaders
cargo test --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin text-lab --features runtime-shaders
```

The separate CPU sweep and Metal allocation diagnostic commands and measured
baselines are in [FONT-CONTROLS.md](FONT-CONTROLS.md#preview-policy-and-measured-baseline).
Neither is a guarantee of globally bounded scrubbing or frame-safe eviction.


The DirectWrite implementation follows Microsoft's
[axis-aware font selection model](https://learn.microsoft.com/en-us/windows/win32/directwrite/font-selection).
Its runtime capability probe and the native tests are preferable to assuming
support from a Windows version string alone.
