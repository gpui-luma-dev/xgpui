#!/usr/bin/env bash
# Preview or remove ignored Cargo target directories throughout this checkout.
#
#   scripts/clean_build_outputs.sh           list what would be removed
#   scripts/clean_build_outputs.sh --apply   remove it
#
# Every target directory is checked before anything is deleted: the run stops
# if one contains tracked files or is not ignored by git.
set -euo pipefail

apply=0
case "${1-}" in
  "") ;;
  --apply) apply=1 ;;
  -h | --help) sed -n '2,8p' "$0"; exit 0 ;;
  *) echo "usage: $0 [--apply]" >&2; exit 2 ;;
esac

cd "$(dirname "$0")/.."

# Directories named target, excluding .git and symlinks, without descending
# into a target directory (find -type d does not match symlinks by default).
# Listed top-down: sorted by parent directory one path component at a time
# (separators become \001 so they sort before any name character).
keyed() {
  local path parent
  while IFS= read -r -d '' path; do
    path=${path#./}
    parent=${path%target}
    printf '%s\002%s\0' "${parent//\//$'\001'}" "$path"
  done
}
targets=()
while IFS= read -r -d '' record; do
  targets+=("${record#*$'\002'}")
done < <(find . -name .git -prune -o -type d -name target -prune -print0 | keyed | LC_ALL=C sort -z)

for relative in ${targets[@]+"${targets[@]}"}; do
  if [ -n "$(git ls-files -- "$relative")" ]; then
    echo "Refusing to clean tracked files in $relative" >&2
    exit 1
  fi
  if ! git check-ignore -q -- "$relative/"; then
    echo "Refusing to clean non-ignored directory $relative" >&2
    exit 1
  fi
done

for relative in ${targets[@]+"${targets[@]}"}; do
  if [ "$apply" -eq 1 ]; then
    echo "Removing $relative"
    rm -rf -- "$relative"
  else
    echo "Would remove $relative"
  fi
done

if [ "$apply" -eq 1 ]; then
  echo "${#targets[@]} target directories."
else
  echo "${#targets[@]} target directories. Run with --apply to remove them."
fi
