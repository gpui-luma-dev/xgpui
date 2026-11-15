#!/usr/bin/env bash
# Runs a Cargo command and fails if it fails or prints any warning: rustc
# warnings, Cargo manifest lints and build-script warnings alike.
#
# Usage: no-warnings.sh <label> <command...>
set -uo pipefail

label=$1
shift
log=$(mktemp)

"$@" 2>&1 | tee "$log"
status=${PIPESTATUS[0]}

if [[ $status -ne 0 ]]; then
  echo "::error title=$label failed::$* exited with $status"
  exit "$status"
fi

count=$(grep -cE '^warning(\[|:)' "$log" | tr -d ' ')
if [[ $count -ne 0 ]]; then
  echo "::error title=$label: $count warning lines::$*"
  grep -nE '^warning(\[|:)' "$log" | head -50
  exit 1
fi
echo "$label: no warnings"
