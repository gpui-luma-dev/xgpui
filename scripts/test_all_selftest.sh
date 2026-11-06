#!/usr/bin/env bash
# Self-test for scripts/test_all.sh: environment propagation, prerequisite
# handling and continue-after-failure. Runs test_all.sh as a Linux host with
# fake uname, cargo and vulkaninfo, so it needs no GPU or Rust build.
set -uo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
fake="$tmp/bin"
mkdir -p "$fake"

cat >"$fake/uname" <<'EOF'
#!/usr/bin/env bash
echo "${FAKE_UNAME:-Linux}"
EOF
# Records its arguments and the Vulkan/wgpu environment, one file per call,
# and fails when an argument matches FAKE_CARGO_FAIL.
cat >"$fake/cargo" <<'EOF'
#!/usr/bin/env bash
n=$(ls "$SELFTEST_RECORD" | wc -l | tr -d ' ')
{
  echo "args=$*"
  for var in WGPU_BACKEND WGPU_DEBUG WGPU_VALIDATION VK_INSTANCE_LAYERS VK_LAYER_ENABLES; do
    echo "$var=${!var-<unset>}"
  done
} >"$SELFTEST_RECORD/$n.txt"
if [ -n "${FAKE_CARGO_FAIL:-}" ] && [[ "$*" == *"$FAKE_CARGO_FAIL"* ]]; then exit 1; fi
exit 0
EOF
cat >"$fake/vulkaninfo" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "${FAKE_VULKANINFO_OUTPUT:-}"
EOF
chmod +x "$fake"/*

layer_present='Instance Layers: count = 1
	VK_LAYER_KHRONOS_validation  Khronos Validation Layer'

failures=0
check() { # check <description> <command...>
  local description=$1
  shift
  if "$@"; then echo "ok   $description"; else echo "FAIL $description"; failures=$((failures + 1)); fi
}

# run <only> [VAR=value...]: run test_all.sh with fakes; sets $status/$output.
run() {
  local only=$1
  shift
  rm -rf "$tmp/record" "$tmp/logs"
  mkdir -p "$tmp/record"
  output=$(env -u VK_INSTANCE_LAYERS -u VK_LAYER_ENABLES -u WGPU_BACKEND -u WGPU_DEBUG -u WGPU_VALIDATION \
    PATH="$fake:$PATH" SELFTEST_RECORD="$tmp/record" TEST_ALL_LOG_DIR="$tmp/logs" \
    "$@" bash "$root/scripts/test_all.sh" --only "$only" --fake-arg 2>&1)
  status=$?
}
recorded() { cat "$tmp/record"/*.txt 2>/dev/null; }
has() { grep -qF -- "$1" <<<"$2"; }
has_line() { grep -qxF -- "$1" <<<"$2"; }

# 1. Validation environment reaches the native GPU suite; layer default applied.
run wgpu-dma-buf VULKANINFO="$fake/vulkaninfo" FAKE_VULKANINFO_OUTPUT="$layer_present"
check "dma-buf suite passes with the layer present" [ "$status" -eq 0 ]
check "WGPU_BACKEND=vulkan" has "WGPU_BACKEND=vulkan" "$(recorded)"
check "WGPU_DEBUG=1" has "WGPU_DEBUG=1" "$(recorded)"
check "WGPU_VALIDATION=1" has "WGPU_VALIDATION=1" "$(recorded)"
check "VK_INSTANCE_LAYERS defaults to the Khronos layer" has "VK_INSTANCE_LAYERS=VK_LAYER_KHRONOS_validation" "$(recorded)"
check "VK_LAYER_ENABLES defaults to sync validation" has "VK_LAYER_ENABLES=VK_VALIDATION_FEATURE_ENABLE_SYNCHRONIZATION_VALIDATION_EXT" "$(recorded)"
check "forwarded cargo arguments reach cargo" has "--fake-arg" "$(recorded)"

# 2. User-configured layers and settings are preserved.
run wgpu-dma-buf VULKANINFO="$fake/vulkaninfo" FAKE_VULKANINFO_OUTPUT="$layer_present" \
  VK_INSTANCE_LAYERS=VK_LAYER_MESA_overlay VK_LAYER_ENABLES=USER_SETTING
check "existing layers keep their order and gain Khronos" has "VK_INSTANCE_LAYERS=VK_LAYER_MESA_overlay:VK_LAYER_KHRONOS_validation" "$(recorded)"
check "existing VK_LAYER_ENABLES is kept" has "VK_LAYER_ENABLES=USER_SETTING" "$(recorded)"
run wgpu-dma-buf VULKANINFO="$fake/vulkaninfo" FAKE_VULKANINFO_OUTPUT="$layer_present" \
  VK_INSTANCE_LAYERS=VK_LAYER_KHRONOS_validation
check "Khronos layer is not added twice" has_line "VK_INSTANCE_LAYERS=VK_LAYER_KHRONOS_validation" "$(recorded)"

# 3. Missing validation layer: no cargo call, PREREQ in summary, non-zero exit.
run wgpu-dma-buf VULKANINFO="$fake/vulkaninfo" FAKE_VULKANINFO_OUTPUT="Instance Layers: count = 0"
check "missing layer fails the run" [ "$status" -ne 0 ]
check "missing layer does not invoke cargo" [ -z "$(recorded)" ]
check "missing layer is reported as PREREQ" has "PREREQ wgpu-dma-buf" "$output"
check "missing layer log gives an install hint" has "vulkan-validation-layers" "$(cat "$tmp/logs/wgpu-dma-buf.log")"

# 4. Missing vulkaninfo: same handling, with its own install hint.
run wgpu-dma-buf VULKANINFO="$tmp/no-such-vulkaninfo"
check "missing vulkaninfo fails the run" [ "$status" -ne 0 ]
check "missing vulkaninfo does not invoke cargo" [ -z "$(recorded)" ]
check "missing vulkaninfo log gives an install hint" has "vulkan-tools" "$(cat "$tmp/logs/wgpu-dma-buf.log")"

# 5. A failing suite does not stop later suites; each keeps its own log.
run dma-buf VULKANINFO="$fake/vulkaninfo" FAKE_VULKANINFO_OUTPUT="$layer_present" FAKE_CARGO_FAIL=gpui-wgpu
check "run fails when one suite fails" [ "$status" -ne 0 ]
check "failing suite is reported" has "FAIL  wgpu-dma-buf" "$output"
check "later suite still runs and passes" has "PASS  media-dma-buf-video" "$output"
check "each suite writes its own log" [ -f "$tmp/logs/wgpu-dma-buf.log" -a -f "$tmp/logs/media-dma-buf-video.log" ]

# 6. Ordinary suites do not receive the validation environment.
run cosmic-text
check "non-GPU suite passes" [ "$status" -eq 0 ]
check "non-GPU suite gets no validation layer" has "VK_INSTANCE_LAYERS=<unset>" "$(recorded)"
check "non-GPU suite gets no WGPU_VALIDATION" has "WGPU_VALIDATION=<unset>" "$(recorded)"

# 7. Platform benchmark compilation is part of the local runner, as in CI.
run gpui-bench-build
check "benchmark build suite passes" [ "$status" -eq 0 ]
check "benchmark suite compiles every benchmark" has "--benches" "$(recorded)"
check "benchmark suite enables CI features" has "--features bench-support,test-support,runtime_shaders" "$(recorded)"
run gpui-bench-build FAKE_CARGO_FAIL=--benches
check "benchmark compilation failure fails the runner" [ "$status" -ne 0 ]
check "benchmark compilation failure appears in the summary" has "FAIL  gpui-bench-build" "$output"

# 8. Every host includes CI's full native-library gate, with failures propagated.
for host in Linux Darwin MINGW64_NT; do
  run native-libraries FAKE_UNAME="$host"
  check "$host includes the full native-library gate" has "--features full-capture,runtime-shaders" "$(recorded)"
  run gpui-bench-build FAKE_UNAME="$host"
  check "$host includes platform benchmark compilation" has "--benches" "$(recorded)"
done
run native-libraries FAKE_CARGO_FAIL=full-capture
check "native compilation failure fails the runner" [ "$status" -ne 0 ]
check "native compilation failure appears in the summary" has "FAIL  native-libraries" "$output"

# 9. Use the CI feature variants, rather than passing only default-feature tests.
run color
check "color tests include ICC" has "--features icc" "$(recorded)"
run canvas
check "canvas tests include GPUI integration" has "--features gpui" "$(recorded)"
run media
check "media tests include wgpu" has "--features wgpu" "$(recorded)"
run parley
check "Linux Parley includes system font coverage" has "system-fonts,native-test,gpui-platform/wayland,gpui-platform/x11" "$(recorded)"
run media-minimal
check "Linux checks media without GPU backends" has "--no-default-features --lib" "$(recorded)"

if [ "$failures" -ne 0 ]; then
  echo "$failures runner self-test check(s) failed."
  exit 1
fi
echo "All runner self-test checks passed."
