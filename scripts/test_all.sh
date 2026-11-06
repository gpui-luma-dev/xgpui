#!/usr/bin/env bash
# Run every test suite for this host OS, continue past failures, and print a
# summary. Invoked by `just test`; extra arguments are forwarded to cargo.
#
#   just test                 run everything for this OS
#   just test --list          print the suites without running them
#   just test --only <text>   run only suites whose name contains <text>
#
# Most suites share one workspace build: a single `cargo test --no-run` over
# every tested package with one feature set, so GPUI and its dependencies are
# compiled once. Suites then run the prebuilt test executables with their own
# filters, thread limits and GPU environment. Deliberately isolated
# configurations (default-feature gamut, media without GPU backends, the
# portable-only paint lab, cross-target iOS) keep their own cargo invocations.
#
# Forwarded arguments are cargo options. They are passed to the shared build
# and to every isolated cargo invocation, not to the test executables.
#
# Logs: target/test-logs/<suite>.log, plus build.log (compiler output for the
# shared build) and build.json (Cargo's JSON messages). Override the directory
# with TEST_ALL_LOG_DIR. Exit status is non-zero if the build or any suite
# failed or lacked a prerequisite (PREREQ).
# GPU, display and window suites expect a real desktop session on the host.
set -uo pipefail

cd "$(dirname "$0")/.."
root=$(pwd)

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
case "$logs" in /* | [A-Za-z]:*) ;; *) logs="$root/$logs" ;; esac

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

# --- Shared workspace build -------------------------------------------------
# Every package and feature tested on this host, compiled together so Cargo
# unifies features once. Changing this list changes the feature set for every
# shared suite; keep it a superset of what each suite needs.
packages=(xgpui xgpui-platform xgpui-color xgpui-gamut xgpui-canvas xgpui-media xgpui-svg xgpui-shader xgpui-shell xgpui-ext-lab)
features=(
  xgpui/test-support xgpui/bench-support
  xgpui-platform/test-support xgpui-platform/bench-support xgpui-platform/runtime_shaders
  xgpui-color/icc xgpui-gamut/instanced xgpui-canvas/gpui xgpui-media/wgpu
  xgpui-shader/wgpu xgpui-shader/portable-shaders
  xgpui-ext-lab/runtime-shaders xgpui-ext-lab/portable-shaders xgpui-ext-lab/wgpu
)
case "$os" in
  macos)
    packages+=(xgpui-apple)
    features+=(xgpui-apple/test-support xgpui-apple/font-kit xgpui-apple/runtime_shaders)
    ;;
  linux)
    packages+=(xgpui-wgpu xgpui-parley)
    features+=(
      xgpui-wgpu/test-support xgpui-wgpu/dma-buf
      xgpui-parley/system-fonts xgpui-parley/native-test
      xgpui-platform/wayland xgpui-platform/x11
      xgpui-ext-lab/linux-window-tests
    )
    ;;
  windows)
    packages+=(xgpui-windows xgpui-wgpu)
    features+=(xgpui-windows/test-support xgpui-wgpu/test-support)
    ;;
esac
build_args=(test --no-run --locked --config .cargo/validation.toml --message-format json-render-diagnostics)
for package in "${packages[@]}"; do build_args+=(-p "$package"); done
build_args+=(--features "$(IFS=,; echo "${features[*]}")" --tests --benches)

# Executable index written after the shared build: one tab-separated line per
# test executable: package, target kind (lib, bin, test, bench), target name,
# executable, package directory.
export TEST_ALL_ARTIFACTS="$logs/artifacts.tsv"

# run_tests <package> <targets> [test arguments...]
# Runs the prebuilt test executables of <package> selected by <targets>, a
# comma-separated list of: all, lib, bins, bin:<name>, test:<name>,
# bench:<name>. Executables run from the package directory, as under
# `cargo test`. Every selected executable runs; any failure fails the suite.
run_tests() {
  local package=$1 targets=$2
  shift 2
  local status=0 found=0 p kind name exe dir selected spec
  if [ ! -s "$TEST_ALL_ARTIFACTS" ]; then
    echo "run_tests: shared build produced no test executables; see build.log" >&2
    return 1
  fi
  while IFS=$'\t' read -r p kind name exe dir; do
    [ "$p" = "$package" ] || continue
    selected=0
    for spec in ${targets//,/ }; do
      case "$spec" in
        all) selected=1 ;;
        lib) [ "$kind" = lib ] && selected=1 ;;
        bins) [ "$kind" = bin ] && selected=1 ;;
        *:*) [ "$spec" = "$kind:$name" ] && selected=1 ;;
      esac
    done
    [ $selected -eq 1 ] || continue
    found=1
    printf '     Running %s %s (%s)\n' "$kind" "$name" "$exe"
    (
      cd "$dir" || exit 1
      export CARGO_MANIFEST_DIR="$dir" CARGO_PKG_NAME="$p"
      "$exe" "$@" </dev/null
    ) || status=1
  done <"$TEST_ALL_ARTIFACTS"
  if [ $found -eq 0 ]; then
    echo "run_tests: no test executable for $package ($targets)" >&2
    return 1
  fi
  return $status
}
export -f run_tests

# built <package> <targets>: pass when the shared build produced the selected
# executables. Used for compile-only coverage such as benchmarks.
built() {
  run_tests "$1" "$2" --help >/dev/null
}
export -f built

names=()
commands=()
shared=()
# suite <name> <all|macos|linux|windows|unix> <command...>
# A "{cargo}" word in the command is replaced by any forwarded cargo arguments.
# Commands using run_tests or built need the shared build.
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
  case " $* " in *" run_tests "* | *" built "*) shared+=(1) ;; *) shared+=(0) ;; esac
}

ct="cargo test --locked --config .cargo/validation.toml"
lab="--manifest-path apps/xgpui-ext-lab/Cargo.toml"

# Shared: tools, CPU extensions, labs and core benchmarks.
suite runner-selftest all "bash scripts/test_all_selftest.sh"
suite release-sync all "cargo test --locked --manifest-path tools/release-sync/Cargo.toml {cargo}"
# Match native CI's compilation gate: platform libraries with release features.
suite native-libraries all "cargo check --locked --features full-capture,runtime-shaders {cargo}"
# Platform benchmarks compile in the shared build (bench-support, test-support).
suite gpui-bench-build all "built xgpui-platform bench:atlas_upload,bench:focus_cleanup,bench:inspector_render,bench:randomized_element_tree"
# Isolated: media without optional GPU backends.
suite media-minimal linux "cargo check --locked --config .cargo/validation.toml --manifest-path extensions/crates/xgpui-media/Cargo.toml --no-default-features --lib {cargo}"
# Doctests are not part of `--no-run`; xgpui-color has a small graph of its own.
suite color all "run_tests xgpui-color all && $ct --manifest-path extensions/crates/xgpui-color/Cargo.toml --features icc --doc {cargo}"
# Isolated: gamut with default features (the shared build enables instanced).
suite gamut all "$ct --manifest-path extensions/crates/xgpui-gamut/Cargo.toml {cargo}"
suite gamut-instanced all "run_tests xgpui-gamut all"
suite canvas all "run_tests xgpui-canvas lib"
suite media all "run_tests xgpui-media lib"
suite svg all "run_tests xgpui-svg lib"
suite shader all "run_tests xgpui-shader lib"
suite shell all "run_tests xgpui-shell lib"
# The shared build enables bench-support; its collectors run single-threaded below.
suite gpui-core all "run_tests xgpui lib --skip bench_metrics:: --skip app::bench_context::"
# CRITERION_HOME stops Criterion's fallback `cargo metadata` call.
suite gpui-bench-metrics all "CRITERION_HOME=$root/target/criterion-tests run_tests xgpui lib bench_metrics:: --test-threads=1"
suite gpui-bench-context all "CRITERION_HOME=$root/target/criterion-tests run_tests xgpui lib app::bench_context:: --test-threads=1"
suite labs all "run_tests xgpui-ext-lab lib,bins,test:compatibility --test-threads=1"

# macOS: CoreText font backend and iOS simulator compile check (isolated target).
suite apple-text macos "run_tests xgpui-apple lib text_system::"
suite ios-compile macos "$ct --manifest-path gpui/crates/gpui-ios/Cargo.toml --features runtime_shaders --target aarch64-apple-ios-sim --test text_input --no-run {cargo}"

# Linux: Cosmic/Parley text, wgpu, DMA-BUF, window build, Vulkan paint suites.
suite cosmic-text linux "run_tests xgpui-wgpu lib cosmic_text_system::tests"
suite parley linux "run_tests xgpui-parley lib"
suite parley-construction linux "run_tests xgpui-parley test:construction"
suite text-lab-axes linux "run_tests xgpui-ext-lab bin:text-lab axes::tests"
suite wgpu-dma-buf linux "$vk run_tests xgpui-wgpu lib custom_draw::dma_buf::native_tests::dma_buf_native_generated_snapshot --ignored --exact --nocapture --test-threads=1"
suite media-dma-buf-video linux "$vk run_tests xgpui-ext-lab bin:media-lab video_lab::linux::tests::dma_buf_video_capture_pixels_and_lifetimes --ignored --exact --nocapture --test-threads=1"
suite media-video-capture linux "$vk cargo run --locked --config .cargo/validation.toml $lab --features runtime-shaders,portable-shaders,wgpu --bin media-lab {cargo} -- --headless --video --wgpu --png $logs/media-video.png"
suite linux-windows-build linux "built xgpui-ext-lab test:linux_windows"

# Paint GPU suites from .github/paints/commands.json (CI runs them through
# run.py; keep the two lists in step). Steps already covered above are not
# repeated: paint-lab wgpu checks run in "labs", brush tests in "shader", and
# gpui-windows coverage/DX11/wide-gamut tests in "windows-backend".
suite wgpu-coverage-cache linux "WGPU_BACKEND=vulkan run_tests xgpui-wgpu lib coverage_cache"
suite wgpu-custom-draw linux "WGPU_BACKEND=vulkan run_tests xgpui-wgpu lib wgpu_custom --nocapture"

# Windows: DirectWrite fonts, DirectX renderer, Windows apps, paint GPU suites.
suite directwrite-fonts windows "run_tests xgpui-windows lib font_variation_tests --test-threads=1"
suite windows-backend windows "run_tests xgpui-windows lib --test-threads=1 --skip font_variation_tests"
suite color-lab windows "run_tests xgpui-ext-lab bin:color-lab --test-threads=1"
suite gamut-lab windows "run_tests xgpui-ext-lab bin:gamut-lab,test:compatibility"
suite media-lab-video windows "run_tests xgpui-ext-lab bin:media-lab video_lab:: --test-threads=1"
suite wgpu-coverage-cache windows "WGPU_BACKEND=dx12 run_tests xgpui-wgpu lib coverage_cache"
suite wgpu-custom-draw windows "WGPU_BACKEND=dx12 run_tests xgpui-wgpu lib wgpu_custom --nocapture"
# Isolated: the paint lab with portable shaders only (no wgpu). The check and
# the DX11 capture use the same cargo selection, so the capture reuses it.
paint_portable="--locked --config .cargo/validation.toml $lab --bin paint-lab --features portable-shaders"
suite paint-lab-check windows "cargo build $paint_portable {cargo}"
suite paint-lab-portable windows "WGPU_BACKEND=dx12 run_tests xgpui-ext-lab bin:paint-lab --nocapture"
suite shader-brush-native windows "XGPUI_BRUSH_NATIVE=1 run_tests xgpui-shader lib brush::tests --nocapture"
suite paint-dx11-capture windows "cargo run $paint_portable {cargo} -- --headless --effect mesh --frames 3 --png $logs/native-dx11-mesh.png"

if [ ${#names[@]} -eq 0 ]; then
  echo "test_all: no suites match${only:+ \"$only\"} on $os" >&2
  exit 2
fi

forwarded=""
if [ ${#cargo_args[@]} -gt 0 ]; then
  forwarded=$(printf '%q ' "${cargo_args[@]}")
fi

needs_build=0
for flag in "${shared[@]}"; do [ "$flag" = 1 ] && needs_build=1; done

if [ $list_only -eq 1 ]; then
  if [ $needs_build -eq 1 ]; then
    printf '%-24s %s\n' "(shared build)" "cargo $(printf '%q ' "${build_args[@]}")$forwarded"
  fi
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

# index_artifacts <cargo-json> <tsv>: list test executables from Cargo's JSON.
index_artifacts() {
  local python=${PYTHON:-}
  if [ -z "$python" ]; then
    if [ "$os" = windows ]; then python=python; else python=python3; fi
  fi
  "$python" - "$1" "$2" <<'PY'
import json, os, sys
rows = []
with open(sys.argv[1], encoding="utf-8") as messages:
    for line in messages:
        if not line.startswith("{"):
            continue
        message = json.loads(line)
        if message.get("reason") != "compiler-artifact" or not message.get("executable"):
            continue
        if not message["profile"]["test"] and "bench" not in message["target"]["kind"]:
            continue
        kinds = message["target"]["kind"]
        kind = "bin" if "bin" in kinds else "test" if "test" in kinds else "bench" if "bench" in kinds else "lib"
        # Package IDs are "<source>#<name>@<version>", or "<source>#<version>"
        # when the package name matches its directory name.
        source, _, fragment = message["package_id"].rpartition("#")
        package = fragment.split("@")[0] if "@" in fragment else source.rstrip("/").rsplit("/", 1)[-1]
        rows.append((package, kind, message["target"]["name"], message["executable"], os.path.dirname(message["manifest_path"])))
with open(sys.argv[2], "w", encoding="utf-8", newline="\n") as index:
    for row in rows:
        index.write("\t".join(row) + "\n")
PY
}

if [ $needs_build -eq 1 ]; then
  printf '\n==> shared build (%d packages)\n    cargo %s%s\n' ${#packages[@]} "$(printf '%q ' "${build_args[@]}")" "$forwarded"
  start=$SECONDS
  rm -f "$TEST_ALL_ARTIFACTS"
  # JSON messages go to build.json; rendered diagnostics and progress go to
  # build.log and the console.
  eval "cargo $(printf '%q ' "${build_args[@]}") $forwarded" >"$logs/build.json" 2> >(tee "$logs/build.log" >&2)
  build_status=$?
  wait
  if [ $build_status -eq 0 ] && ! index_artifacts "$logs/build.json" "$TEST_ALL_ARTIFACTS"; then
    echo "test_all: could not index test executables from $logs/build.json" >&2
    build_status=1
  fi
  elapsed=$((SECONDS - start))
  if [ $build_status -eq 0 ]; then
    results+=("$(printf 'PASS  %-24s %5ss' "(shared build)" "$elapsed")")
  else
    results+=("$(printf 'FAIL  %-24s %5ss  (exit %s, %s)' "(shared build)" "$elapsed" "$build_status" "$logs/build.log")")
    failed=$((failed + 1))
    rm -f "$TEST_ALL_ARTIFACTS"
  fi
fi

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

total=$(( ${#names[@]} + needs_build ))
printf '\n==== Test summary (%s, %ss) ====\n' "$os" $((SECONDS - start_all))
printf '%s\n' "${results[@]}"
printf '%d of %d steps failed or lacked prerequisites.\n' "$failed" "$total"
[ "$failed" -eq 0 ]
