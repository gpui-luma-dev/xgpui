#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../../.."
app=apps/xgpui-ext-lab/shader-examples
out=target/web/shaders
if [[ "$(wasm-bindgen --version)" != "wasm-bindgen 0.2.129" ]]; then
    echo "Install wasm-bindgen-cli 0.2.129 to match this app." >&2
    exit 1
fi
env -u RUSTFLAGS -u CARGO_ENCODED_RUSTFLAGS cargo build --locked --config .cargo/validation.toml \
    --manifest-path "apps/xgpui-ext-lab/Cargo.toml" --bin shader-examples --no-default-features --features web --target wasm32-unknown-unknown --target-dir target "$@"
profile=debug
for arg in "$@"; do if [[ "$arg" == --release ]]; then profile=release; fi; done
rm -rf "$out"
mkdir -p "$out"
cp apps/web-support/graphics.mjs "$out/graphics.mjs"
cp apps/web-support/gallery.css "$out/gallery.css"
wasm-bindgen --target web --out-dir "$out" --out-name shader_examples "target/wasm32-unknown-unknown/$profile/shader-examples.wasm"
cp "$app/index.html" "$out/index.html"
echo "Built $out. Run: python3 apps/xgpui-ext-lab/shader-examples/serve.py"
