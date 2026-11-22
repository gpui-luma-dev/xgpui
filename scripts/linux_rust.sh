#!/usr/bin/env bash
# Step 2 of the Linux setup (#389): install the stable Rust toolchain for your
# user with rustup, the way CI gets it. Run it as yourself, not root.
# See docs/LINUX-SETUP.md.
#
# Usage: scripts/linux_rust.sh [options]
#   --dry-run   check the system and print the commands; change nothing
#   -h, --help  show this help
#
# What it does:
#   - rustup already installed: installs/updates stable with rustfmt and
#     clippy and makes it the default
#   - otherwise: installs the distribution's rustup package if it has one
#     (Fedora, openSUSE, Arch, Debian 13+, Ubuntu 24.04+; uses sudo), else
#     runs the official installer from https://sh.rustup.rs, as Zed's
#     script/linux does; then installs stable into ~/.rustup and ~/.cargo
#   - never edits your shell profile; it prints the line to add instead
#
# Safe to re-run. Undo with `rustup self uninstall` (and remove the
# distribution's rustup package if this script installed it).
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=linux_common.sh
. "$here/linux_common.sh"

usage() { sed -n '2,/^set -euo/{/^set -euo/d;s/^# \{0,1\}//;p;}' "$0"; }
die() { printf 'linux_rust: error: %s\n' "$*" >&2; exit 1; }
note() { printf 'linux_rust: %s\n' "$*"; }

dry_run=0
while (($#)); do
  case "$1" in
    --dry-run) dry_run=1 ;;
    -h | --help) usage; exit 0 ;;
    *) usage >&2; die "unknown option: $1" ;;
  esac
  shift
done

run() {
  echo "+ $*"
  if ((!dry_run)); then "$@"; fi
}

if ! detect_distro; then
  [[ $(uname -s) == Linux || -n ${OS_RELEASE:-} ]] || die "this is $(uname -s), not Linux."
  note "warning: unrecognized distribution (${distro_name:-unknown}); using the rustup.rs installer."
fi
((EUID != 0)) ||
  die "don't run this as root: rustup installs into the current user's home. Run it as your normal user; it uses sudo only for the package manager."

# Distribution Rust packages put cargo in /usr/bin and lag behind stable.
for p in /usr/bin/cargo /usr/bin/rustc; do
  if [[ -x $p ]] && ! [[ -L $p && $(readlink -f "$p") == *rustup* ]]; then
    note "warning: $p is a distribution Rust, not rustup's. Keep ~/.cargo/bin before /usr/bin on PATH, or remove the distribution's rust/cargo package."
    break
  fi
done

components=(--component rustfmt --component clippy)
cargo_home="${CARGO_HOME:-$HOME/.cargo}"

rustup_bin=''
if command -v rustup >/dev/null 2>&1; then rustup_bin=$(command -v rustup)
elif [[ -x $cargo_home/bin/rustup ]]; then rustup_bin=$cargo_home/bin/rustup
fi

if [[ -n $rustup_bin ]]; then
  note "rustup found ($rustup_bin); installing or updating stable."
else
  # The distribution package, when there is one: signed and updated by the
  # package manager. Fedora's ships rustup-init; the others ship rustup.
  if [[ -n $distro_family ]] && command -v "$pkg_manager" >/dev/null 2>&1 &&
    { package_installed rustup || package_available rustup; }; then
    if ! package_installed rustup; then
      find_root_command || die "installing the rustup package needs root, and neither sudo nor doas is installed."
      case "$pkg_manager" in
        dnf) run "${as_root[@]}" dnf install -y rustup ;;
        apt-get) run "${as_root[@]}" apt-get install -y rustup ;;
        zypper) run "${as_root[@]}" zypper --non-interactive install rustup ;;
        pacman) run "${as_root[@]}" pacman -S --needed --noconfirm rustup ;;
      esac
    fi
    if command -v rustup-init >/dev/null 2>&1 || [[ $pkg_manager == dnf ]]; then
      run rustup-init -y --no-modify-path --default-toolchain none
      rustup_bin=$cargo_home/bin/rustup
    else
      rustup_bin=rustup
    fi
  else
    command -v curl >/dev/null 2>&1 || die "no rustup package for this distribution and curl isn't installed to fetch the installer. Install curl, or Rust from https://rustup.rs."
    note "no rustup package in the repositories; using the official installer from https://sh.rustup.rs"
    echo "+ curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path --default-toolchain none"
    if ((!dry_run)); then
      curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path --default-toolchain none ||
        die "the rustup installer failed. Check network access to static.rust-lang.org."
    fi
    rustup_bin=$cargo_home/bin/rustup
  fi
fi

run "$rustup_bin" toolchain install stable --profile minimal "${components[@]}"
run "$rustup_bin" default stable

if ((dry_run)); then
  note "dry run: nothing installed."
  exit 0
fi

# Check the result through ~/.cargo/bin (or the distribution rustup's proxies).
original_path=$PATH
PATH="$cargo_home/bin:$PATH"
for tool in rustc cargo rustfmt cargo-clippy; do
  command -v "$tool" >/dev/null 2>&1 || die "$tool not found after installing. Run: rustup component add rustfmt clippy"
done
note "$(rustc --version)"

if [[ -x $cargo_home/bin/rustc && ":$original_path:" != *":$cargo_home/bin:"* ]]; then
  cat <<EOF

linux_rust: $cargo_home/bin is not on your PATH yet. For new shells add
  . "$cargo_home/env"
to ~/.bashrc (or ~/.zshrc), then open a new terminal or run that line now.
EOF
fi

cat <<'EOF'

linux_rust: done. Next: scripts/linux_doctor.sh to check the whole setup.
EOF
