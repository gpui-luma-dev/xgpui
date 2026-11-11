#!/usr/bin/env bash
# Remove only the files build.sh generates in pages/examples/extensions/.
set -euo pipefail
cd "$(dirname "$0")/../../.."
out=pages/examples/extensions
rm -f "$out"/extensions_web.js "$out"/extensions_web.d.ts "$out"/extensions_web_bg.wasm "$out"/extensions_web_bg.wasm.d.ts \
    "$out"/build-id.txt "$out"/svg-ATTRIBUTION.md "$out"/*.svg \
    "$out"/font-licenses/RobotoFlex-OFL.txt "$out"/font-licenses/Inter-LICENSE \
    "$out"/font-licenses/NotoSans-LICENSE "$out"/font-licenses/IBM-Plex-license.txt
rm -rf "$out"/snippets "$out"/italy "$out"/icc "$out"/colorimetry
