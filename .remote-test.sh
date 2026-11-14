#!/usr/bin/env bash
set -euo pipefail
post_to 53
note "Issue #53: native Metal wide-gamut typography acceptance"
if [[ "$(uname -s)" != Darwin ]]; then
    note "This issue requires macOS/Metal. Other backends cannot establish acceptance."
    step "Record this run as skipped" "Issue #53 remains unverified on this machine"
    exit 0
fi
note "Building the existing typography lab"
cargo build --locked --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin shader-examples --features runtime-shaders,portable-shaders,wgpu --target-dir target
./target/debug/shader-examples --example typography --wide-gamut &
XGPUI_EXAMPLE_PIDS="$!"
trap stop_example EXIT
step "Inspect presentation status" "Requested and Effective both say WideGamutSdr; otherwise skip the remaining wide-gamut checks"
step "Try glow and drop shadow at softness 4, 8 and 12 in light and dark themes" "Effects render without stale frames or errors; Effective stays WideGamutSdr"
step "Try Static, Typewriter, Scramble and Wave; pause, scrub and enable reduced motion" "Effects update correctly; reduced motion shows stable source; presentation stays wide"
step "Select and copy the Unicode and Ligatures samples, including while text is partly revealed" "Clipboard and source inspector preserve the original text and complete graphemes"
step "Change type size and width, resize the window and inspect Retina blur edges" "No clipping seams, broken halos or stale frames; selection remains usable"
step "Switch Default SDR, then Wide-gamut SDR repeatedly" "Requested/effective status follows the selection; effects survive every switch"
step "Close the lab and inspect terminal output" "No captured-effects or BGRA8-SDR rejection errors"
