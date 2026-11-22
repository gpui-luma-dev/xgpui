#!/usr/bin/env bash
# Step 1 of the Linux setup (#389): install the system packages needed to
# build and run xgpui, with the distribution's package manager. Supports the
# Fedora/RHEL (dnf), Debian/Ubuntu (apt), openSUSE (zypper) and Arch (pacman)
# families. The package list is scripts/linux_packages.txt; see
# docs/LINUX-SETUP.md for what each package is for.
#
# Usage: scripts/linux_setup.sh [options]
#   --luma            also install GTK 3 headers (xgpui-luma's apps)
#   --headless-tests  also install Xvfb, Weston, Openbox, Mutter and X11 tools
#                     for .github/linux/window-smoke.sh and mutter-smoke.sh
#   --all             everything above
#   --dry-run         check the system and print the install command; change nothing
#   -y, --yes         don't ask the package manager to confirm
#   -h, --help        show this help
#
# Safe to re-run: packages that are already installed are skipped. The exact
# undo command is printed at the end and saved under
# ~/.local/state/xgpui/. Rust is step 2: scripts/linux_rust.sh.
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=linux_common.sh
. "$here/linux_common.sh"

usage() { sed -n '2,/^set -euo/{/^set -euo/d;s/^# \{0,1\}//;p;}' "$0"; }
die() { printf 'linux_setup: error: %s\n' "$*" >&2; exit 1; }
note() { printf 'linux_setup: %s\n' "$*"; }

groups=(base)
dry_run=0
assume_yes=0
while (($#)); do
  case "$1" in
    --luma) groups+=(luma) ;;
    --headless-tests) groups+=(headless) ;;
    --all) groups+=(luma headless) ;;
    --dry-run) dry_run=1 ;;
    -y | --yes) assume_yes=1 ;;
    -h | --help) usage; exit 0 ;;
    *) usage >&2; die "unknown option: $1" ;;
  esac
  shift
done

# ---------------------------------------------------------------- checks

if ! detect_distro; then
  if [[ $(uname -s) != Linux && -z ${OS_RELEASE:-} ]]; then
    die "this is $(uname -s), not Linux."
  elif [[ ! -r $linux_os_release ]]; then
    die "can't read $linux_os_release, so the distribution is unknown."
  fi
  die "unsupported distribution: $distro_name. Supported: Fedora/RHEL, Debian/Ubuntu, openSUSE, Arch and their derivatives. Install the equivalents of the fedora or debian column of scripts/linux_packages.txt by hand."
fi
note "found $distro_name ($distro_family family, $pkg_manager)"

# Oldest releases the package names were checked against. Older releases
# usually work but may name a package differently, so this only warns.
case "$distro_id:$distro_version" in
  fedora:[0-9]*) ((${distro_version%%.*} >= 41)) || note "warning: Fedora $distro_version is older than 41; some package names may differ." ;;
  ubuntu:[0-9]*) ((${distro_version%%.*} >= 22)) || note "warning: Ubuntu $distro_version is older than 22.04; some package names may differ." ;;
  debian:[0-9]*) ((${distro_version%%.*} >= 12)) || note "warning: Debian $distro_version is older than 12; some package names may differ." ;;
esac

if is_image_based; then
  die "this is an image-based (rpm-ostree) system. Run the script inside a toolbox or distrobox container: toolbox create && toolbox enter."
fi
command -v "$pkg_manager" >/dev/null 2>&1 ||
  die "$pkg_manager not found, though $distro_name normally has it. Is this a minimal container?"

if ! find_root_command; then
  ((dry_run)) || die "not running as root and neither sudo nor doas is installed. Run as root, or install sudo and add your user to the admin group (wheel or sudo)."
  as_root=(sudo)
fi
if ((!dry_run)) && [[ ${as_root[*]-} == sudo ]] && ! sudo -n true 2>/dev/null; then
  note "the package manager needs root; sudo will ask for your password."
  sudo -v || die "sudo failed. Your user must be in the wheel (Fedora, openSUSE, Arch) or sudo (Debian, Ubuntu) group, or run as root."
fi

# ---------------------------------------------------------------- packages

packages=()
while IFS=$'\t' read -r _check pkgs _purpose; do
  [[ $pkgs == - ]] && continue
  for pkg in $pkgs; do
    [[ " ${packages[*]-} " == *" $pkg "* ]] || packages+=("$pkg")
  done
done < <(package_rows "${groups[@]}")
((${#packages[@]})) || die "no packages listed for $distro_family in $linux_packages_file."

# apt only knows what's in its cache, so refresh it before checking names.
if [[ $pkg_manager == apt-get ]]; then
  if ((dry_run)); then
    echo "+ ${as_root[*]-} apt-get update   (skipped by --dry-run; names are checked against the current cache)"
  else
    "${as_root[@]}" apt-get update || die "apt-get update failed. Check network access and /etc/apt/sources.list*."
  fi
fi

# Name every package the repositories don't have, instead of letting the
# package manager fail somewhere in a long transcript.
note "checking ${#packages[@]} package names against the enabled repositories..."
missing_status=0
missing=$(missing_packages "${packages[@]}") || missing_status=$?
((missing_status != 2)) || die "$pkg_manager couldn't query the repositories. Check network access and: dnf repolist"
if [[ -n $missing ]]; then
  printf 'linux_setup: error: not found in the enabled repositories: %s\n' "$(tr '\n' ' ' <<<"$missing")" >&2
  case "$distro_family:$distro_id" in
    fedora:fedora) printf '  Look up a renamed package: dnf provides "*/<file>"  or  dnf search <name>\n' >&2 ;;
    fedora:*)
      printf '  On RHEL and its clones some -devel packages are in CRB (PowerTools on 8) and\n' >&2
      printf '  weston/openbox in EPEL: sudo dnf config-manager --set-enabled crb && sudo dnf install epel-release\n' >&2 ;;
    debian:*) printf '  Ubuntu needs the universe component (sudo add-apt-repository universe); then: apt-cache search <name>\n' >&2 ;;
    suse:*) printf '  Search: zypper search <name>; check repositories: zypper repos\n' >&2 ;;
    arch:*) printf '  Refresh the package databases (sudo pacman -Sy is part of -Syu), or: pacman -Ss <name>\n' >&2 ;;
  esac
  exit 1
fi

to_install=()
for pkg in "${packages[@]}"; do
  package_installed "$pkg" || to_install+=("$pkg")
done
note "$((${#packages[@]} - ${#to_install[@]})) already installed, ${#to_install[@]} to install"

finish() {
  case "$distro_family" in
    suse) note "Vulkan driver: if vulkaninfo finds no GPU, install libvulkan_radeon (AMD), libvulkan_intel (Intel) or the NVIDIA driver." ;;
    arch) note "Vulkan driver: install vulkan-radeon (AMD), vulkan-intel (Intel) or nvidia-utils (NVIDIA) if vulkaninfo finds no GPU." ;;
  esac
  if ((dry_run)); then
    note "dry run: nothing installed."
    exit 0
  fi
  cat <<'EOF'

linux_setup: done. Next:
  scripts/linux_rust.sh      install Rust with rustup (as your user, not root)
  scripts/linux_doctor.sh    check the whole setup
EOF
  exit 0
}

if ((${#to_install[@]} == 0)); then
  note "nothing to install."
  finish
fi

yes=()
case "$pkg_manager" in
  dnf | apt-get) ((assume_yes)) && yes=(-y) ;;
  zypper) ((assume_yes)) && yes=(--non-interactive) ;;
  pacman) ((assume_yes)) && yes=(--noconfirm) ;;
esac
case "$pkg_manager" in
  dnf) cmd=(dnf install "${yes[@]}" "${to_install[@]}") ;;
  apt-get) cmd=(apt-get install "${yes[@]}" "${to_install[@]}") ;;
  zypper) cmd=(zypper "${yes[@]}" install "${to_install[@]}") ;;
  # Arch doesn't support partial upgrades, so this syncs and upgrades too.
  pacman) cmd=(pacman -Syu --needed "${yes[@]}" "${to_install[@]}") ;;
esac
cmd=("${as_root[@]}" "${cmd[@]}")
echo "+ ${cmd[*]}"
((dry_run)) && finish

status=0
"${cmd[@]}" || status=$?
((status == 0)) || die "$pkg_manager failed (exit $status). Scroll up for its message; docs/LINUX-SETUP.md lists the common ones."

# The package manager can exit 0 when the user declines; check what landed.
installed=() not_installed=()
for pkg in "${to_install[@]}"; do
  if package_installed "$pkg"; then installed+=("$pkg"); else not_installed+=("$pkg"); fi
done
((${#not_installed[@]} == 0)) || die "still not installed: ${not_installed[*]}"

# Record the undo command: the exact packages this run added.
case "$pkg_manager" in
  dnf)
    txn=$(dnf history list 2>/dev/null | awk '$1 ~ /^[0-9]+$/ {print $1; exit}' || true)
    if [[ -n $txn ]]; then undo="sudo dnf history undo $txn"; else undo="sudo dnf remove ${installed[*]}"; fi ;;
  apt-get) undo="sudo apt-get remove ${installed[*]} && sudo apt-get autoremove" ;;
  zypper) undo="sudo zypper remove --clean-deps ${installed[*]}" ;;
  pacman) undo="sudo pacman -Rs ${installed[*]}" ;;
esac
state_dir="${XDG_STATE_HOME:-$HOME/.local/state}/xgpui"
state_file="$state_dir/linux-setup-$(date +%Y%m%d-%H%M%S).txt"
if mkdir -p "$state_dir" 2>/dev/null &&
  printf '# linux_setup.sh on %s\n# installed: %s\n%s\n' "$distro_name" "${installed[*]}" "$undo" >"$state_file"; then
  note "undo with: $undo   (saved in $state_file)"
else
  note "undo with: $undo"
fi
finish
