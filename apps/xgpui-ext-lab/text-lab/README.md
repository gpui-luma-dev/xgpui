# Text lab

Native/Parley comparison, virtualized scrolling, and paragraph editing in one app.
GPUI selects text backends per process; comparison mode launches two windows.
All windows start at 1200 logical pixels high.

## Run

From the repository root:

```sh
just --justfile apps/xgpui-ext-lab/text-lab/justfile ui
just --justfile apps/xgpui-ext-lab/text-lab/justfile timings
just --justfile apps/xgpui-ext-lab/text-lab/justfile scroll
just --justfile apps/xgpui-ext-lab/text-lab/justfile scroll --scroll-frames 640 --exit-after-scroll
just --justfile apps/xgpui-ext-lab/text-lab/justfile paragraph
just --justfile apps/xgpui-ext-lab/text-lab/justfile paragraph --timings-only
just --justfile apps/xgpui-ext-lab/text-lab/justfile test
just --justfile apps/xgpui-ext-lab/text-lab/justfile lint
```

`ui` uses debug; timing, scroll and paragraph recipes use release. Common flags:
`--backend native|parley|both`, `--iterations 31`, `--raster-scale 1`, `--lines 5000`.
`--timings-only` and `--exit-after-scroll` run comparison backends sequentially.
Paragraph mode selects Parley; explicit native requests return Unsupported.

## Modes

| Mode | Coverage | Controls |
| --- | --- | --- |
| Compare | Bundled Plex/Inter; ligatures, combining marks, real fallback, weights 100/400/900; shared single-line editor | Selection, clipboard, grapheme deletion, IME, horizontal caret scrolling |
| Scroll | GPUI `uniform_list`; 100–100,000 distinct lines; normal caches/atlas | Run/pause, 18/24px, weights 400/900 |
| Paragraph | Mixed Arabic/Hebrew/Latin, numbers, wrapping, U+2028, NBSP/ZWSP; bundled RTL fonts | Click/drag, visual arrows, Shift selection, typing, clipboard, IME, width toggle |

The scroll script advances eight rows/frame through 18/400, 24/400, 24/900,
18/400. Reported row counts include virtualizer measurement/overscan. UI mode
performs no corpus pre-shaping; inactive-window throttling is disabled. Restart
for a cold run. The weighted corpus uses Inter-covered characters; synthetic
fallback styles remain unsupported.

Paragraph Enter/paste normalizes line endings to U+2028. Home/End address the
source; Up/Down preserve x. Next sample resets edits. Undo/redo is not implemented.

## Interop

The single-line editor reuses `gpui/examples/input.rs`. Paragraphs use the same
GPUI input bridge with Parley geometry:

```rust,ignore
let layout = window.text_system().layout_paragraph(text, px(24.0), &runs, width)?;
let hit = layout.hit_test(mouse_position - bounds.origin)?;
let fragments = layout.selection_bounds(anchor, hit)?;
// During paint:
window.handle_input(&focus, ElementInputHandler::new(bounds, entity), cx);
gpui::paint_paragraph(layout.as_ref(), bounds.origin, color_at, window)?;
```

IME callbacks translate UTF-16/source UTF-8, including candidate-relative
selections and CRLF folding. Candidate anchors use the requested caret/fragment;
marked underlines use selection fragments. Edits invalidate painted geometry;
keyboard navigation can obtain the current revision through GPUI's cache.
Handled keys stop propagation to prevent duplicate insertion. Active composition
keys remain with the IME. Caret reveal uses GPUI prepaint autoscroll.

## Measurements

| Metric | Scope |
| --- | --- |
| Compare first/warm | First source request / median exact layout cache hit; shared backend state |
| Raster first/warm | Direct bounds + mask generation per unique glyph; bypasses GPUI bounds/atlas caches |
| Scroll corpus | Full first/cached CPU passes; retains all measured layouts |
| Scroll UI | Full-view CPU draw and recorded submission-interval p50/p95/max; includes profiler overhead |
| Paragraph | First source at 320px / exact cache-hit median / new-width reflow median |
| RSS | Whole process; excludes separate GPU allocations |

Warm layout requests verify Arc identity. Paragraph reflow uses distinct widths
`180 + i*0.1`px and retained shaping; it includes layout cloning and glyph
conversion. Font resolution is excluded. Diagnostics use a separate window cache;
UI layout caches start empty but backend state remains warm. Edited text does not
update the displayed seed timings.

CPU diagnostics exclude GPU/presentation. Submission intervals cover frames
classified as animating and are not displayed FPS. Raster diagnostics default to
1x grayscale; live rendering uses display scale. System-font discovery is enabled
for IME fallback and can affect first-use cost.

One macOS/aarch64 release run, sequential 640-frame runs, 2x scale, 5,000 rows:

| Backend | First corpus pass | Cached pass | CPU draw p95 | Final scroll RSS |
| --- | ---: | ---: | ---: | ---: |
| Native | 663.45ms | 0.48ms | 5.95ms | 111.1 MiB |
| Parley | 144.59ms | 0.51ms | 3.37ms | 112.9 MiB |

Corpus and UI are separate runs. Native optical-size policy differs from Parley;
this is not a pixel-equivalent comparison or a cross-platform ranking.

The macOS native fix preserves CTFont variations, features and fallback identity
while instantiating `wght`. Native optical-size behavior is unchanged. The
`native_variations` example tests IDs, advances and masks; pinning `opsz=14` there
matches Parley's default-location widths.

## Validation

Automated tests cover cache reuse/invalidation, mixed-script geometry,
selection/clipboard, grapheme deletion, UTF-16 surrogate/combining selections,
marked candidate replacement, prefix preservation, commit/cancel callbacks,
stale geometry and wrapped candidate anchors. These do not open OS candidate UI.

Manual macOS/aarch64 results (2026-10-24):

| Surface | Result |
| --- | --- |
| Single-line, native + Parley | Japanese–Romaji candidate selection, commit/cancel and popup placement: user-confirmed pass |
| Paragraph, Parley | Typing/paste, visual selection/reflow and accent composition/commit: checked |
| Paragraph U.S. accent panel | Placement, candidate selection, commit and Escape cancel: user-confirmed pass; keyboard settings unchanged |

Remaining: paragraph conversion IMEs, Windows/Linux runtime/IME, accessibility,
and broader BiDi/UAX #14 conformance. See [adapter limits](../../../extensions/xgpui-ext/crates/xgpui-parley/README.md).

## Windows and Linux host testing

The shared adapter uses GPUI's Windows/DX11 or Linux/Wayland/X11 host. Native
window, clipboard, input and GPU initialization stay with GPUI. Windows needs
Visual Studio C++ tools and the Windows SDK (`fxc.exe` for release shaders);
Linux needs native build prerequisites and a Vulkan driver/display session.
A macOS Windows cross-check stopped at missing `llvm-rc`; no Windows build pass
is claimed.

PowerShell/Linux shell, from the repository root; omit `--offline` on first use:

```sh
cargo test --locked --config .cargo/validation.toml --manifest-path extensions/xgpui-ext/crates/xgpui-parley/Cargo.toml --target-dir target --features native-test --test construction
cargo test --locked --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin text-lab --target-dir target
cargo run --release --locked --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin text-lab --target-dir target -- --workload paragraph
```

Use `--backend both` for comparison or `--workload scroll --scroll-frames 640
--exit-after-scroll` for sequential profiling. Test Wayland and X11 separately.
Report OS/display server, scale, GPU/driver and terminal output.

Font licenses: repository Plex assets; [Inter and Noto fixtures](../../../extensions/xgpui-ext/crates/xgpui-parley/tests/fonts/README.md).
