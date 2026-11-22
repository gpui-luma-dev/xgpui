#!/usr/bin/env bash
# Self-test for scripts/linux_setup.sh and linux_doctor.sh (#389). Runs them
# against fake os-release files and fake package managers, so it needs no
# Linux machine and changes nothing. Runs in `just test` as linux-setup-selftest.
#
# Covers: package selection per family, refusing unknown package names,
# virtual package names that a different package provides (#429), idempotent re-runs and the saved undo command.
set -uo pipefail

if ((BASH_VERSINFO[0] < 4)); then
  echo "linux_setup_selftest: skipped (needs bash 4 or newer, this is $BASH_VERSION)"
  exit 0
fi

root="$(cd "$(dirname "$0")/.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
fake="$tmp/bin"
mkdir -p "$fake"
export FAKE_DB="$tmp/installed" # one "name<TAB>provides,provides" line per installed package
export FAKE_UNAVAILABLE=''      # names the fake repositories don't have
export FAKE_PROVIDED_BY=''      # "virtual=real ..." names apt resolves to another package
export XDG_STATE_HOME="$tmp/state"

cat >"$fake/sudo" <<'EOF'
#!/usr/bin/env bash
case "$1" in -n | -v) exit 0 ;; esac
exec "$@"
EOF

# Installs a name, or the package FAKE_PROVIDED_BY maps it to (which then
# "Provides" the requested name, like apt resolving a virtual package).
cat >"$fake/fake-install" <<'EOF'
#!/usr/bin/env bash
for name in "$@"; do
  real=$name provides=''
  for map in $FAKE_PROVIDED_BY; do
    if [[ ${map%%=*} == "$name" ]]; then real=${map#*=} provides=$name; fi
  done
  printf '%s\t%s\n' "$real" "$provides" >>"$FAKE_DB"
done
EOF

cat >"$fake/apt-get" <<'EOF'
#!/usr/bin/env bash
[[ $1 == install ]] || exit 0
shift
names=()
for a in "$@"; do [[ $a == -* ]] || names+=("$a"); done
fake-install "${names[@]}"
EOF
cat >"$fake/apt-cache" <<'EOF'
#!/usr/bin/env bash
name=${*: -1}
[[ " $FAKE_UNAVAILABLE " != *" $name "* ]]
EOF
# dpkg-query -W -f=FORMAT [name]: with a name, that package's status; without
# one, every installed package (FORMAT's ${Provides} filled in).
cat >"$fake/dpkg-query" <<'EOF'
#!/usr/bin/env bash
format='' name=''
for a in "$@"; do
  case "$a" in -W) ;; -f=*) format=${a#-f=} ;; -f) ;; *) [[ -z $format ]] && format=$a || name=$a ;; esac
done
[[ -f $FAKE_DB ]] || touch "$FAKE_DB"
found=1
while IFS=$'\t' read -r pkg provides; do
  if [[ -z $name || $pkg == "$name" ]]; then
    line=${format//'${db:Status-Status}'/installed}
    line=${line//'${Package}'/$pkg}
    line=${line//'${Provides}'/$provides}
    printf '%b' "$line"
    found=0
  fi
done <"$FAKE_DB"
exit "$found"
EOF

cat >"$fake/dnf" <<'EOF'
#!/usr/bin/env bash
case " $* " in
  *" repoquery "*)
    for a in "$@"; do
      case "$a" in -* | repoquery | 1 | '%{name}\n') ;; *) [[ " $FAKE_UNAVAILABLE " == *" $a "* ]] || echo "$a" ;; esac
    done ;;
  *" install "*)
    names=()
    for a in "$@"; do [[ $a == -* || $a == install ]] || names+=("$a"); done
    fake-install "${names[@]}" ;;
  *" history list"*) printf 'ID Command line\n 42 dnf install\n' ;;
esac
EOF
cat >"$fake/rpm" <<'EOF'
#!/usr/bin/env bash
name=${*: -1}
[[ -f $FAKE_DB ]] && awk -F'\t' -v n="$name" '$1 == n || index("," $2 ",", "," n ",") { f = 1 } END { exit !f }' "$FAKE_DB"
EOF
chmod +x "$fake"/*
export PATH="$fake:$PATH"

printf 'ID=ubuntu\nID_LIKE=debian\nVERSION_ID=26.04\nPRETTY_NAME="Ubuntu 26.04 LTS"\n' >"$tmp/ubuntu"
printf 'ID=fedora\nVERSION_ID=43\nPRETTY_NAME="Fedora Linux 43"\n' >"$tmp/fedora"
printf 'ID=void\nPRETTY_NAME="Void Linux"\n' >"$tmp/void"

failures=0
run() { # run <name> <expected-exit> <command...>; output in $tmp/out
  local name=$1 want=$2
  shift 2
  "$@" >"$tmp/out" 2>&1
  local got=$?
  if [[ $got != "$want" ]]; then
    echo "FAIL $name: exit $got, expected $want"
    sed 's/^/    /' "$tmp/out"
    failures=$((failures + 1))
    return 1
  fi
}
expect() { # expect <name> <fixed string in $tmp/out>
  if ! grep -qF -- "$2" "$tmp/out"; then
    echo "FAIL $1: output lacks: $2"
    sed 's/^/    /' "$tmp/out"
    failures=$((failures + 1))
  fi
}
reset() { : >"$FAKE_DB"; rm -rf "$XDG_STATE_HOME"; FAKE_UNAVAILABLE='' FAKE_PROVIDED_BY=''; }
setup() { OS_RELEASE=$1 "$BASH" "$root/scripts/linux_setup.sh" "${@:2}"; }

# Dry run picks each family's names and installs nothing.
reset
run "ubuntu dry run" 0 setup "$tmp/ubuntu" --all --dry-run &&
  expect "ubuntu dry run" "libfontconfig1-dev" &&
  expect "ubuntu dry run" "libgtk-3-dev"
[[ -s $FAKE_DB ]] && { echo "FAIL ubuntu dry run: installed packages"; failures=$((failures + 1)); }
run "fedora dry run" 0 setup "$tmp/fedora" --dry-run && expect "fedora dry run" "dnf install gcc gcc-c++"

# Unknown names are all listed, and nothing is installed.
reset
FAKE_UNAVAILABLE='weston mutter'
run "missing names" 1 setup "$tmp/ubuntu" --headless-tests -y && expect "missing names" "not found in the enabled repositories: weston mutter"
[[ -s $FAKE_DB ]] && { echo "FAIL missing names: installed packages"; failures=$((failures + 1)); }

# A name apt resolves to another package counts as installed after the
# install. #429 hit this with libfreetype6-dev on new Ubuntu (now listed as
# libfreetype-dev); pkg-config, which pkgconf provides, stands in for it here.
reset
FAKE_PROVIDED_BY='pkg-config=pkgconf'
run "virtual package" 0 setup "$tmp/ubuntu" -y && expect "virtual package" "undo with: sudo apt-get remove"
run "virtual package rerun" 0 setup "$tmp/ubuntu" -y && expect "virtual package rerun" "0 to install"

# Re-running installs nothing; dnf's undo names the transaction.
reset
run "fedora install" 0 setup "$tmp/fedora" -y && expect "fedora install" "sudo dnf history undo 42"
run "fedora rerun" 0 setup "$tmp/fedora" -y && expect "fedora rerun" "nothing to install"
if ! grep -qs "sudo dnf history undo 42" "$XDG_STATE_HOME"/xgpui/linux-setup-*.txt; then
  echo "FAIL fedora install: undo command not saved"
  failures=$((failures + 1))
fi

# Unsupported distributions stop before doing anything.
reset
run "unsupported" 1 setup "$tmp/void" --dry-run && expect "unsupported" "unsupported distribution: Void Linux"

# The doctor runs read-only on any machine and names this family's packages.
run "doctor" 1 env OS_RELEASE="$tmp/ubuntu" PKG_CONFIG_PATH=/nonexistent PKG_CONFIG_LIBDIR=/nonexistent \
  "$BASH" "$root/scripts/linux_doctor.sh" && expect "doctor" "sudo apt-get install"

if ((failures)); then
  echo "linux_setup_selftest: $failures failed"
  exit 1
fi
echo "linux_setup_selftest: ok"
