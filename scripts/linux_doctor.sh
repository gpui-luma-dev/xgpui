#!/usr/bin/env bash
# Step 3 of the Linux setup (#389), and the first thing to run when a Linux
# build or window fails: checks compilers, pkg-config libraries, Rust, the
# display session, Vulkan and the desktop portal. Changes nothing.
# See docs/LINUX-SETUP.md for what each failure means.
#
# Usage: scripts/linux_doctor.sh [options]
#   --luma            also check GTK 3 (xgpui-luma)
#   --headless-tests  also check the tools the headless window probes use
#   --all             everything above
#   -h, --help        show this help
#
# Exits 1 if any check fails, 0 when only warnings remain. Install hints use
# this distribution's package names from scripts/linux_packages.txt.
set -uo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=linux_common.sh
. "$here/linux_common.sh"

usage() { sed -n '2,/^set -uo/{/^set -uo/d;s/^# \{0,1\}//;p;}' "$0"; }

groups=(base)
setup_flags=''
while (($#)); do
  case "$1" in
    --luma) groups+=(luma); setup_flags+=' --luma' ;;
    --headless-tests) groups+=(headless); setup_flags+=' --headless-tests' ;;
    --all) groups+=(luma headless); setup_flags=' --all' ;;
    -h | --help) usage; exit 0 ;;
    *) usage >&2; printf 'linux_doctor: unknown option: %s\n' "$1" >&2; exit 2 ;;
  esac
  shift
done

failures=0
warnings=0
fix_packages=()

section() { printf '\n== %s\n' "$*"; }
ok() { printf '  ok    %s\n' "$*"; }
warn() { printf '  warn  %s\n' "$*"; warnings=$((warnings + 1)); }
fail() { printf '  FAIL  %s\n' "$*"; failures=$((failures + 1)); }
have() { command -v "$1" >/dev/null 2>&1; }
# fail_pkg <message> <packages|->: fail and remember the packages for the summary.
fail_pkg() {
  if [[ $2 == - ]]; then fail "$1"; return; fi
  fail "$1 (install: $2)"
  local p
  for p in $2; do fix_packages+=("$p"); done
}

# ---------------------------------------------------------------- system
if [[ $(uname -s) != Linux && -z ${OS_RELEASE:-} ]]; then
  printf 'linux_doctor: this is %s, not Linux; nothing to check.\n' "$(uname -s)" >&2
  exit 1
fi
section "System"
if detect_distro; then
  ok "$distro_name ($distro_family family, $pkg_manager)"
else
  [[ -n $distro_name ]] || distro_name="unreadable $linux_os_release"
  warn "unrecognized distribution ($distro_name); install hints use Fedora's package names"
fi
is_image_based && warn "image-based (rpm-ostree) system: build inside a toolbox or distrobox"
ok "kernel $(uname -r), $(uname -m)"

# ---------------------------------------------------------------- packages
section "Build tools, libraries and runtime packages"
has_icd=0
icds=()
for dir in /usr/share/vulkan/icd.d /etc/vulkan/icd.d; do
  for f in "$dir"/*.json; do [[ -e $f ]] && icds+=("${f##*/}"); done
done
((${#icds[@]})) && has_icd=1

while IFS=$'\t' read -r check pkgs purpose; do
  soft=0
  if [[ $check == \?* ]]; then soft=1 check=${check#\?}; fi
  found=0 detail=''
  case "$check" in
    pc:*)
      if have pkg-config && detail=$(pkg-config --modversion "${check#pc:}" 2>/dev/null); then
        found=1 detail="${check#pc:} $detail"
      else
        detail="${check#pc:}.pc not found"
      fi
      ;;
    cmd:*)
      have "${check#cmd:}" && found=1
      detail=${check#cmd:}
      ((found)) || detail="$detail not on PATH"
      ;;
    lib:*)
      if have ldconfig && ldconfig -p 2>/dev/null | grep -qF "${check#lib:}"; then found=1; fi
      if ((!found)) && compgen -G "/usr/lib*/${check#lib:}" >/dev/null; then found=1; fi
      if ((!found)) && compgen -G "/usr/lib/*/${check#lib:}" >/dev/null; then found=1; fi
      detail=${check#lib:}
      ((found)) || detail="$detail not found"
      ;;
    icd)
      found=$has_icd
      if ((found)); then detail="Vulkan drivers: ${icds[*]}"; else detail="no Vulkan driver JSON in /usr/share/vulkan/icd.d"; fi
      ;;
    portal)
      for p in /usr/libexec/xdg-desktop-portal /usr/lib/xdg-desktop-portal; do
        if [[ -f $p && -x $p ]]; then found=1 detail="xdg-desktop-portal ($p)"; fi
      done
      ((found)) || detail="xdg-desktop-portal not installed"
      ;;
    *) continue ;;
  esac
  # "clang (clang, for cc builds)" reads as "clang, for cc builds".
  label="$detail ($purpose)"
  [[ $purpose == "$detail"* ]] && label=$purpose
  if ((found)); then
    ok "$label"
  elif ((soft)); then
    warn "$detail ($purpose; optional, install: $pkgs)"
  elif [[ $check == icd && $pkgs == - ]]; then
    case "$distro_family" in
      suse) fail "$detail: install libvulkan_radeon (AMD), libvulkan_intel (Intel) or the NVIDIA driver" ;;
      arch) fail "$detail: install vulkan-radeon (AMD), vulkan-intel (Intel) or nvidia-utils (NVIDIA)" ;;
      *) fail "$detail: install your GPU's Vulkan driver (Mesa for AMD/Intel)" ;;
    esac
  else
    fail_pkg "$detail ($purpose)" "$pkgs"
  fi
done < <(package_rows "${groups[@]}")
have pkg-config || fail "pkg-config is missing, so no library above could be checked"

# ---------------------------------------------------------------- rust
section "Rust"
cargo_home="${CARGO_HOME:-$HOME/.cargo}"
if have rustc && have cargo; then
  ok "$(rustc --version)"
  if have rustup; then
    toolchain=$(rustup show active-toolchain 2>/dev/null | awk '{print $1}')
    if [[ $toolchain == stable* ]]; then
      ok "rustup, active toolchain $toolchain"
    else
      warn "active toolchain is ${toolchain:-unknown}; CI uses stable (rustup default stable)"
    fi
    for comp in rustfmt clippy; do
      if rustup component list --installed 2>/dev/null | grep -q "^$comp"; then
        ok "$comp"
      else
        warn "$comp missing (rustup component add $comp)"
      fi
    done
  else
    warn "cargo is $(command -v cargo), not rustup's; CI uses rustup stable (scripts/linux_rust.sh)"
  fi
elif [[ -x $cargo_home/bin/cargo ]]; then
  fail "rustup is installed but $cargo_home/bin is not on PATH (run: . \"$cargo_home/env\")"
else
  fail "rustc/cargo not found (run scripts/linux_rust.sh)"
fi

# ---------------------------------------------------------------- session
section "Display session"
session=${XDG_SESSION_TYPE:-}
case "$session" in
  wayland) ok "Wayland session (WAYLAND_DISPLAY=${WAYLAND_DISPLAY:-unset}; DISPLAY=${DISPLAY:-unset} is XWayland)" ;;
  x11) ok "X11 session (DISPLAY=${DISPLAY:-unset})" ;;
  *)
    if [[ -n ${WAYLAND_DISPLAY:-} || -n ${DISPLAY:-} ]]; then
      ok "display found (WAYLAND_DISPLAY=${WAYLAND_DISPLAY:-unset}, DISPLAY=${DISPLAY:-unset}, XDG_SESSION_TYPE=${session:-unset})"
    else
      warn "no WAYLAND_DISPLAY or DISPLAY (SSH or text console?): builds and tests work, windows won't open"
    fi
    ;;
esac
if [[ -n ${WAYLAND_DISPLAY:-} && $WAYLAND_DISPLAY != /* && -n ${XDG_RUNTIME_DIR:-} && ! -S $XDG_RUNTIME_DIR/$WAYLAND_DISPLAY ]]; then
  warn "WAYLAND_DISPLAY=$WAYLAND_DISPLAY but $XDG_RUNTIME_DIR/$WAYLAND_DISPLAY is not a socket"
fi
[[ -n ${XDG_RUNTIME_DIR:-} ]] || warn "XDG_RUNTIME_DIR is unset (Wayland and the portal need it)"

# ---------------------------------------------------------------- vulkan
section "GPU and Vulkan"
has_nvidia=0
if have lspci; then
  while IFS= read -r line; do
    ok "GPU: ${line#*: }"
    [[ $line == *[Nn][Vv][Ii][Dd][Ii][Aa]* ]] && has_nvidia=1
  done < <(lspci 2>/dev/null | grep -Ei 'vga|3d controller|display controller')
else
  warn "lspci not found, so GPUs can't be listed (install pciutils)"
fi
if ((has_nvidia)) && ! printf '%s\n' "${icds[@]-}" | grep -qi nvidia; then
  warn "NVIDIA GPU without NVIDIA's Vulkan driver: Mesa's NVK or llvmpipe will be used. For the proprietary driver see docs/LINUX-SETUP.md"
fi
if have vulkaninfo; then
  if summary=$(vulkaninfo --summary 2>&1); then
    devices=$(awk -F'= ' '/deviceName/ {print $2}' <<<"$summary" | sort -u)
    if [[ -z $devices ]]; then
      fail "vulkaninfo found no devices"
    else
      while IFS= read -r dev; do
        if [[ $dev == *llvmpipe* || $dev == *lavapipe* ]]; then
          warn "Vulkan device: $dev (CPU software renderer; slow, and the GPU driver may not be loading)"
        else
          ok "Vulkan device: $dev"
        fi
      done <<<"$devices"
    fi
  else
    fail "vulkaninfo --summary failed: $(grep -m1 -E 'ERROR|error' <<<"$summary" || head -n1 <<<"$summary")"
  fi
fi
[[ -n ${WGPU_BACKEND:-} ]] && ok "WGPU_BACKEND=$WGPU_BACKEND"
if [[ -n ${VK_ICD_FILENAMES:-}${VK_DRIVER_FILES:-} ]]; then
  warn "VK_DRIVER_FILES/VK_ICD_FILENAMES is set (${VK_DRIVER_FILES:-${VK_ICD_FILENAMES:-}}); it overrides driver discovery"
fi

# ---------------------------------------------------------------- portal
section "Desktop portal"
if have busctl && [[ -n ${DBUS_SESSION_BUS_ADDRESS:-} ]]; then
  if busctl --user status org.freedesktop.portal.Desktop >/dev/null 2>&1; then
    ok "portal answers on the session bus"
  else
    warn "portal not answering on the session bus (in a desktop session: systemctl --user restart xdg-desktop-portal)"
  fi
else
  warn "no session bus here (SSH?), so the portal can't be asked; file dialogs need a desktop session"
fi
backends=()
for f in /usr/share/xdg-desktop-portal/portals/*.portal; do
  [[ -e $f ]] && backends+=("$(basename "$f" .portal)")
done
if ((${#backends[@]})); then
  ok "portal backends: ${backends[*]}"
else
  warn "no portal backend installed (xdg-desktop-portal-gnome, -gtk or -kde, matching your desktop)"
fi

# ---------------------------------------------------------------- summary
section "Summary"
printf '  %d failed, %d warnings\n' "$failures" "$warnings"
if ((${#fix_packages[@]})); then
  fix=$(printf '%s\n' "${fix_packages[@]}" | sort -u | tr '\n' ' ')
  case "$pkg_manager" in
    dnf) printf '  Install the missing packages:\n    sudo dnf install %s\n' "$fix" ;;
    apt-get) printf '  Install the missing packages:\n    sudo apt-get install %s\n' "$fix" ;;
    zypper) printf '  Install the missing packages:\n    sudo zypper install %s\n' "$fix" ;;
    pacman) printf '  Install the missing packages:\n    sudo pacman -S --needed %s\n' "$fix" ;;
    *) printf '  Missing (Fedora names): %s\n' "$fix" ;;
  esac
  [[ -n $pkg_manager ]] && printf '  or run: scripts/linux_setup.sh%s\n' "$setup_flags"
fi
((failures == 0))
