#!/usr/bin/env bash
# Remove only the files build.sh generates in pages/examples/shaders/.
set -euo pipefail
cd "$(dirname "$0")/../../.."
out=pages/examples/shaders
rm -f "$out"/shader_examples.js "$out"/shader_examples.d.ts "$out"/shader_examples_bg.wasm "$out"/shader_examples_bg.wasm.d.ts
rm -rf "$out"/snippets
