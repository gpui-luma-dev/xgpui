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

# Preview all ignored Cargo outputs, including standalone crates and old app paths.
clean-preview:
    {{ python }} scripts/clean_build_outputs.py

# Remove all ignored Cargo target directories in this checkout.
clean:
    {{ python }} scripts/clean_build_outputs.py --apply

# Experimental spatial viewport with 10,000 selectable cards.
canvas-lab *args:
    cargo run {{ options }} --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin canvas-lab -- "$@"

# Image mipmapping, filters, GPU surfaces and path effects (generated samples).
media *args:
    cargo run {{ options }} --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin media-lab -- "$@"

# Six shader examples and typography; select an example in the window.
shaders *args:
    cargo run {{ options }} --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin shader-examples -- "$@"

# Paints, gradients, brushes and layer effects.
paints *args:
    cargo run {{ options }} --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin paint-lab -- "$@"

# Native wide-SDR gamut comparison.
gamut *args:
    cargo run {{ options }} --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin gamut-lab -- "$@"

# Color conversion and sRGB/P3 reference swatches.
color *args:
    cargo run {{ options }} --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin color-lab -- "$@"

# Native versus Parley text, paragraphs and scrolling.
text *args:
    cargo run {{ options }} --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin text-lab -- "$@"

# SVG rendering, zoom and scheduling comparisons.
svg *args:
    cargo run {{ options }} --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin svg-lab -- "$@"

# Quad rendering benchmark (bounded run by default).
quad-bench *args:
    cargo run {{ options }} --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin quad-bench -- "$@"

# Color-field rendering benchmark (bounded run by default).
color-field-bench *args:
    cargo run {{ options }} --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin color-field-bench -- "$@"

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
    {{ python }} scripts/stage_website.py
    {{ python }} -m http.server "{{ port }}" --bind 127.0.0.1 --directory target/site

# Build both browser demo collections; requires pinned wasm-bindgen and Python 3.
web-build:
    bash apps/xgpui-ext-lab/extensions-web/build.sh --release
    bash apps/xgpui-ext-lab/shader-examples/build.sh --release

# Build every native extension lab and benchmark without launching a window.
lab-build *args:
    cargo build {{ options }} --manifest-path apps/xgpui-ext-lab/Cargo.toml --bins "$@"

# Run unit and compatibility tests; native window tests remain opt-in.
lab-test *args:
    cargo test --locked --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --bins --test compatibility --target-dir target "$@"
