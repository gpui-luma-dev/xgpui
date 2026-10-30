#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../../.."
app=apps/xgpui-ext-lab/extensions-web
out=target/web/extensions
expected_bindgen=0.2.129
actual_bindgen=$(wasm-bindgen --version | cut -d ' ' -f 2)
if [[ "$actual_bindgen" != "$expected_bindgen" ]]; then
    echo "wasm-bindgen $expected_bindgen is required (found $actual_bindgen)." >&2
    echo "Install with: cargo install wasm-bindgen-cli --version $expected_bindgen --locked" >&2
    exit 1
fi
# Stamp the page and compiled module together; each build gets fresh asset URLs.
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
rm -rf "$out"
mkdir -p "$out"
cp apps/web-support/graphics.mjs "$out/graphics.mjs"
cp apps/web-support/gallery.css "$out/gallery.css"
wasm-bindgen --target web --out-dir "$out" --out-name extensions_web \
    "target/wasm32-unknown-unknown/$profile/extensions-web.wasm"
cp docs/favicon.svg docs/favicon.ico "$out/"
python3 - "$app/index.html" "$out/index.html" "$XGPUI_WEB_BUILD_ID" <<'PYTHON'
import pathlib
import sys
source, destination, revision = sys.argv[1:]
pathlib.Path(destination).write_text(pathlib.Path(source).read_text().replace("__BUILD_ID__", revision))
PYTHON
cp "$app/svg-attribution.html" "$out/svg-attribution.html"
cp apps/xgpui-ext-lab/svg-lab/samples/ATTRIBUTION.md "$out/svg-ATTRIBUTION.md"
for sample in ribbon-leaf.svg AJ_Digital_Camera.svg rg1024_green_grapes.svg rg1024_metal_effect.svg dragon.svg; do
    cp "apps/xgpui-ext-lab/svg-lab/samples/$sample" "$out/$sample"
done
echo "Built $out. Run: python3 apps/xgpui-ext-lab/extensions-web/serve.py"
