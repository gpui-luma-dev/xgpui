#!/usr/bin/env bash
# Run every test suite for this host OS, continue past failures, and print a
# summary. Invoked by `just test`; extra arguments are forwarded to cargo.
#
#   just test                 run everything for this OS
#   just test --list          print the suites without running them
#   just test --only <text>   run only suites whose name contains <text>
#
# Logs: target/test-logs/<suite>.log (override with TEST_ALL_LOG_DIR). Exit
# status is non-zero if any suite failed or lacked a prerequisite (PREREQ).
# GPU, display and window suites expect a real desktop session on the host.
set -uo pipefail

cd "$(dirname "$0")/.."

case "$(uname -s)" in
  Darwin) os=macos ;;
  Linux) os=linux ;;
  MINGW* | MSYS* | CYGWIN*) os=windows ;;
  *) echo "test_all: unsupported host $(uname -s)" >&2; exit 2 ;;
esac

list_only=0
only=""
cargo_args=()
while [ $# -gt 0 ]; do
  case "$1" in
    --list) list_only=1 ;;
    --only) only="${2:?--only needs a value}"; shift ;;
    *) cargo_args+=("$1") ;;
  esac
  shift
done

logs=${TEST_ALL_LOG_DIR:-target/test-logs}

# A suite exits with this status when a host prerequisite is missing. It is
# reported as PREREQ and counted as a failure, never as a pass.
prereq_status=77

# Strict Vulkan validation for native Linux GPU suites, matching the
# native-dma-buf job in linux-extensions.yml. The Khronos layer is appended to
# any VK_INSTANCE_LAYERS already set; a VK_LAYER_ENABLES already set is kept.
# Checks for the layer first and explains how to install it if it is missing.
# VULKANINFO overrides the vulkaninfo executable (used by the runner self-test).
vulkan_validation() {
  export WGPU_BACKEND=vulkan WGPU_DEBUG=1 WGPU_VALIDATION=1
  case ":${VK_INSTANCE_LAYERS-}:" in
    *:VK_LAYER_KHRONOS_validation:*) ;;
    *) export VK_INSTANCE_LAYERS="${VK_INSTANCE_LAYERS:+$VK_INSTANCE_LAYERS:}VK_LAYER_KHRONOS_validation" ;;
  esac
  export VK_LAYER_ENABLES="${VK_LAYER_ENABLES:-VK_VALIDATION_FEATURE_ENABLE_SYNCHRONIZATION_VALIDATION_EXT}"
  local vulkaninfo=${VULKANINFO:-vulkaninfo}
  if ! command -v "$vulkaninfo" >/dev/null 2>&1; then
    echo "PREREQ: vulkaninfo not found. Install Vulkan tools (Fedora: sudo dnf install vulkan-tools; Debian/Ubuntu: sudo apt install vulkan-tools)." >&2
    return 1
  fi
  if ! "$vulkaninfo" --summary 2>&1 | grep -q VK_LAYER_KHRONOS_validation; then
    echo "PREREQ: Khronos validation layer VK_LAYER_KHRONOS_validation not reported by vulkaninfo. Install it (Fedora: sudo dnf install vulkan-validation-layers; Debian/Ubuntu: sudo apt install vulkan-validationlayers)." >&2
    return 1
  fi
}
export -f vulkan_validation
vk="vulkan_validation || exit $prereq_status;"

ct="cargo test --locked --config .cargo/validation.toml"
lab="--manifest-path apps/xgpui-ext-lab/Cargo.toml"
win="--manifest-path gpui/crates/gpui-windows/Cargo.toml --features test-support"
wgpu="--manifest-path gpui/crates/gpui-wgpu/Cargo.toml"
parley="--manifest-path extensions/crates/xgpui-parley/Cargo.toml"
linux_parley="--features system-fonts,native-test,gpui-platform/wayland,gpui-platform/x11"

names=()
commands=()
# suite <name> <all|macos|linux|windows|unix> <command...>
# A "{cargo}" word in the command is replaced by any forwarded cargo arguments.
suite() {
  local name=$1 where=$2
  shift 2
  case "$where" in
    all) ;;
    unix) [ "$os" = windows ] && return ;;
    macos | linux | windows) [ "$where" = "$os" ] || return ;;
    *) echo "test_all: suite $name has unknown platform '$where'" >&2; exit 2 ;;
  esac
  if [ -n "$only" ] && [[ "$name" != *"$only"* ]]; then return; fi
  names+=("$name")
  commands+=("$*")
}

# Shared: tools, CPU extensions, labs and core benchmarks.
suite runner-selftest all "bash scripts/test_all_selftest.sh"
suite release-sync all "cargo test --locked --manifest-path tools/release-sync/Cargo.toml {cargo}"
# Match native CI's compilation gates before starting the runtime suites.
suite native-libraries all "cargo check --locked --features full-capture,runtime-shaders {cargo}"
suite gpui-bench-build all "cargo check --locked --config .cargo/validation.toml --manifest-path gpui/crates/gpui-platform/Cargo.toml --features bench-support,test-support,runtime_shaders --benches {cargo}"
suite media-minimal linux "cargo check --locked --config .cargo/validation.toml --manifest-path extensions/crates/xgpui-media/Cargo.toml --no-default-features --lib {cargo}"
suite color all "$ct --manifest-path extensions/crates/xgpui-color/Cargo.toml --features icc {cargo}"
suite gamut all "$ct --manifest-path extensions/crates/xgpui-gamut/Cargo.toml {cargo}"
suite gamut-instanced all "$ct --manifest-path extensions/crates/xgpui-gamut/Cargo.toml --features instanced {cargo}"
suite canvas all "$ct --manifest-path extensions/crates/xgpui-canvas/Cargo.toml --features gpui --lib {cargo}"
suite media all "$ct --manifest-path extensions/crates/xgpui-media/Cargo.toml --features wgpu --lib {cargo}"
suite svg all "$ct --manifest-path extensions/crates/xgpui-svg/Cargo.toml --lib {cargo}"
suite shader all "$ct --manifest-path extensions/crates/xgpui-shader/Cargo.toml --features wgpu,portable-shaders --lib {cargo}"
suite shell all "$ct --manifest-path extensions/crates/xgpui-shell/Cargo.toml --lib {cargo}"
suite gpui-core all "$ct --manifest-path gpui/crates/gpui/Cargo.toml --features test-support --lib {cargo}"
# CRITERION_HOME stops Criterion's fallback `cargo metadata` call, which ignores
# the validation config and would rewrite gpui's Cargo.lock (as in validate.yml).
bench="CRITERION_HOME=target/criterion-tests $ct --manifest-path gpui/crates/gpui/Cargo.toml --features bench-support,test-support,gpui_platform/runtime_shaders --lib {cargo}"
suite gpui-bench-metrics all "$bench bench_metrics:: -- --test-threads=1"
suite gpui-bench-context all "$bench app::bench_context:: -- --test-threads=1"
suite labs all "$ct $lab --features runtime-shaders,portable-shaders,wgpu --lib --bins --test compatibility {cargo} -- --test-threads=1"

# macOS: CoreText font backend and iOS simulator compile check.
# gpui-apple has no checked-in lockfile, so it cannot use --locked; remove the
# lockfile cargo generates so the run leaves the checkout clean.
apple_lock=gpui/crates/gpui-apple/Cargo.lock
suite apple-text macos "keep=0; [ -e $apple_lock ] && keep=1; cargo test --config .cargo/validation.toml --manifest-path gpui/crates/gpui-apple/Cargo.toml --features test-support,font-kit,runtime_shaders --lib {cargo} text_system::; status=\$?; [ \$keep = 1 ] || rm -f $apple_lock; exit \$status"
suite ios-compile macos "$ct --manifest-path gpui/crates/gpui-ios/Cargo.toml --features runtime_shaders --target aarch64-apple-ios-sim --test text_input --no-run {cargo}"

# Linux: Cosmic/Parley text, wgpu, DMA-BUF, window build, Vulkan paint suites.
suite cosmic-text linux "$ct $wgpu --lib {cargo} cosmic_text_system::tests"
suite parley linux "$ct $parley --lib $linux_parley {cargo}"
suite parley-construction linux "$ct $parley --test construction $linux_parley {cargo}"
suite text-lab-axes linux "$ct $lab --bin text-lab --features runtime-shaders {cargo} axes::tests"
suite wgpu-dma-buf linux "$vk $ct $wgpu --features dma-buf,test-support --lib {cargo} custom_draw::dma_buf::native_tests::dma_buf_native_generated_snapshot -- --ignored --exact --nocapture --test-threads=1"
suite media-dma-buf-video linux "$vk $ct $lab --features runtime-shaders,portable-shaders,wgpu --bin media-lab {cargo} video_lab::linux::tests::dma_buf_video_capture_pixels_and_lifetimes -- --ignored --exact --nocapture --test-threads=1"
suite media-video-capture linux "$vk cargo run --locked --config .cargo/validation.toml $lab --features runtime-shaders,portable-shaders,wgpu --bin media-lab {cargo} -- --headless --video --wgpu --png $logs/media-video.png"
suite linux-windows-build linux "$ct $lab --features linux-window-tests,runtime-shaders,portable-shaders,wgpu --test linux_windows --no-run {cargo}"

# Paint GPU suites from .github/paints/commands.json (CI runs them through
# run.py; keep the two lists in step). Steps already covered above are not
# repeated: paint-lab wgpu checks run in "labs", brush tests in "shader", and
# gpui-windows coverage/DX11/wide-gamut tests in "windows-backend".
suite wgpu-coverage-cache linux "WGPU_BACKEND=vulkan $ct $wgpu --features test-support --lib {cargo} coverage_cache"
suite wgpu-custom-draw linux "WGPU_BACKEND=vulkan $ct $wgpu --features test-support --lib {cargo} wgpu_custom -- --nocapture"

# Windows: DirectWrite fonts, DirectX renderer, Windows apps, paint GPU suites.
suite directwrite-fonts windows "$ct $win --lib {cargo} font_variation_tests -- --test-threads=1"
suite windows-backend windows "$ct $win --lib {cargo} -- --test-threads=1 --skip font_variation_tests"
suite color-lab windows "$ct $lab --bin color-lab {cargo} -- --test-threads=1"
suite gamut-lab windows "$ct $lab --bin gamut-lab --test compatibility {cargo}"
suite media-lab-video windows "$ct $lab --bin media-lab {cargo} video_lab:: -- --test-threads=1"
suite wgpu-coverage-cache windows "WGPU_BACKEND=dx12 $ct $wgpu --features test-support --lib {cargo} coverage_cache"
suite wgpu-custom-draw windows "WGPU_BACKEND=dx12 $ct $wgpu --features test-support --lib {cargo} wgpu_custom -- --nocapture"
suite paint-lab-check windows "cargo check --locked --config .cargo/validation.toml $lab --bin paint-lab --features portable-shaders {cargo}"
suite paint-lab-portable windows "WGPU_BACKEND=dx12 $ct $lab --bin paint-lab --features wgpu,portable-shaders {cargo} -- --nocapture"
suite shader-brush-native windows "XGPUI_BRUSH_NATIVE=1 $ct --manifest-path extensions/crates/xgpui-shader/Cargo.toml --features wgpu,portable-shaders --lib {cargo} brush::tests -- --nocapture"
suite paint-dx11-capture windows "cargo run --locked --config .cargo/validation.toml $lab --bin paint-lab --features portable-shaders {cargo} -- --headless --effect mesh --frames 3 --png $logs/native-dx11-mesh.png"

if [ ${#names[@]} -eq 0 ]; then
  echo "test_all: no suites match${only:+ \"$only\"} on $os" >&2
  exit 2
fi

forwarded=""
if [ ${#cargo_args[@]} -gt 0 ]; then
  forwarded=$(printf '%q ' "${cargo_args[@]}")
fi

if [ $list_only -eq 1 ]; then
  for i in "${!names[@]}"; do
    printf '%-24s %s\n' "${names[$i]}" "${commands[$i]//\{cargo\}/$forwarded}"
  done
  exit 0
fi

mkdir -p "$logs"
export RUST_BACKTRACE=1
results=()
failed=0
start_all=$SECONDS

for i in "${!names[@]}"; do
  name=${names[$i]}
  command=${commands[$i]//\{cargo\}/$forwarded}
  log="$logs/$name.log"
  printf '\n==> [%d/%d] %s\n    %s\n' $((i + 1)) ${#names[@]} "$name" "$command"
  start=$SECONDS
  bash -c "$command" 2>&1 | tee "$log"
  status=${PIPESTATUS[0]}
  elapsed=$((SECONDS - start))
  if [ "$status" -eq 0 ]; then
    results+=("$(printf 'PASS  %-24s %5ss' "$name" "$elapsed")")
  elif [ "$status" -eq "$prereq_status" ]; then
    results+=("$(printf 'PREREQ %-23s %5ss  (missing prerequisite, see %s)' "$name" "$elapsed" "$log")")
    failed=$((failed + 1))
  else
    results+=("$(printf 'FAIL  %-24s %5ss  (exit %s, %s)' "$name" "$elapsed" "$status" "$log")")
    failed=$((failed + 1))
  fi
done

printf '\n==== Test summary (%s, %ss) ====\n' "$os" $((SECONDS - start_all))
printf '%s\n' "${results[@]}"
printf '%d of %d suites failed or lacked prerequisites.\n' "$failed" ${#names[@]}
[ "$failed" -eq 0 ]
