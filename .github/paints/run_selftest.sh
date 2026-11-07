#!/usr/bin/env bash
# Contract checks for .github/paints/run.sh and commands.tsv; no GPU or build.
# Uses fake cargo and rustc on PATH and a temporary results directory.
set -uo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
runner="$root/.github/paints/run.sh"
inventory="$root/.github/paints/commands.tsv"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

failures=0
check() { # check <description> <command...>
  local description=$1
  shift
  if "$@"; then echo "ok   $description"; else echo "FAIL $description"; failures=$((failures + 1)); fi
}
has() { grep -qF -- "$1" <<<"$2"; }

# 1. Inventory: step counts per backend, --locked everywhere, known scopes,
#    and a lockfile for every manifest (the root lock for workspace members).
steps() { grep -v '^#' "$inventory" | awk -F'\t' -v b="$1" '$1 == b' | wc -l | tr -d ' '; }
check "linux-vulkan has 6 steps" [ "$(steps linux-vulkan)" -eq 6 ]
check "windows-wgpu has 6 steps" [ "$(steps windows-wgpu)" -eq 6 ]
check "windows-native has 12 steps" [ "$(steps windows-native)" -eq 12 ]
check "every row has four fields" [ -z "$(grep -v '^#' "$inventory" | awk -F'\t' 'NF != 4')" ]
check "every command uses --locked" [ -z "$(grep -v '^#' "$inventory" | cut -f4 | grep -v -- ' --locked ')" ]
check "every scope is known" [ -z "$(grep -v '^#' "$inventory" | cut -f2 | grep -vxE 'color-gamut|producer-cache|paint-parity')" ]
check "commands contain no quotes" [ -z "$(grep -v '^#' "$inventory" | cut -f4 | grep -E "[\"'\\\\]")" ]
members=$(sed -n '/^members = \[/,/^\]/p' "$root/Cargo.toml")
missing=""
while read -r manifest; do
  dir=${manifest%/Cargo.toml}
  if has "\"$dir\"" "$members"; then lock="$root/Cargo.lock"; else lock="$root/$dir/Cargo.lock"; fi
  [ -f "$lock" ] || missing+="$manifest "
done < <(grep -v '^#' "$inventory" | cut -f4 | grep -oE -- '--manifest-path [^ ]+' | cut -d' ' -f2 | sort -u)
check "every manifest has a lockfile for --locked" [ -z "$missing" ]
for backend in linux-vulkan windows-wgpu windows-native; do
  last=$(grep -v '^#' "$inventory" | awk -F'\t' -v b="$backend" '$1 == b' | tail -1 | cut -f4)
  check "$backend ends with the brush tests" has "brush::tests" "$last"
done

# 2. Dry run: phases and scope selection.
fixture="$tmp/fixture/.github/paints"
mkdir -p "$fixture"
cp "$runner" "$fixture/run.sh"
lf_inventory=$(tr -d '\r' <"$inventory")
printf '%s\n' "$lf_inventory" >"$fixture/commands.tsv"
lf_plan=$(bash "$fixture/run.sh" --backend windows-native --dry-run)
printf '%s\n' "$lf_inventory" | sed 's/$/\r/' >"$fixture/commands.tsv"
crlf_plan=$(bash "$fixture/run.sh" --backend windows-native --dry-run)
check "LF and CRLF inventories produce identical phases" [ "$lf_plan" = "$crlf_plan" ]
check "CRLF inventory leaves no carriage returns in commands" test "${crlf_plan//$'\r'/}" = "$crlf_plan"

plan=$(bash "$runner" --backend windows-native --scope full --dry-run)
check "test steps build with --no-run before testing" has "build	cargo test --locked --config .cargo/validation.toml --manifest-path extensions/crates/xgpui-color/Cargo.toml --no-run" "$plan"
check "test arguments after -- are not built" has "build	cargo test --locked --config .cargo/validation.toml --manifest-path gpui/crates/gpui-windows/Cargo.toml --features test-support --lib dx11_custom --no-run" "$plan"
check "test phase keeps the full command" has "test	cargo test --locked --config .cargo/validation.toml --manifest-path gpui/crates/gpui-windows/Cargo.toml --features test-support --lib dx11_custom -- --nocapture" "$plan"
check "run steps build with cargo build" has "build	cargo build --locked --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin paint-lab --features portable-shaders" "$plan"
check "capture phase keeps the run arguments" has "capture	cargo run --locked" "$plan"
check "check steps are build only" [ "$(grep -c 'cargo check' <<<"$plan")" -eq 1 ]
focused=$(bash "$runner" --backend windows-native --scope color-gamut --dry-run)
check "focused scope selects only its steps" [ "$(grep -vc $'^ ' <<<"$focused")" -eq 6 ]
bash "$runner" --backend linux-vulkan --scope color-gamut --dry-run >/dev/null 2>&1
check "unavailable scope is rejected" [ $? -eq 2 ]
bash "$runner" --backend nowhere --dry-run >/dev/null 2>&1
check "unknown backend is rejected" [ $? -eq 2 ]

# 3. Execution with fake cargo: results distinguish full, focused and failure.
fake="$tmp/bin"
mkdir -p "$fake"
cat >"$fake/cargo" <<'EOF'
#!/usr/bin/env bash
echo "fake cargo $*"
for arg in "$@"; do
  if [[ "$arg" == *$'\r'* ]]; then echo "unexpected carriage return in cargo argument" >&2; exit 9; fi
done
if [ -n "${FAKE_CARGO_FAIL:-}" ] && [[ "$*" == *"$FAKE_CARGO_FAIL"* ]]; then exit 7; fi
exit 0
EOF
printf '#!/usr/bin/env bash\necho "rustc 0.0.0-fake"\n' >"$fake/rustc"
chmod +x "$fake"/*
results="$root/paint-results"
backup=""
if [ -e "$results" ]; then backup="$tmp/paint-results"; mv "$results" "$backup"; fi
restore() { rm -rf "$results"; [ -z "$backup" ] || mv "$backup" "$results"; rm -rf "$tmp"; }
trap restore EXIT
execute() { # execute <scope> [VAR=value...]
  local scope=$1
  shift
  rm -rf "$results"
  env PATH="$fake:$PATH" CARGO_TARGET_DIR="$tmp/target" GITHUB_SHA=test-sha \
    GITHUB_STEP_SUMMARY="$tmp/summary" "$@" bash "$runner" --backend windows-native --scope "$scope" >"$tmp/out" 2>&1
  status=$?
}
execute full
check "full run passes" [ "$status" -eq 0 ]
check "full run is acceptance" has '"acceptance": true' "$(cat "$results/timings.json")"
check "every phase is recorded" [ "$(grep -c '"phase"' "$results/timings.json")" -eq 23 ]
check "logs are written per phase" [ -f "$results/00-build.log" ] && [ -f "$results/10-capture.log" ]
check "summary is written" has "Full backend acceptance: **true**" "$(cat "$results/summary.md")"
check "GitHub step summary is appended" has "## windows-native / full" "$(cat "$tmp/summary")"
check "timings are valid JSON" bash -c "command -v jq >/dev/null || exit 0; jq -e . '$results/timings.json' >/dev/null"
execute producer-cache
check "focused run passes" [ "$status" -eq 0 ]
check "focused run is not acceptance" has '"acceptance": false' "$(cat "$results/timings.json")"
execute full FAKE_CARGO_FAIL=dx11_custom
check "failing command fails the run with its status" [ "$status" -eq 7 ]
check "failure is recorded" has '"exit_code": 7' "$(cat "$results/timings.json")"
check "failed run is not acceptance" has '"acceptance": false' "$(cat "$results/timings.json")"
check "run stops at the first failure" [ "$(grep -c '"phase"' "$results/timings.json")" -eq 4 ]
env -u CARGO_TARGET_DIR bash "$runner" --backend windows-native >/dev/null 2>&1
check "missing CARGO_TARGET_DIR is rejected" [ $? -eq 2 ]

if [ "$failures" -ne 0 ]; then
  echo "$failures paint runner self-test check(s) failed."
  exit 1
fi
echo "All paint runner self-test checks passed."
