# Run from the repository root. Windows: put Git for Windows Bash on PATH.
set shell := ["bash", "-cu"]
set windows-shell := ["bash", "-cu"]
set positional-arguments

profile := "release"
python := if os() == "windows" { "python" } else { "python3" }
options := "--profile " + profile + " --locked --config .cargo/validation.toml --target-dir target"
lab-features := "--features runtime-shaders,portable-shaders,wgpu"

# List sample apps and development commands without launching anything.
default:
    @just --justfile "{{ justfile() }}" --list

# List all lab demos, alternate views and diagnostics with copyable run commands.
list-all-labs:
    #!/usr/bin/env bash
    cat <<'LABS'
    All commands below run from the repository root.
    Use any lab command with --help to see its full options.

    Main interactive labs
      Linux shell / preferences    just lab-shell
      Canvas / spatial cards       just lab-canvas
      Media / mipmaps              just lab-media
      Paints / gradients           just lab-paint
      Gamut comparison             just lab-gamut
      Color swatches               just lab-color
      Text / native and Parley     just lab-text
      SVG comparison               just lab-svg
      Shader examples              just lab-shader

    Additional interactive views
      Mandelbrot explorer          just lab-canvas --mandelbrot
      Tiled image canvas           just lab-media --tiled
      Floating-point images        just lab-media --float-images
      Image adjustments            just lab-media --adjustments
      Path effects                 just lab-media --paths
      Synthetic video surfaces     just lab-media --video
      Standard SDR gamut baseline  just lab-gamut --sdr
      Instanced gamut fills        just lab-gamut --instanced
      Instanced gamut gradients    just lab-gamut --instanced --gradient
      CIE chromaticity             just lab-color --chromaticity
      CIE u-prime/v-prime          just lab-color --uv
      Italy photo comparison       just lab-color --photo
      Parley text only             just lab-text --backend parley
      Paragraph comparison         just lab-text --workload paragraph
      Text scrolling               just lab-text --workload scroll
      Text CPU timings             just lab-text --timings-only
      SVG artwork gallery          just lab-svg --gallery apps/xgpui-ext-lab/svg-lab/samples

    Shader views
      Minimal shader               just lab-shader --example minimal
      Animated glow badges         just lab-shader --example glow
      Particles                    just lab-shader --example particles
      Composition / clipping       just lab-shader --example composition
      Shadows                      just lab-shader --example shadows
      Borders                      just lab-shader --example borders
      Selectable typography        just lab-shader --example typography

    Paint views
      Linear gradients             just lab-paint --effect linear
      Radial gradients             just lab-paint --effect radial
      Conic gradients              just lab-paint --effect conic
      Diamond gradients            just lab-paint --effect diamond
      Hermite mesh                 just lab-paint --effect mesh
      Pastel waves                 just lab-paint --pastel-waves

    Benchmarks and diagnostics
      Quad benchmark               just lab-quad-bench
      Color-field benchmark        just lab-color-field-bench
      Quad CPU benchmark           just lab-quad-bench --headless --cpu-only
      Color-field CPU benchmark    just lab-color-field-bench --headless --cpu-only
      Media performance recipes    just --justfile apps/xgpui-ext-lab/media-lab/justfile --list
      SVG scheduling benchmarks    just --justfile apps/xgpui-ext-lab/svg-lab/justfile --list
      Font variations [macOS only] cargo run --release --locked --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --example native_variations --target-dir target
      Metal producer [macOS only]  cargo run --release --locked --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin metal-producer-regression --target-dir target

    Browser galleries (requires Wasm tooling)
      Build both galleries         just web-build
      Serve documentation/gallery  just docs-serve

    Native wide-gamut views use SDR fallback where available; native P3 output
    requires a capable backend and display. Linux video requires Vulkan DMA-BUF.
    App-level justfiles also list specialized captures, tests and diagnostics.
    LABS

# Detect changed crates needing version bumps or dependency pin updates.
release-check *args:
    cargo run --locked --manifest-path tools/release-sync/Cargo.toml -- release-check "$@"

# Preview all ignored Cargo outputs, including standalone crates and old app paths.
clean-preview:
    bash scripts/clean_build_outputs.sh

# Remove all ignored Cargo target directories in this checkout.
clean:
    bash scripts/clean_build_outputs.sh --apply

# Desktop preference inspector and per-window shell override/state controls.
lab-shell *args:
    cargo run {{ options }} --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin shell-lab -- "$@"

# Experimental spatial viewport with 10,000 selectable cards.
lab-canvas *args:
    cargo run {{ options }} {{ lab-features }} --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin canvas-lab -- "$@"

# Image mipmapping, filters, GPU surfaces and path effects (generated samples).
lab-media *args:
    cargo run {{ options }} {{ lab-features }} --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin media-lab -- "$@"

# Six shader examples and typography; select an example in the window.
lab-shader *args:
    cargo run {{ options }} {{ lab-features }} --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin shader-examples -- "$@"

# Paints, gradients, brushes and layer effects.
lab-paint *args:
    cargo run {{ options }} {{ lab-features }} --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin paint-lab -- "$@"

# Native wide-SDR gamut comparison.
lab-gamut *args:
    cargo run {{ options }} {{ lab-features }} --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin gamut-lab -- "$@"

# Color swatches and CIE chromaticity; --help lists workspace options.
lab-color *args:
    cargo run {{ options }} {{ lab-features }} --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin color-lab -- "$@"

# Existing short commands remain available; every alias forwards app options.
alias media := lab-media
alias shaders := lab-shader
alias paints := lab-paint
alias gamut := lab-gamut
alias color := lab-color
alias text := lab-text
alias svg := lab-svg
# Parley shares the text comparison lab; select --backend parley for Parley only.
alias lab-parley := lab-text

alias canvas-lab := lab-canvas
alias media-lab := lab-media
alias shader-lab := lab-shader
alias paint-lab := lab-paint
alias gamut-lab := lab-gamut
alias color-lab := lab-color
alias text-lab := lab-text
alias svg-lab := lab-svg
alias quad-bench := lab-quad-bench
alias color-field-bench := lab-color-field-bench
alias chromaticity := lab-chromaticity
alias chromaticity-test := lab-color-test
alias lab-chromaticity-test := lab-color-test
alias parley-lab := lab-text

# Compatibility shortcut; use lab-color --chromaticity for new commands.
[private]
lab-chromaticity *args:
    @just --justfile "{{ justfile() }}" --set profile "{{ profile }}" lab-color --chromaticity "$@"

# Numerical, ICC, CIE dataset and color workspace checks.
lab-color-test *args:
    cargo test {{ options }} --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin color-lab --features runtime-shaders,portable-shaders,wgpu "$@"

# Native versus Parley text, paragraphs and scrolling.
lab-text *args:
    cargo run {{ options }} {{ lab-features }} --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin text-lab -- "$@"

# SVG rendering, zoom and scheduling comparisons.
lab-svg *args:
    cargo run {{ options }} {{ lab-features }} --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin svg-lab -- "$@"

# Quad rendering benchmark (bounded run by default).
lab-quad-bench *args:
    cargo run {{ options }} {{ lab-features }} --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin quad-bench -- "$@"

# Color-field rendering benchmark (bounded run by default).
lab-color-field-bench *args:
    cargo run {{ options }} {{ lab-features }} --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin color-field-bench -- "$@"

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

# Run every test suite for this OS and print a summary (--list, --only <name>).
test *args:
    bash scripts/test_all.sh "$@"

# Run unit and compatibility tests; native window tests remain opt-in.
lab-test *args:
    RUST_BACKTRACE=1 cargo test --locked --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --lib --bins --test compatibility --target-dir target "$@"
