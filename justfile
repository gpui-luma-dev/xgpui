# Run from the repository root. Windows: put Git for Windows Bash on PATH.
set shell := ["bash", "-cu"]
set windows-shell := ["bash", "-cu"]
set positional-arguments

profile := "release"
python := if os() == "windows" { "python" } else { "python3" }
# Lab launcher (#310); see `labs` below.
lab := "cargo run --quiet --locked --manifest-path tools/lab/Cargo.toml --"

# List sample apps and development commands without launching anything.
default:
    @just --justfile "{{ justfile() }}" --list

# Labs (#310) are defined in apps/xgpui-ext-lab/labs.toml. `just <lab> [view|task]`
# runs one directly (`just media tiled`, `just media capture`); `just labs check`
# keeps labs.toml, Cargo.toml and the recipes below in sync.

# Pick a lab, view or task from a menu (also: labs list, build, test, check).
labs *args:
    @{{ lab }} "$@"

# List every lab, view and task for this OS.
list-all-labs:
    @{{ lab }} list

# Desktop preference inspector and per-window shell controls.
shell *args:
    @XGPUI_LAB_DIRECT=1 {{ lab }} shell "$@"

# Spatial canvas with 10,000 selectable cards; Mandelbrot explorer.
canvas *args:
    @XGPUI_LAB_DIRECT=1 {{ lab }} canvas "$@"

# Image mipmaps, filters, GPU surfaces, path effects and video.
media *args:
    @XGPUI_LAB_DIRECT=1 {{ lab }} media "$@"

# Paints, gradients, brushes and layer effects.
paint *args:
    @XGPUI_LAB_DIRECT=1 {{ lab }} paint "$@"

# Native wide-SDR gamut comparison.
gamut *args:
    @XGPUI_LAB_DIRECT=1 {{ lab }} gamut "$@"

# xgpui-curve: mesh wires, spines, nodes, series, easing and shapes.
curve *args:
    @XGPUI_LAB_DIRECT=1 {{ lab }} curve "$@"

# Color swatches and CIE chromaticity.
color *args:
    @XGPUI_LAB_DIRECT=1 {{ lab }} color "$@"

# Native versus Parley text, paragraphs, scrolling and font axes.
text *args:
    @XGPUI_LAB_DIRECT=1 {{ lab }} text "$@"

# SVG rendering, zoom and scheduling.
svg *args:
    @XGPUI_LAB_DIRECT=1 {{ lab }} svg "$@"

# Six shader examples and selectable typography.
shader *args:
    @XGPUI_LAB_DIRECT=1 {{ lab }} shader "$@"

# GPU interop: wgpu engine on its own device presenting into xgpui.
gpu-interop *args:
    @XGPUI_LAB_DIRECT=1 {{ lab }} gpu-interop "$@"

# GPU interop: wgpu's boids example as an independent engine.
interop-boids *args:
    @XGPUI_LAB_DIRECT=1 {{ lab }} interop-boids "$@"

# GPU interop: editable GPU canvas in an independent engine.
gpu-canvas-workbench *args:
    @XGPUI_LAB_DIRECT=1 {{ lab }} gpu-canvas-workbench "$@"

# Quad rendering benchmark.
quad-bench *args:
    @XGPUI_LAB_DIRECT=1 {{ lab }} quad-bench "$@"

# Color-field rendering benchmark.
color-field-bench *args:
    @XGPUI_LAB_DIRECT=1 {{ lab }} color-field-bench "$@"

# Metal custom-producer deadlock regression (macOS).
metal-producer *args:
    @XGPUI_LAB_DIRECT=1 {{ lab }} metal-producer "$@"

# Automated checks in real platform windows (the probes CI runs).
window-probes *args:
    @XGPUI_LAB_DIRECT=1 {{ lab }} window-probes "$@"

# Native font-variation investigation (macOS).
font-variations *args:
    @XGPUI_LAB_DIRECT=1 {{ lab }} font-variations "$@"

# The right-to-left text example from xgpui.dev/right-to-left-text.html.
rtl-example *args:
    @XGPUI_LAB_DIRECT=1 {{ lab }} rtl-example "$@"

# Browser extension gallery (`just extensions-web build`).
extensions-web *args:
    @XGPUI_LAB_DIRECT=1 {{ lab }} extensions-web "$@"

# Numerical, ICC, CIE dataset and color workspace checks (`just color test`).
lab-color-test *args:
    @{{ lab }} color test "$@"

# Build every native lab without launching a window.
lab-build *args:
    @{{ lab }} build "$@"

# Lab unit and compatibility tests; native window tests remain opt-in.
lab-test *args:
    @{{ lab }} test "$@"

[private]
lab-chromaticity *args:
    @{{ lab }} color chromaticity "$@"

# Older names; each forwards views, tasks and app options.
alias lab := labs
alias lab-shell := shell
alias lab-canvas := canvas
alias canvas-lab := canvas
alias lab-media := media
alias media-lab := media
alias lab-paint := paint
alias paint-lab := paint
alias paints := paint
alias lab-gamut := gamut
alias gamut-lab := gamut
alias lab-color := color
alias color-lab := color
alias lab-text := text
alias text-lab := text
alias lab-parley := text
alias parley-lab := text
alias lab-svg := svg
alias svg-lab := svg
alias lab-shader := shader
alias shader-lab := shader
alias shaders := shader
alias lab-gpu-interop := gpu-interop
alias gpu-interop-lab := gpu-interop
alias lab-interop-boids := interop-boids
alias lab-gpu-canvas-workbench := gpu-canvas-workbench
alias lab-quad-bench := quad-bench
alias lab-color-field-bench := color-field-bench
alias chromaticity := lab-chromaticity
alias chromaticity-test := lab-color-test
alias lab-chromaticity-test := lab-color-test

# List registry status for the latest release tag; no uploads.
release-preflight *args:
    @cargo run --quiet --locked --manifest-path tools/release-sync/Cargo.toml -- release-flow preflight "$@"

# Check that tag's packages/builds; no uploads.
release-check *args:
    @cargo run --quiet --locked --manifest-path tools/release-sync/Cargo.toml -- release-flow check "$@"

# Alias for release-check.
release-dry-run *args:
    @cargo run --quiet --locked --manifest-path tools/release-sync/Cargo.toml -- release-flow check "$@"

# Publish the checked tag in dependency order; requires CARGO_REGISTRY_TOKEN.
release-publish *args:
    @cargo run --quiet --locked --manifest-path tools/release-sync/Cargo.toml -- release-flow publish "$@"

# Preview all ignored Cargo outputs, including standalone crates and old app paths.
clean-preview:
    bash scripts/clean_build_outputs.sh

# Remove all ignored Cargo target directories in this checkout.
clean:
    bash scripts/clean_build_outputs.sh --apply

# Build the representative core/platform libraries; does not launch an app.
build *args:
    cargo build --profile {{ profile }} --locked "$@"

# Build every captured library supported by this target (not all feature combinations).
build-full *args:
    cargo build --profile {{ profile }} --locked --features full-capture "$@"

# Check the static website in pages/: links, anchors, assets and legacy routes.
pages-check:
    {{ python }} scripts/check_pages.py

# Serve pages/ at http://localhost:8000 (optional port). Demos need `just pages-demos` first.
pages-serve port="8000":
    {{ python }} -m http.server "{{ port }}" --bind 127.0.0.1 --directory pages

# Build both browser demo collections into pages/examples/; requires pinned wasm-bindgen.
pages-demos:
    bash apps/xgpui-ext-lab/extensions-web/build.sh --release
    bash apps/xgpui-ext-lab/shader-examples/build.sh --release

# Remove the generated demo bundles from pages/examples/, keeping checked-in pages.
pages-demos-clean:
    bash apps/xgpui-ext-lab/extensions-web/clean.sh
    bash apps/xgpui-ext-lab/shader-examples/clean.sh

# Run every test suite for this OS and print a summary (--list, --only <name>).
test *args:
    bash scripts/test_all.sh "$@"

# Clippy under the #280 policy, as CI runs it (optional: upstream | ours).
clippy *args:
    bash .github/scripts/clippy.sh "$@"

# Rounded clip-chain feasibility experiments (#37).
clip-chain-probe *args:
    @XGPUI_LAB_DIRECT=1 {{ lab }} clip-chain-probe "$@"
