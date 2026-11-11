#!/usr/bin/env bash
# Build the instanced shader examples into pages/examples/shaders/ (generated files only).
set -euo pipefail
cd "$(dirname "$0")/../../.."
out=pages/examples/shaders
if [[ "$(wasm-bindgen --version)" != "wasm-bindgen 0.2.129" ]]; then
    echo "Install wasm-bindgen-cli 0.2.129 to match this app." >&2
    exit 1
fi
env -u RUSTFLAGS -u CARGO_ENCODED_RUSTFLAGS cargo build --locked --config .cargo/validation.toml \
    --manifest-path "apps/xgpui-ext-lab/Cargo.toml" --bin shader-examples --no-default-features --features web --target wasm32-unknown-unknown --target-dir target "$@"
profile=debug
for arg in "$@"; do if [[ "$arg" == --release ]]; then profile=release; fi; done
bash apps/xgpui-ext-lab/shader-examples/clean.sh
wasm-bindgen --target web --out-dir "$out" --out-name shader_examples "target/wasm32-unknown-unknown/$profile/shader-examples.wasm"
echo "Built $out. Preview with: just pages-serve"
