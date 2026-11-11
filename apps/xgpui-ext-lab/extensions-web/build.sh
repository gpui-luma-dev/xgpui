#!/usr/bin/env bash
# Build the extension demos into pages/examples/extensions/. Only generated files
# are written there; the hand-authored page, scripts and styles are checked in.
set -euo pipefail
cd "$(dirname "$0")/../../.."
out=pages/examples/extensions
expected_bindgen=0.2.129
actual_bindgen=$(wasm-bindgen --version | cut -d ' ' -f 2)
if [[ "$actual_bindgen" != "$expected_bindgen" ]]; then
    echo "wasm-bindgen $expected_bindgen is required (found $actual_bindgen)." >&2
    echo "Install with: cargo install wasm-bindgen-cli --version $expected_bindgen --locked" >&2
    exit 1
fi
# The page compares build-id.txt with the ID compiled into the module.
export XGPUI_WEB_BUILD_ID="$(git rev-parse --short HEAD)-$(date -u +%Y%m%dT%H%M%SZ)"
# Ignore global host linker flags (for example macOS -lc++). This target is
# deliberately non-atomic, with no shared-memory linker flags.
env -u RUSTFLAGS -u CARGO_ENCODED_RUSTFLAGS cargo build --locked --config .cargo/validation.toml \
    --manifest-path "apps/xgpui-ext-lab/Cargo.toml" --bin extensions-web --no-default-features --features web --target wasm32-unknown-unknown \
    --target-dir target "$@"
# Use --release to optimize; leave the default debug build quick to iterate.
profile=debug
for arg in "$@"; do
    if [[ "$arg" == --release ]]; then profile=release; fi
done
bash apps/xgpui-ext-lab/extensions-web/clean.sh
wasm-bindgen --target web --out-dir "$out" --out-name extensions_web \
    "target/wasm32-unknown-unknown/$profile/extensions-web.wasm"
printf '%s\n' "$XGPUI_WEB_BUILD_ID" > "$out/build-id.txt"
# Data shared with native labs and tests stays beside its Rust sources.
app=apps/xgpui-ext-lab/extensions-web
mkdir -p "$out/italy" "$out/icc" "$out/colorimetry"
cp "$app/assets/italy/"*.jpg "$app/assets/italy/README.md" "$app/assets/italy/prepare.swift" "$out/italy/"
cp "$app/assets/colorimetry/"* "$out/colorimetry/"
for notice in README.md manifest.json CC0-1.0.txt; do
    cp "extensions/crates/xgpui-color/tests/reference/real-icc/$notice" "$out/icc/$notice"
done
cp extensions/crates/xgpui-parley/tests/fonts/RobotoFlex-OFL.txt \
    extensions/crates/xgpui-parley/tests/fonts/Inter-LICENSE \
    extensions/crates/xgpui-parley/tests/fonts/NotoSans-LICENSE "$out/font-licenses/"
cp gpui/crates/gpui-web/assets/fonts/ibm-plex-sans/license.txt "$out/font-licenses/IBM-Plex-license.txt"
cp apps/xgpui-ext-lab/svg-lab/samples/ATTRIBUTION.md "$out/svg-ATTRIBUTION.md"
for sample in ribbon-leaf.svg AJ_Digital_Camera.svg rg1024_green_grapes.svg rg1024_metal_effect.svg dragon.svg; do
    cp "apps/xgpui-ext-lab/svg-lab/samples/$sample" "$out/$sample"
done
echo "Built $out. Preview with: just docs-serve"
