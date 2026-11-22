# Shared by scripts/linux_setup.sh, linux_rust.sh and linux_doctor.sh (#389).
# Source it; it defines functions and doesn't run anything on its own.
# shellcheck shell=bash
# The variables set here are used by the scripts that source this file.
# shellcheck disable=SC2034

# OS_RELEASE points at another os-release file, so the scripts can be tried
# (with --dry-run) for another distribution, or on a machine that isn't Linux.
linux_os_release="${OS_RELEASE:-/etc/os-release}"
# `.` looks a bare file name up on PATH, so anchor relative paths.
[[ $linux_os_release == /* ]] || linux_os_release="./$linux_os_release"
linux_packages_file="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/linux_packages.txt"

# Sets: distro_family (fedora|debian|suse|arch, or empty), pkg_manager,
# distro_name, distro_id, distro_version. Returns 1 when the family is unknown.
detect_distro() {
  distro_family='' pkg_manager='' distro_name='' distro_id='' distro_version=''
  if [[ $(uname -s) != Linux && -z ${OS_RELEASE:-} ]]; then
    distro_name=$(uname -s)
    return 1
  fi
  [[ -r $linux_os_release ]] || return 1
  local ID='' ID_LIKE='' VERSION_ID='' PRETTY_NAME=''
  # shellcheck disable=SC1090
  . "$linux_os_release"
  distro_id=$ID
  distro_version=$VERSION_ID
  distro_name=${PRETTY_NAME:-$ID $VERSION_ID}
  case " $ID $ID_LIKE " in
    *" fedora "* | *" rhel "* | *" centos "*) distro_family=fedora pkg_manager=dnf ;;
    *" debian "* | *" ubuntu "*) distro_family=debian pkg_manager=apt-get ;;
    *suse*) distro_family=suse pkg_manager=zypper ;;
    *" arch "*) distro_family=arch pkg_manager=pacman ;;
    *) return 1 ;;
  esac
}

# Prints one line per table row of the requested groups:
#   check<TAB>packages<TAB>purpose
# with packages for $distro_family ("-" when the distribution has none).
package_rows() {
  local col
  case "$distro_family" in
    fedora) col=3 ;; debian) col=4 ;; suse) col=5 ;; arch) col=6 ;;
    *) col=3 ;; # unknown distributions get Fedora's names as hints
  esac
  local groups=" $* "
  local line group
  while IFS= read -r line; do
    [[ -z $line || $line == \#* ]] && continue
    group=${line%%|*}
    [[ $groups == *" $group "* ]] || continue
    awk -F'|' -v c="$col" '{ printf "%s\t%s\t%s\n", $2, $c, $7 }' <<<"$line"
  done <"$linux_packages_file"
}

# Is package $1 installed? (per package manager)
package_installed() {
  case "$pkg_manager" in
    dnf | zypper) rpm -q --whatprovides "$1" >/dev/null 2>&1 ;;
    apt-get)
      [[ $(dpkg-query -W -f='${db:Status-Status}' "$1" 2>/dev/null) == installed ]] && return 0
      # A virtual name (Ubuntu 25.04+'s libfreetype6-dev) is satisfied by an
      # installed package that Provides it; apt installs that package instead.
      dpkg-query -W -f='${db:Status-Status}\t${Provides}\n' 2>/dev/null |
        awk -F'\t' -v n="$1" '$1 == "installed" {
          k = split($2, p, /, */)
          for (i = 1; i <= k; i++) { sub(/[ :].*/, "", p[i]); if (p[i] == n) f = 1 }
        } END { exit !f }'
      ;;
    # -T (deptest) also accepts names that an installed package provides.
    pacman) pacman -T "$1" >/dev/null 2>&1 || pacman -Qg "$1" >/dev/null 2>&1 ;;
    *) return 1 ;;
  esac
}

# Does package $1 exist in the enabled repositories (or is it installed)?
package_available() {
  package_installed "$1" && return 0
  case "$pkg_manager" in
    dnf) [[ -n $(dnf -q repoquery --available --latest-limit 1 --qf '%{name}\n' "$1" 2>/dev/null) ]] ;;
    apt-get) apt-cache show --no-all-versions "$1" >/dev/null 2>&1 ;;
    zypper) zypper --non-interactive --quiet search --match-exact --type package -- "$1" >/dev/null 2>&1 ;;
    pacman) pacman -Si "$1" >/dev/null 2>&1 || pacman -Sg "$1" >/dev/null 2>&1 ;;
    *) return 1 ;;
  esac
}

# Prints the packages among "$@" that are neither installed nor in the
# enabled repositories, one per line. dnf is asked once for all of them
# (a repoquery per package takes seconds); the others are fast per package.
missing_packages() {
  local pkg available=''
  if [[ $pkg_manager == dnf ]]; then
    available=$(dnf -q repoquery --available --latest-limit 1 --qf '%{name}\n' "$@" 2>/dev/null) || return 2
  fi
  for pkg in "$@"; do
    package_installed "$pkg" && continue
    if [[ $pkg_manager == dnf ]]; then
      grep -qxF "$pkg" <<<"$available" || printf '%s\n' "$pkg"
    else
      package_available "$pkg" || printf '%s\n' "$pkg"
    fi
  done
}

# Sets as_root to the command prefix that runs a command as root: empty for
# root, else sudo, else doas (Zed's script/linux does the same). Returns 1
# when neither is installed.
find_root_command() {
  as_root=()
  ((EUID == 0)) && return 0
  if command -v sudo >/dev/null 2>&1; then as_root=(sudo)
  elif command -v doas >/dev/null 2>&1; then as_root=(doas)
  else return 1
  fi
}

# Image-based systems (Silverblue, Kinoite, Bazzite, ...) can't dnf install.
is_image_based() { [[ -e /run/ostree-booted ]]; }
