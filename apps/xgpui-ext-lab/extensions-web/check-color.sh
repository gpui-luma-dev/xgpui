#!/usr/bin/env bash
# Check the dependency-free preview sampler without browser platform dependencies.
set -euo pipefail
cd "$(dirname "$0")/../../.."
color_check_dir=$(mktemp -d "${TMPDIR:-/tmp}/xgpui-color-check.XXXXXX")
trap 'rm -rf "$color_check_dir"' EXIT
rustc --edition 2024 --crate-type rlib --crate-name xgpui_color \
    extensions/crates/xgpui-color/src/lib.rs \
    -o "$color_check_dir/libxgpui_color.rlib"
rustc --edition 2024 --test apps/xgpui-ext-lab/extensions-web/src/color_samples.rs \
    --extern "xgpui_color=$color_check_dir/libxgpui_color.rlib" \
    -o "$color_check_dir/color-samples"
"$color_check_dir/color-samples"
