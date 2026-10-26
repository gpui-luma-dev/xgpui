#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
app=apps/shader-examples
if [[ "$(wasm-bindgen --version)" != "wasm-bindgen 0.2.129" ]]; then
    echo "Install wasm-bindgen-cli 0.2.129 to match this app." >&2
    exit 1
fi
env -u RUSTFLAGS -u CARGO_ENCODED_RUSTFLAGS cargo build --locked --config .cargo/validation.toml \
    --manifest-path "$app/Cargo.toml" --target wasm32-unknown-unknown --target-dir target "$@"
profile=debug
for arg in "$@"; do if [[ "$arg" == --release ]]; then profile=release; fi; done
mkdir -p "$app/dist"
cp apps/web-support/graphics.mjs "$app/dist/graphics.mjs"
cp apps/web-support/gallery.css "$app/dist/gallery.css"
wasm-bindgen --target web --out-dir "$app/dist" --out-name shader_examples "target/wasm32-unknown-unknown/$profile/shader-examples.wasm"
cp "$app/index.html" "$app/dist/index.html"
mkdir -p docs/examples/shaders
cp "$app/dist/"* docs/examples/shaders/
echo "Built $app/dist and docs/examples/shaders. Run: python3 apps/shader-examples/serve.py"
