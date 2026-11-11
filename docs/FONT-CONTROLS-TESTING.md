# Test font controls

Run commands from the repository root. The lab bundles licensed fonts; no font
installation is needed. See the [font-controls guide](https://xgpui.dev/font-controls.html)
for API usage, backend support and the core 3.0 migration.

## Wasm inspector

Open the [font-controls demo](examples/extensions/?demo=font-controls)
from the website's Text gallery. Use its WebGPU and WebGL2 launch links to check
both renderers. The real GPUI slider component and Parley/Skrifa pipeline execute
in Wasm; the specimens are not CSS text. Check all four sliders, keyboard limits,
presets, measured advances, and axis/feature metadata.

To build and preview the site locally:

```sh
just web-build
just docs
just docs-serve
```

Open `/examples/extensions/?demo=font-controls` on the local server. The default
GPUI web text backend remains separate from this demo's Parley backend.

## Native and Parley comparison

On macOS or Windows:

```sh
just text --backend both
```

Two windows compare native CoreText/DirectWrite with Parley. Each has independent
weight, width, slant and optical-size sliders. Use the same presets to compare
matching settings. Drag a track or focus a slider and use arrows, Shift for
larger steps, Home and End. Confirm both specimen sizes, wrapping and measured
advance update together, without clipping. Default restores the original values.

The inspector uses steps of 10/1/0.25/1 design units and admits 512 combinations
per window. When full, it holds the last preview and displays an explanation.
Previously admitted settings and Default remain usable. Restart for a fresh
session. This is not a global CPU/GPU memory budget or an eviction policy.

Scroll down for the static samples:

- `wdth=50`, upright, `opsz=14`: condensed.
- `wdth=100`, `slnt=-10`, `opsz=14`: visibly slanted.
- `wdth=150`, `slnt=-5`, `opsz=72`: wider, with a larger optical-size design.
- Inter `cv02=2`: a numeric OpenType feature value.

Available axis ranges and GSUB/GPOS tags appear with the samples. Presence does
not guarantee an effect on every string. Check that the editor still supports
selection and typing. Minor native/Parley or display-dependent raster differences
need not indicate a coordinate mismatch; compare the displayed values and presets.

Windows requires the MSVC toolchain, Visual Studio C++ Build Tools and Windows
SDK. DirectWrite static axes require the `IDWriteTextLayout4` runtime probe to
succeed; unsupported runtimes omit the controls instead of pretending to apply
axes. macOS uses the shared Apple font-kit/CoreText implementation.

For headless comparison output:

```sh
just text --backend both --timings-only --iterations 3
```

## Linux: inspection versus editing

```sh
just text --backend both
```

The native Cosmic panel displays Roboto Flex's axis ranges and feature tags but
omits editable sliders and static-axis specimens. The Parley panel applies axes
with the same controls used on macOS, Windows and Wasm. Keep these capabilities
separate: successful inspection does not establish arbitrary-axis shaping support.

The repository validation workflow runs Cosmic fixture tests, the Parley library,
a headless native-host construction/axis smoke check and the shared slider tests
on Ubuntu. This is distinct from interactive Wayland/X11, IME and accessibility
acceptance on a Linux desktop.

## Lab tests

```sh
just lab-test
```

Backtraces are enabled automatically. To isolate slider interaction failures:

```sh
just lab-test live_axis -- --nocapture
```

These tests send actual pointer and keyboard events through GPUI's simulated test
windows, checking endpoint values, focus/keyboard changes, drag release, presets,
layout changes and restored defaults. Windows uses a real DirectWrite text system:
its headless platform deliberately supplies a no-op text backend and is unsuitable
for this test. The preview-capacity test is included in the full lab suite.

## Validation scope

The Wasm inspector was exercised in the local in-app browser on November 5,
2026, using both WebGPU and WebGL2. Checks covered all four pointer sliders,
Home/arrow keyboard changes (including `slnt=-9.75`), the Wide preset and Default
reset. The pinned specimen's 32px advance changed from 337.05px at Default to
446.73px at Wide and returned to 337.05px on reset. Axis ranges, GSUB/GPOS feature
tags and static/numeric-feature specimens rendered through the Wasm pipeline.
These are browser checks on this host, not exhaustive browser/device coverage.

The native/Parley labs and lab unit tests were confirmed working on macOS and
Windows on November 5, 2026. A subtle cross-platform slant appearance difference
was observed in both panels, with matching endpoints, and accepted as non-blocking.

Implementation-host checks also passed 52 core text tests (including six private
parser/query tests), nine Apple text-system tests, 36 Parley tests and ten text-lab
tests. Core tests distinguish unsupported queries, absent tables, read failures
and malformed metadata. Backend fixtures cover resolved-face inspection, signed
axes, clamping, fallback, stable identity and layout/raster agreement. Five
standalone DirectWrite fixture tests and the lab test targets cross-compile for
MSVC; separate native execution of the standalone fixture suite is not recorded.

The CPU sweep and Metal allocation baselines are documented in
[FONT-CONTROLS.md](FONT-CONTROLS.md#preview-policy-and-measured-baseline). They do
not establish globally bounded scrubbing, frame-safe eviction, exhaustive runtime
compatibility, physical-display equivalence or complete IME acceptance.
