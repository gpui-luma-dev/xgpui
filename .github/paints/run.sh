#!/usr/bin/env bash
# Run the paint GPU command inventory (commands.tsv) for one backend, timing
# the build separately from test execution or native capture.
#
#   run.sh --backend BACKEND [--scope SCOPE] [--dry-run]
#
# Each `cargo test` step first runs the same command with `--no-run` (build),
# then the test. Each `cargo run` step first runs `cargo build` with the same
# arguments, then the capture. `cargo check` is a build only. Logs, timings.json
# and summary.md go to paint-results/; the first failing command stops the run.
set -uo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
inventory="$root/.github/paints/commands.tsv"
scopes=(full color-gamut producer-cache paint-parity)

usage() {
  echo "usage: run.sh --backend BACKEND [--scope SCOPE] [--dry-run]" >&2
  echo "$1" >&2
  exit 2
}

backend="" scope=full dry_run=0
while [ $# -gt 0 ]; do
  case "$1" in
    --backend) backend="${2:-}"; shift ;;
    --scope) scope="${2:-}"; shift ;;
    --dry-run) dry_run=1 ;;
    *) usage "unknown argument: $1" ;;
  esac
  shift
done

backends=$(grep -v '^#' "$inventory" | cut -f1 | sort -u | tr '\n' ' ')
[ -n "$backend" ] || usage "--backend is required (one of: $backends)"
[[ " $backends " == *" $backend "* ]] || usage "unknown backend $backend (one of: $backends)"
[[ " ${scopes[*]} " == *" $scope "* ]] || usage "unknown scope $scope (one of: ${scopes[*]})"

names=() commands=()
while IFS=$'\t' read -r step_backend step_scope name command; do
  [[ "$step_backend" == "#"* ]] && continue
  [ "$step_backend" = "$backend" ] || continue
  [ "$scope" = full ] || [ "$step_scope" = "$scope" ] || continue
  names+=("$name")
  commands+=("$command")
done <"$inventory"
[ ${#names[@]} -gt 0 ] || usage "$scope is unavailable for $backend; use windows-native for color-gamut"

# phases <command>: print "phase<TAB>command" lines. Commands contain no quotes,
# so splitting on spaces is exact.
phases() {
  local -a args build
  read -ra args <<<"$1"
  local i split=${#args[@]}
  for i in "${!args[@]}"; do
    if [ "${args[$i]}" = -- ]; then split=$i; break; fi
  done
  build=("${args[@]:0:$split}")
  case "${args[1]}" in
    check) printf 'build\t%s\n' "${args[*]}" ;;
    test)
      printf 'build\t%s --no-run\n' "${build[*]}"
      printf 'test\t%s\n' "${args[*]}"
      ;;
    run)
      build[1]=build
      printf 'build\t%s\n' "${build[*]}"
      printf 'capture\t%s\n' "${args[*]}"
      ;;
    *) echo "run.sh: unsupported cargo subcommand in: $1" >&2; return 1 ;;
  esac
}

if [ "$dry_run" = 1 ]; then
  for i in "${!names[@]}"; do
    echo "${names[$i]}"
    phases "${commands[$i]}" | sed 's/^/  /' || exit 1
  done
  exit 0
fi

[ -n "${CARGO_TARGET_DIR:-}" ] || usage "set an absolute CARGO_TARGET_DIR to share builds across manifests"

now() { if [ -n "${EPOCHREALTIME:-}" ]; then echo "${EPOCHREALTIME/,/.}"; else date +%s; fi; }
elapsed() { awk -v a="$1" -v b="$2" 'BEGIN { printf "%.3f", b - a }'; }
json() { # json <string>: a JSON string literal
  local s=$1
  s=${s//\\/\\\\}
  s=${s//\"/\\\"}
  s=${s//$'\r'/}
  s=${s//$'\t'/\\t}
  s=${s//$'\n'/\\n}
  printf '"%s"' "$s"
}

output="$root/paint-results"
mkdir -p "$output"
sha=${GITHUB_SHA:-$(git -C "$root" rev-parse HEAD)}
setup_seconds=$(elapsed "${SETUP_STARTED:-$(now)}" "$(now)")
rustc_version=$(rustc -Vv)
results=() status=0 acceptance=false
build_total=0.0 test_total=0.0 capture_total=0.0

finish() {
  {
    printf '{\n'
    printf '  "backend": %s,\n  "scope": %s,\n  "sha": %s,\n' "$(json "$backend")" "$(json "$scope")" "$(json "$sha")"
    printf '  "cache_mode": %s,\n  "cache_match": %s,\n' "$(json "${CACHE_MODE:-local}")" "$(json "${CACHE_MATCH:-}")"
    printf '  "setup_seconds": %s,\n  "rustc": %s,\n' "$setup_seconds" "$(json "$rustc_version")"
    printf '  "target_dir": %s,\n  "acceptance": %s,\n  "results": [' "$(json "$CARGO_TARGET_DIR")" "$acceptance"
    local i
    for i in "${!results[@]}"; do
      [ "$i" -eq 0 ] || printf ','
      printf '\n    %s' "${results[$i]}"
    done
    printf '\n  ]\n}\n'
  } >"$output/timings.json"
  local summary
  summary="## $backend / $scope

Commit: \`$sha\`

Full backend acceptance: **$acceptance**. Focused runs are diagnostic only.

Cache: \`${CACHE_MODE:-local}\`; restored key: \`${CACHE_MATCH:-miss}\`

Setup: $(awk -v s="$setup_seconds" 'BEGIN { printf "%.1f", s }')s; build: ${build_total}s; test: ${test_total}s; capture: ${capture_total}s.
"
  printf '%s' "$summary" >"$output/summary.md"
  [ -z "${GITHUB_STEP_SUMMARY:-}" ] || printf '%s' "$summary" >>"$GITHUB_STEP_SUMMARY"
}

cd "$root" || exit 1
for i in "${!names[@]}"; do
  plan=$(phases "${commands[$i]}") || { status=2; break; }
  while IFS=$'\t' read -r phase command; do
    echo "::group::${names[$i]} [$phase]"
    echo "$command"
    started=$(now)
    read -ra args <<<"$command"
    "${args[@]}" </dev/null 2>&1 | tee "$output/$(printf '%02d' "$i")-$phase.log"
    status=${PIPESTATUS[0]}
    seconds=$(elapsed "$started" "$(now)")
    echo "::endgroup::"
    results+=("{\"name\": $(json "${names[$i]}"), \"phase\": $(json "$phase"), \"command\": $(json "$command"), \"seconds\": $seconds, \"exit_code\": $status}")
    case "$phase" in
      build) build_total=$(awk -v a="$build_total" -v b="$seconds" 'BEGIN { printf "%.1f", a + b }') ;;
      test) test_total=$(awk -v a="$test_total" -v b="$seconds" 'BEGIN { printf "%.1f", a + b }') ;;
      capture) capture_total=$(awk -v a="$capture_total" -v b="$seconds" 'BEGIN { printf "%.1f", a + b }') ;;
    esac
    [ "$status" -eq 0 ] || break
  done <<<"$plan"
  [ "$status" -eq 0 ] || break
done
[ "$status" -ne 0 ] || [ "$scope" != full ] || acceptance=true
finish
exit "$status"
