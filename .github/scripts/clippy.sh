#!/usr/bin/env bash
# Clippy policy (#280), shared by CI and `just clippy`. Fails on any warning.
#
# - Upstream-derived crates (gpui/crates/*): the default Clippy set minus the
#   `style` group and a few design-level lints, which upstream Zed allows in its
#   own workspace and which would cause churn on every upstream sync.
# - Our crates (everything else in the workspace): the full default set.
#
# The policy lives here, not in upstream sources or manifests, so syncs and
# `release-sync normalize` never touch it.
#
# Usage: clippy.sh [upstream|ours]   (default: both)
set -euo pipefail
cd "$(dirname "$0")/../.."

upstream_allow=(
  -A clippy::style
  -A clippy::too_many_arguments
  -A clippy::type_complexity
  -A clippy::large_enum_variant
)

upstream=()
ours=()
while read -r member; do
  name=$(sed -n 's/^name = "\(.*\)"/\1/p' "$member/Cargo.toml" | head -1)
  case "$member" in
    gpui/crates/*) upstream+=(-p "$name") ;;
    tools/build-all) ;; # compiles its dependencies only; no code of its own
    *) ours+=(-p "$name") ;;
  esac
done < <(sed -n '/^members = \[/,/^\]/p' Cargo.toml | sed -n 's/^ *"\([^"]*\)",*$/\1/p')

run() {
  echo "== clippy: $1"
  shift
  cargo clippy --locked --all-targets --all-features --no-deps "$@"
}

which=${1:-both}
if [[ $which == both || $which == upstream ]]; then
  run "upstream-derived crates" "${upstream[@]}" -- -D warnings "${upstream_allow[@]}"
fi
if [[ $which == both || $which == ours ]]; then
  run "our crates" "${ours[@]}" -- -D warnings
fi
