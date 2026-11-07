#!/usr/bin/env bash
# Self-test for scripts/test_all.sh: environment propagation, prerequisite
# handling, the shared build and continue-after-failure. Runs test_all.sh as a
# Linux host with fake uname, cargo, test executables and vulkaninfo, so it
# needs no GPU; the small Rust helper is reused after its initial build.
set -uo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
export TEST_ALL_HELPER="${TEST_ALL_HELPER:-$root/target/debug/xgpui-test-support}"
case "$(uname -s)" in MINGW* | MSYS* | CYGWIN*) export TEST_ALL_HELPER="${TEST_ALL_HELPER%.exe}.exe" ;; esac
if [ "${TEST_ALL_HELPER_READY:-0}" != 1 ]; then
  cargo build --locked --manifest-path "$root/tools/release-sync/Cargo.toml" --bin xgpui-test-support --target-dir "$root/target" || exit $?
fi
export TEST_ALL_HELPER_READY=1
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
fake="$tmp/bin"
mkdir -p "$fake"

cat >"$fake/uname" <<'EOF'
#!/usr/bin/env bash
echo "${FAKE_UNAME:-Linux}"
EOF
# Records its arguments and the Vulkan/wgpu environment, one file per call,
# and fails when an argument matches FAKE_CARGO_FAIL. For the shared build
# (--message-format) it also reports a fake test executable for each target
# of every requested package, as Cargo's JSON messages would.
cat >"$fake/cargo" <<'EOF'
#!/usr/bin/env bash
n=$(ls "$SELFTEST_RECORD" | wc -l | tr -d ' ')
{
  echo "cargo args=$*"
  for var in WGPU_BACKEND WGPU_DEBUG WGPU_VALIDATION VK_INSTANCE_LAYERS VK_LAYER_ENABLES; do
    echo "$var=${!var-<unset>}"
  done
} >"$SELFTEST_RECORD/$n.txt"
if [ "${FAKE_WARNINGS:-0}" = 1 ]; then
  printf '%s\n' '{"reason":"compiler-message","package_id":"pkg@1","target":{"name":"pkg","kind":["lib"]},"message":{"level":"warning","message":"example","rendered":"warning: example\n","spans":[]}}'
  echo 'warning: Cargo notice' >&2
  echo '  explanation' >&2
  echo 'test stdout preserved'
fi
if [ -n "${FAKE_CARGO_FAIL:-}" ] && [[ "$*" == *"$FAKE_CARGO_FAIL"* ]]; then exit 1; fi
if [[ "$*" == *--message-format* ]]; then
  while [ $# -gt 0 ]; do
    if [ "$1" = -p ]; then
      package=$2
      mkdir -p "$SELFTEST_PACKAGES/$package"
      for target in lib:x bin:media-lab bin:text-lab bin:color-lab bin:gamut-lab bin:paint-lab \
        test:compatibility test:linux_windows test:construction \
        bench:atlas_upload bench:focus_cleanup bench:inspector_render bench:randomized_element_tree; do
        [ "${FAKE_MISSING_TARGET:-}" != "$target" ] || continue
        kind=${target%%:*} name=${target#*:}
        exe="$SELFTEST_PACKAGES/$package/$kind-$name"
        printf '#!/usr/bin/env bash\nexec "%s" "%s" "$@"\n' "$SELFTEST_TESTEXE" "$package" >"$exe"
        chmod +x "$exe"
        printf '{"reason":"compiler-artifact","package_id":"path+file://%s#0.1.0","manifest_path":"%s/Cargo.toml","target":{"kind":["%s"],"name":"%s"},"profile":{"test":true},"executable":"%s"}\n' \
          "$SELFTEST_PACKAGES/$package" "$SELFTEST_PACKAGES/$package" "$kind" "$name" "$exe"
      done
      shift
    fi
    shift
  done
fi
exit 0
EOF
# Fake test executable: records package, arguments, working directory and the
# Vulkan/wgpu environment; fails when "<package> <args>" matches FAKE_TEST_FAIL.
cat >"$fake/testexe" <<'EOF'
#!/usr/bin/env bash
package=$1
shift
n=$(ls "$SELFTEST_RECORD" | wc -l | tr -d ' ')
{
  echo "test package=$package args=$* cwd=$(pwd)"
  for var in WGPU_BACKEND WGPU_DEBUG WGPU_VALIDATION VK_INSTANCE_LAYERS VK_LAYER_ENABLES; do
    echo "$var=${!var-<unset>}"
  done
} >"$SELFTEST_RECORD/$n.txt"
if [ -n "${FAKE_TEST_FAIL:-}" ] && [[ "$package $*" == *"$FAKE_TEST_FAIL"* ]]; then exit 1; fi
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
  rm -rf "$tmp/record" "$tmp/logs" "$tmp/packages"
  mkdir -p "$tmp/record" "$tmp/packages"
  output=$(env -u VK_INSTANCE_LAYERS -u VK_LAYER_ENABLES -u WGPU_BACKEND -u WGPU_DEBUG -u WGPU_VALIDATION \
    PATH="$fake:$PATH" SELFTEST_RECORD="$tmp/record" TEST_ALL_LOG_DIR="$tmp/logs" \
    SELFTEST_PACKAGES="$tmp/packages" SELFTEST_TESTEXE="$fake/testexe" \
    "$@" bash "$root/scripts/test_all.sh" --only "$only" --fake-arg 2>&1)
  status=$?
}
recorded() { cat "$tmp/record"/*.txt 2>/dev/null; }
# Records of test executables only (the shared build always calls cargo).
tests_run() { grep -l '^test package=' "$tmp/record"/*.txt 2>/dev/null | xargs cat 2>/dev/null; }
cargo_calls() { grep -h '^cargo args=' "$tmp/record"/*.txt 2>/dev/null; }
has() { grep -qF -- "$1" <<<"$2"; }
lacks() { ! has "$@"; }
has_line() { grep -qxF -- "$1" <<<"$2"; }

# 1. Validation environment reaches the native GPU suite; layer default applied.
run wgpu-dma-buf VULKANINFO="$fake/vulkaninfo" FAKE_VULKANINFO_OUTPUT="$layer_present"
check "dma-buf suite passes with the layer present" [ "$status" -eq 0 ]
check "WGPU_BACKEND=vulkan" has "WGPU_BACKEND=vulkan" "$(recorded)"
check "WGPU_DEBUG=1" has "WGPU_DEBUG=1" "$(recorded)"
check "WGPU_VALIDATION=1" has "WGPU_VALIDATION=1" "$(recorded)"
check "VK_INSTANCE_LAYERS defaults to the Khronos layer" has "VK_INSTANCE_LAYERS=VK_LAYER_KHRONOS_validation" "$(recorded)"
check "VK_LAYER_ENABLES defaults to sync validation" has "VK_LAYER_ENABLES=VK_VALIDATION_FEATURE_ENABLE_SYNCHRONIZATION_VALIDATION_EXT" "$(recorded)"
check "forwarded cargo arguments reach the shared build" has "--fake-arg" "$(cargo_calls)"
check "GPU environment reaches the test executable" has "WGPU_VALIDATION=1" "$(tests_run)"
check "suite arguments reach the test executable" has "args=custom_draw::dma_buf::native_tests::dma_buf_native_generated_snapshot --ignored --exact" "$(tests_run)"
check "test executable runs from its package directory" has "cwd=$tmp/packages/xgpui-wgpu" "$(tests_run)"
check "forwarded cargo arguments do not reach test executables" lacks "--fake-arg" "$(tests_run)"

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
check "missing layer does not run the test executable" [ -z "$(tests_run)" ]
check "missing layer is reported as PREREQ" has "PREREQ wgpu-dma-buf" "$output"
check "missing layer log gives an install hint" has "vulkan-validation-layers" "$(cat "$tmp/logs/wgpu-dma-buf.log")"

# 4. Missing vulkaninfo: same handling, with its own install hint.
run wgpu-dma-buf VULKANINFO="$tmp/no-such-vulkaninfo"
check "missing vulkaninfo fails the run" [ "$status" -ne 0 ]
check "missing vulkaninfo does not run the test executable" [ -z "$(tests_run)" ]
check "missing vulkaninfo log gives an install hint" has "vulkan-tools" "$(cat "$tmp/logs/wgpu-dma-buf.log")"

# 5. A failing suite does not stop later suites; each keeps its own log.
run dma-buf VULKANINFO="$fake/vulkaninfo" FAKE_VULKANINFO_OUTPUT="$layer_present" FAKE_TEST_FAIL=xgpui-wgpu
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
check "benchmark executables are never launched" [ -z "$(tests_run)" ]
check "benchmark build suite passes" [ "$status" -eq 0 ]
check "benchmark suite compiles every benchmark" has "--benches" "$(recorded)"
for feature in xgpui-platform/bench-support xgpui-platform/test-support xgpui-platform/runtime_shaders; do
  check "shared build enables $feature" has "$feature" "$(cargo_calls)"
done
run gpui-bench-build FAKE_CARGO_FAIL=--benches
check "benchmark compilation failure fails the runner" [ "$status" -ne 0 ]
check "shared build failure appears in the summary" has "FAIL  (shared build)" "$output"
check "benchmark compilation failure appears in the summary" has "FAIL  gpui-bench-build" "$output"

run gpui-bench-build FAKE_MISSING_TARGET=bench:focus_cleanup
check "missing individual benchmark fails the runner" [ "$status" -ne 0 ]

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
check "color tests include ICC" has "xgpui-color/icc" "$(cargo_calls)"
check "color doctests run with ICC" has "--features icc --doc" "$(cargo_calls)"
run canvas
check "canvas tests include GPUI integration" has "xgpui-canvas/gpui" "$(cargo_calls)"
run media
check "media tests include wgpu" has "xgpui-media/wgpu" "$(cargo_calls)"
run parley
for feature in xgpui-parley/system-fonts xgpui-parley/native-test xgpui-platform/wayland xgpui-platform/x11; do
  check "Linux Parley build includes $feature" has "$feature" "$(cargo_calls)"
done

# 10. One shared build serves every suite; a failed build fails them all.
run gpui
check "shared build runs once for several suites" [ "$(cargo_calls | grep -c -- --message-format)" -eq 1 ]
check "every gpui suite runs" has "PASS  gpui-bench-context" "$output"
run gpui FAKE_CARGO_FAIL=--message-format
check "failed shared build fails the run" [ "$status" -ne 0 ]
check "suites fail rather than pass after a failed build" has "FAIL  gpui-core" "$output"
check "no test executable runs after a failed build" [ -z "$(tests_run)" ]
run media-minimal
check "Linux checks media without GPU backends" has "--no-default-features --lib" "$(recorded)"


# Warning capture covers both the shared build and isolated Cargo invocations.
run color FAKE_WARNINGS=1
check "warning capture passes across shared and isolated Cargo" [ "$status" -eq 0 ]
check "compiler warnings are rendered" has "warning: example" "$output"
check "ordinary stdout is preserved" has "test stdout preserved" "$output"
check "reports deduplicate shared and isolated warnings" has "Warnings: 2 unique" "$output"
check "warning report retains both invocations" has '"invocations":["build","color"]' "$(cat "$tmp/logs/warnings.jsonl")"
check "raw shared diagnostics retained" [ -s "$tmp/logs/cargo/build.jsonl" ]
check "raw isolated diagnostics retained" [ -s "$tmp/logs/cargo/color.jsonl" ]
run release-sync FAKE_WARNINGS=1 FAKE_CARGO_FAIL=--fake-arg
check "captured Cargo failure remains a failure" [ "$status" -ne 0 ]
check "warnings survive Cargo failure" has "Warnings: 2 unique" "$output"

if [ "$failures" -ne 0 ]; then
  echo "$failures runner self-test check(s) failed."
  exit 1
fi
echo "All runner self-test checks passed."
