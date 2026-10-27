# Run from the repository root. Windows: put Git for Windows Bash on PATH.
set shell := ["bash", "-cu"]
set windows-shell := ["bash", "-cu"]
set positional-arguments

profile := "release"
python := if os() == "windows" { "python" } else { "python3" }
options := "--profile " + profile + " --locked --config .cargo/validation.toml --target-dir target"

# List sample apps and development commands without launching anything.
default:
    @just --justfile "{{ justfile() }}" --list

# Image mipmapping, filters, GPU surfaces and path effects (generated samples).
media *args:
    cargo run {{ options }} --manifest-path apps/media-lab/Cargo.toml -- "$@"

# Six shader examples and typography; select an example in the window.
shaders *args:
    cargo run {{ options }} --manifest-path apps/shader-examples/Cargo.toml -- "$@"

# Paints, gradients, brushes and layer effects.
paints *args:
    cargo run {{ options }} --manifest-path apps/paint-lab/Cargo.toml -- "$@"

# Native wide-SDR gamut comparison.
gamut *args:
    cargo run {{ options }} --manifest-path apps/gamut-lab/Cargo.toml --bin gamut-lab -- "$@"

# Color conversion and sRGB/P3 reference swatches.
color *args:
    cargo run {{ options }} --manifest-path apps/gamut-lab/Cargo.toml --bin color-lab -- "$@"

# Native versus Parley text, paragraphs and scrolling.
text *args:
    cargo run {{ options }} --manifest-path apps/text-lab/Cargo.toml -- "$@"

# SVG rendering, zoom and scheduling comparisons.
svg *args:
    cargo run {{ options }} --manifest-path apps/svg-lab/Cargo.toml -- "$@"

# Quad rendering benchmark (bounded run by default).
quad-bench *args:
    cargo run {{ options }} --manifest-path apps/quad-bench/Cargo.toml -- "$@"

# Color-field rendering benchmark (bounded run by default).
color-field-bench *args:
    cargo run {{ options }} --manifest-path apps/color-field-bench/Cargo.toml -- "$@"

# Build the representative core/platform libraries; does not launch an app.
build *args:
    cargo build --profile {{ profile }} --locked "$@"

# Build every captured library supported by this target (not all feature combinations).
build-full *args:
    cargo build --profile {{ profile }} --locked --features full-capture "$@"

# Generate the documentation inventory and embedded guides.
docs:
    cargo run --locked --manifest-path tools/crate-roadmap/Cargo.toml

# Verify generated documentation (requires Node.js).
docs-check: docs
    node docs/verify.mjs

# Serve generated documentation at http://localhost:8000 (optional port argument).
docs-serve port="8000":
    {{ python }} -m http.server "{{ port }}" --bind 127.0.0.1 --directory docs

# Build both browser demo collections; requires pinned wasm-bindgen and Python 3.
web-build:
    bash apps/extensions-web/build.sh --release
    bash apps/shader-examples/build.sh --release
