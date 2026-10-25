#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
app=apps/extensions-web
expected_bindgen=0.2.129
actual_bindgen=$(wasm-bindgen --version | cut -d ' ' -f 2)
if [[ "$actual_bindgen" != "$expected_bindgen" ]]; then
    echo "wasm-bindgen $expected_bindgen is required (found $actual_bindgen)." >&2
    echo "Install with: cargo install wasm-bindgen-cli --version $expected_bindgen --locked" >&2
    exit 1
fi
# Ignore global host linker flags (for example macOS -lc++). This target is
# deliberately non-atomic, with no shared-memory linker flags.
env -u RUSTFLAGS -u CARGO_ENCODED_RUSTFLAGS cargo build --locked --config .cargo/validation.toml \
    --manifest-path "$app/Cargo.toml" --target wasm32-unknown-unknown \
    --target-dir "$app/target" "$@"
# Use --release to optimize; leave the default debug build quick to iterate.
profile=debug
for arg in "$@"; do
    if [[ "$arg" == --release ]]; then profile=release; fi
done
mkdir -p "$app/dist"
wasm-bindgen --target web --out-dir "$app/dist" --out-name extensions_web \
    "$app/target/wasm32-unknown-unknown/$profile/extensions-web.wasm"
cp docs/favicon.svg docs/favicon.ico "$app/dist/"
cp "$app/index.html" "$app/dist/index.html"
cp "$app/svg-attribution.html" "$app/dist/svg-attribution.html"
cp apps/svg-lab/samples/ATTRIBUTION.md "$app/dist/svg-ATTRIBUTION.md"
for sample in ribbon-leaf.svg AJ_Digital_Camera.svg rg1024_green_grapes.svg rg1024_metal_effect.svg dragon.svg; do
    cp "apps/svg-lab/samples/$sample" "$app/dist/$sample"
done
echo "Built $app/dist. Run: python3 apps/extensions-web/serve.py"
