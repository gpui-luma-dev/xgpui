# Linux setup

How to set up a Linux machine to build and run xgpui (and xgpui-luma), using
the scripts in [`scripts/`](../scripts). They read `/etc/os-release` and use
the distribution's own package manager, so the same three steps work on every
supported family:

| Family | Detected from `ID` / `ID_LIKE` | Package manager | Checked against |
| --- | --- | --- | --- |
| Fedora, RHEL, CentOS Stream, Rocky, Alma | `fedora`, `rhel`, `centos` | `dnf` | Fedora 41+ |
| Debian, Ubuntu, Mint, Pop!_OS | `debian`, `ubuntu` | `apt-get` | Debian 12+, Ubuntu 22.04+ |
| openSUSE Leap, Tumbleweed | `*suse*` | `zypper` | — |
| Arch, Manjaro, EndeavourOS | `arch` | `pacman` | — |

Other distributions (Void, Gentoo, NixOS, Alpine) aren't automated: the setup
script stops and names the package table to copy from. The Ubuntu names are
exactly the ones CI installs; the others are each distribution's equivalents
and haven't all been tried on a real machine yet (see [Status](#status)).

The scripts take their distribution detection, the `sudo`/`doas` fallback and
the RHEL repository notes from Zed's `script/linux`. Zed's own extras
(libgit2, sqlite, libva, musl) aren't needed by xgpui and are left out.

## Contents

- [Before you start](#before-you-start)
- [Quick start](#quick-start)
- [Prerequisites](#prerequisites)
- [The scripts](#the-scripts)
- [Diagnosing problems](#diagnosing-problems)
- [Keeping the lists current](#keeping-the-lists-current)
- [Status](#status)

## Before you start

Install these by hand. The scripts live in the repository, so they can't
install the tools you need before you have it.

| What | Needed for | How |
| --- | --- | --- |
| **git** | Cloning the repository (required) | Fedora/RHEL `sudo dnf install git`, Debian/Ubuntu `sudo apt-get install git`, openSUSE `sudo zypper install git`, Arch `sudo pacman -S git` |
| **Google Chrome** | The browser demos (`just pages-serve`, the WebGPU/WebGL2 shader and extension galleries) and checking the website | Download the `.rpm` or `.deb` from <https://www.google.com/chrome/> and install the downloaded file with your package manager (below). |
| **rustup** (optional) | Rust | Skip it: step 2 installs rustup. If you already have rustup, step 2 only updates stable and adds rustfmt and clippy. |

Installing the downloaded Chrome package also adds Google's repository, so
Chrome updates with the rest of the system:

```sh
sudo dnf install ./google-chrome-stable_current_x86_64.rpm     # Fedora, RHEL
sudo apt-get install ./google-chrome-stable_current_amd64.deb  # Debian, Ubuntu
sudo zypper install ./google-chrome-stable_current_x86_64.rpm  # openSUSE
```

On Fedora you can instead enable the bundled repository definition:
`sudo dnf install fedora-workstation-repositories && sudo dnf config-manager setopt google-chrome.enabled=1 && sudo dnf install google-chrome-stable`.
On Arch, Chrome is in the AUR (`google-chrome`). Chrome for Linux is x86_64
only, so on ARM machines use Chromium from the distribution's repositories.
Chrome isn't needed to build or run native xgpui apps.

Then clone and enter the repository:

```sh
git clone https://github.com/gpui-luma-dev/xgpui.git
cd xgpui
```

**Optional: xgpui-luma.** The lookless controls SDK is a separate repository,
so the clone above doesn't include it. It builds on its own against the
published xgpui 5.x crates, and doesn't need an xgpui checkout beside it. If
you want it, clone it next to xgpui:

```sh
git clone https://github.com/gpui-luma-dev/xgpui-luma.git ../xgpui-luma
```

Then pass `--luma` to steps 1 and 3 below. Its apps need GTK 3, which only
that flag installs. Its `rust-toolchain.toml` asks for stable with rustfmt and
clippy, which is what step 2 installs.

## Quick start

From the repository root, as your normal user (not root):

```sh
scripts/linux_setup.sh --dry-run   # see what would be installed
scripts/linux_setup.sh             # 1. system packages (asks for sudo)
scripts/linux_rust.sh              # 2. Rust stable via rustup
scripts/linux_doctor.sh            # 3. check everything
```

Add `--luma` to steps 1 and 3 when you also cloned and build xgpui-luma, and
`--headless-tests` to run the headless window probes in `.github/linux/`
(`--all` does both). With `just` installed, `just linux-setup`,
`just linux-rust` and `just linux-doctor` run the same scripts and pass
options through.

Then build: `cargo build --locked` or `just build`.

## Prerequisites

### Distribution

A supported family from the table above, installed normally, with network
access to its repositories. The scripts warn, but carry on, on releases older
than the ones checked (some package names may differ).

**Image-based systems** (Fedora Silverblue/Kinoite, Bazzite, other rpm-ostree
systems) can't install packages into the host this way. The setup script
detects `/run/ostree-booted` and stops. Run all three steps inside a
[toolbox](https://containertoolbx.org/) or distrobox container instead:

```sh
toolbox create && toolbox enter
```

### Root access

Step 1 installs packages, so it needs root. Run it as your normal user. It uses
`sudo`, or `doas` when `sudo` isn't installed, and checks that access first:

- Fedora, openSUSE and Arch: your user must be in the `wheel` group.
- Debian and Ubuntu: your user must be in the `sudo` group.

Check with `id -nG`. Step 2 refuses to run as root, because rustup installs
into the current user's home directory. It uses `sudo` only to install the
distribution's `rustup` package.

### Repositories to enable first

| Distribution | What to enable | Why |
| --- | --- | --- |
| Fedora | nothing | Everything is in the default repositories. |
| RHEL 9, Rocky 9, Alma 9, CentOS Stream 9 | `sudo dnf config-manager --set-enabled crb` | `libxkbcommon-x11-devel` and other `-devel` packages are in CRB. |
| RHEL 8 clones | `sudo dnf config-manager --set-enabled powertools` | Same, for release 8. |
| RHEL family, `--headless-tests` | `sudo dnf install epel-release` | Weston and Openbox are in EPEL. |
| Ubuntu | `sudo add-apt-repository universe` | Weston, Openbox and some `-dev` packages are in universe (enabled by default on desktop installs). |
| Debian, openSUSE, Arch | nothing | |

Zed's script enables CRB/PowerTools automatically. These scripts don't change
your repository setup: they stop, name the missing package and print the
command above.

### Packages

[`scripts/linux_packages.txt`](../scripts/linux_packages.txt) is the single
list both the setup script and the doctor read. It has one row per thing to
check, with that row's package name for each family. The groups:

- **base** (always):
  - Compilers and tools: gcc/g++, make, clang, cmake, pkg-config and git.
    lld is installed too, as an optional faster linker.
  - Libraries the crates build against: fontconfig, freetype, Wayland,
    xkbcommon (plus its X11 part), X11/XCB with RandR, XInput, Xcursor and
    Xinerama, ALSA, libudev, Vulkan headers, PipeWire, GBM, EGL and D-Bus.
  - Runtime pieces: the Vulkan loader, Mesa's Vulkan drivers (Fedora and
    Debian), `vulkaninfo` and `xdg-desktop-portal`. xgpui uses the portal
    (through `ashpd`) for file dialogs and desktop settings.
- **luma** (`--luma`): GTK 3 development files. xgpui-luma's apps use the
  `gtk` crate, and this one package pulls in the glib, cairo, pango and
  gdk-pixbuf headers.
- **headless** (`--headless-tests`): Xvfb, Weston, Openbox, Mutter,
  `xdpyinfo`, `xprop` and `dbus-x11`, for `.github/linux/window-smoke.sh`
  and `mutter-smoke.sh`.

Two packages from the original Fedora notes in #389 aren't in the list:

- **Google Chrome** is a manual prerequisite (see
  [Before you start](#before-you-start)). It comes from Google's repository,
  not the distribution's.
- **`openssl-devel`** isn't needed, because no crate in the workspace links
  OpenSSL (TLS in the tooling uses rustls/aws-lc). Add it by hand if you need
  it.

### Rust toolchain

CI builds with **stable** Rust from rustup, with `rustfmt` and `clippy`. There
is no `rust-toolchain.toml`. Step 2 installs exactly that. Distribution
`rust`/`cargo` packages usually lag behind stable and can shadow rustup's
`cargo` on `PATH`. The scripts warn about them rather than removing them.

### GPU and Vulkan drivers

xgpui renders through wgpu on **Vulkan**. You need a GPU with a Vulkan driver
and the Vulkan loader (`libvulkan.so.1`):

| GPU | Driver |
| --- | --- |
| AMD, Intel | Mesa (RADV, ANV). Installed by step 1 on Fedora and Debian/Ubuntu (`mesa-vulkan-drivers`). openSUSE: `libvulkan_radeon` / `libvulkan_intel`. Arch: `vulkan-radeon` / `vulkan-intel`. |
| NVIDIA, proprietary | Fedora: enable [RPM Fusion](https://rpmfusion.org/Howto/NVIDIA) and install `akmod-nvidia`, then reboot. Ubuntu: `sudo ubuntu-drivers install`. Arch: `nvidia` + `nvidia-utils`. openSUSE: NVIDIA's repository (`openSUSE-repos-*-NVIDIA`). |
| NVIDIA, open source | Mesa's NVK (`nouveau` kernel driver), included in `mesa-vulkan-drivers` on recent releases. Works, but less mature. |
| No GPU / VM | Mesa's llvmpipe (lavapipe), a CPU renderer. It works for tests but is slow, and the doctor warns about it. |

When `vulkaninfo --summary` lists your GPU (not only llvmpipe), the driver is
working.

### Wayland and X11

xgpui has both a Wayland and an X11 backend and picks one at startup:

- **Wayland session** (the default on current Fedora, Ubuntu, Debian GNOME and
  KDE Plasma): xgpui uses Wayland when `WAYLAND_DISPLAY` is set. GNOME on
  recent Fedora and Ubuntu is Wayland-only, and X11 apps run through XWayland.
- **X11 session**: xgpui uses X11 through `DISPLAY`.
- To try the X11 backend from a Wayland session, start the app with
  `WAYLAND_DISPLAY` unset, so it connects to XWayland:
  `env -u WAYLAND_DISPLAY cargo run ...`.
- Over SSH or on a text console, neither is set. Building and the headless
  tests work, but windows can't open. For real-window tests without a desktop,
  use `--headless-tests` and the scripts in `.github/linux/`.

The default features build both backends, so the headers for both are
required whichever session you use.

## The scripts

All three print what they're doing, stop at the first problem with a message
that names the fix, and accept `-h` for help.

### 1. `scripts/linux_setup.sh` — system packages

```text
scripts/linux_setup.sh [--luma] [--headless-tests] [--all] [--dry-run] [-y]
```

What it does, in order:

1. Detects the family from `/etc/os-release`. It stops on macOS, on an
   unsupported distribution, on an image-based system, or when the package
   manager is missing.
2. Finds `sudo` or `doas`, and checks it works. sudo asks for your password
   once, up front.
3. Builds the package list for the selected groups from
   `scripts/linux_packages.txt`.
4. On apt, runs `apt-get update` so the package names can be checked.
5. Checks **every** name against the enabled repositories. If any are
   missing, it lists them all and prints the distribution's hint (CRB/EPEL,
   universe, `dnf provides`), then stops without installing anything.
6. Skips packages that are already installed, then runs one install
   transaction:
   - `dnf install`, `apt-get install` or `zypper install`.
   - On Arch, `pacman -Syu --needed`. Arch doesn't support partial upgrades,
     so this also **upgrades the whole system**.
7. Checks that every package actually landed, then prints and saves the undo
   command.

What it changes: only packages, through the package manager. On apt, it also
refreshes the package index. It doesn't touch repositories, drivers, shell
files or anything in the repository. It writes one small file,
`~/.local/state/xgpui/linux-setup-<date>.txt` (`$XDG_STATE_HOME` if set),
which holds the undo command.

**Idempotent:** yes. A second run reports `N already installed, 0 to install`
and changes nothing (on Arch, `-Syu` still offers system upgrades).

**Undo:** run the command it printed, also saved in that state file:

| Family | Undo |
| --- | --- |
| dnf | `sudo dnf history undo <id>` (the transaction id it printed; `dnf history list` shows it too) |
| apt | `sudo apt-get remove <the packages it installed> && sudo apt-get autoremove` |
| zypper | `sudo zypper remove --clean-deps <packages>` |
| pacman | `sudo pacman -Rs <packages>` |

Only the packages this run installed are listed, so undo never removes
something you already had.

`--dry-run` does steps 1–6 without root and without changing anything, and
prints the install command. On apt it checks names against the current cache,
because refreshing the cache needs root. `OS_RELEASE=<file>` points the
scripts at another os-release file. This is only useful with `--dry-run`, to
see another distribution's list.

### 2. `scripts/linux_rust.sh` — Rust

```text
scripts/linux_rust.sh [--dry-run]
```

Run it as your user. It refuses to run as root.

- **rustup already installed:** runs
  `rustup toolchain install stable --profile minimal --component rustfmt --component clippy`
  and `rustup default stable`.
- **Otherwise:** installs the distribution's `rustup` package if the
  repositories have one (Fedora, openSUSE, Arch, Debian 13+, Ubuntu 24.04+).
  It uses sudo for that, and runs `rustup-init` where the package ships that.
  Without a package, it runs the official installer from
  <https://sh.rustup.rs> (HTTPS only, as Zed's script does). Then it installs
  stable as above.

What it changes:
- `~/.rustup` and `~/.cargo`.
- The `rustup` package, if one was installed.
- **Not** your shell profile. If `~/.cargo/bin` isn't on `PATH`, it prints
  the line to add (`. "$HOME/.cargo/env"` in `~/.bashrc` or `~/.zshrc`).

**Idempotent:** yes. Re-running updates stable to the latest release.
**Undo:** `rustup self uninstall`, plus removing the distribution's `rustup`
package if the script installed one.

### 3. `scripts/linux_doctor.sh` — checks

```text
scripts/linux_doctor.sh [--luma] [--headless-tests] [--all]
```

Read-only: it never installs or changes anything, and needs no root. For each
row of the package table it checks the thing itself (a `pkg-config` module, a
command, a library or a Vulkan driver file), not whether a package is
installed. That way it also catches libraries installed some other way, and a
`PKG_CONFIG_PATH` that hides them. It then checks:

- **Rust:** rustup, the active toolchain, rustfmt and clippy.
- **Display session:** `XDG_SESSION_TYPE`, `WAYLAND_DISPLAY`, `DISPLAY`,
  `XDG_RUNTIME_DIR`, and that the Wayland socket exists.
- **GPU and Vulkan:** GPUs from `lspci`, the devices `vulkaninfo --summary`
  reports (warning on llvmpipe), an NVIDIA GPU without NVIDIA's driver, and
  `VK_DRIVER_FILES` overrides.
- **Desktop portal:** whether it answers on the session bus, and which
  backends are installed.

It ends with a summary and a single install command for everything missing,
using this distribution's package names. It exits 1 when any check fails and
0 when only warnings remain, so it can gate a script.

Run it first whenever a Linux build or window fails, and paste its output into
the issue.

## Diagnosing problems

### Checks to run by hand

| Question | Command | Expected |
| --- | --- | --- |
| Which distribution? | `cat /etc/os-release` | `ID` / `ID_LIKE` in the table above |
| Wayland or X11? | `echo $XDG_SESSION_TYPE $WAYLAND_DISPLAY $DISPLAY` | `wayland wayland-0 :0` or `x11  :0` |
| Is a library's `.pc` file visible? | `pkg-config --modversion xkbcommon-x11` | a version, not "not found" |
| Which package provides a missing file? | Fedora `dnf provides '*/xkbcommon-x11.pc'`, Debian `apt-file search xkbcommon-x11.pc`, openSUSE `zypper what-provides 'pkgconfig(xkbcommon-x11)'`, Arch `pacman -F xkbcommon-x11.pc` | a package name |
| Vulkan loader present? | `ldconfig -p \| grep libvulkan.so.1` | one or more paths |
| Vulkan drivers installed? | `ls /usr/share/vulkan/icd.d/` | `radeon_icd.*.json`, `intel_icd.*.json`, `nvidia_icd.json`, … |
| Does Vulkan see the GPU? | `vulkaninfo --summary` | your GPU's name under `deviceName` |
| Which driver is the GPU using? | `lspci -k \| grep -A3 -Ei 'vga\|3d'` | `Kernel driver in use: amdgpu / i915 / xe / nvidia / nouveau` |
| Which Rust? | `which cargo; rustup show` | `~/.cargo/bin/cargo`, `stable-…` active |
| Portal running? | `busctl --user status org.freedesktop.portal.Desktop` | process details, not an error |

### Setup script failures

| Message | Cause | Fix |
| --- | --- | --- |
| `this is Darwin, not Linux.` | Run on macOS | Run it on the Linux machine. |
| `unsupported distribution: …` | Family not in the table | Install the packages from the closest column of `scripts/linux_packages.txt` by hand, then run the doctor. |
| `this is an image-based (rpm-ostree) system` | Silverblue/Kinoite | `toolbox create && toolbox enter`, then re-run. |
| `neither sudo nor doas is installed` | Minimal install or container | Run as root, or install sudo. |
| `sudo failed. Your user must be in the wheel … group` | User not an administrator | `sudo usermod -aG wheel $USER` (Debian/Ubuntu: `sudo`) as an admin, then log in again. |
| `not found in the enabled repositories: libxkbcommon-x11-devel` (RHEL clone) | CRB/PowerTools disabled | `sudo dnf config-manager --set-enabled crb` (8: `powertools`), re-run. |
| `not found …: weston openbox` (RHEL clone) | EPEL missing | `sudo dnf install epel-release`, re-run. |
| `not found …: weston mutter …` (Ubuntu) | universe disabled | `sudo add-apt-repository universe`, re-run. |
| `not found …: <name>` (anything else) | Package renamed on this release | Find the new name with the "which package provides" command above, update `scripts/linux_packages.txt`, open a PR. |
| `couldn't query the repositories` | No network, broken repository | `dnf repolist` / `zypper repos`; check the network or proxy. |
| `apt-get update failed` | Broken source or no network | Read the apt error, fix `/etc/apt/sources.list*`. |
| `still not installed: …` | Transaction declined or partially failed | Re-run without `-y` and read the package manager's output. |
| pacman: `failed to commit transaction (conflicting files)` | Leftover files from a manual install | Read which file and package conflict; remove the stale file. |

### Rust script failures

| Message | Fix |
| --- | --- |
| `don't run this as root` | Run as your user, without `sudo`. |
| `… is a distribution Rust, not rustup's` (warning) | Put `~/.cargo/bin` before `/usr/bin` in `PATH`, or remove the distribution's `rust`/`cargo` packages. |
| `curl isn't installed` | Install `curl` with the package manager. |
| `the rustup installer failed` | Check access to `static.rust-lang.org` (proxy, firewall). |
| `rustc not found after installing` | `rustup component add rustfmt clippy`; check `~/.cargo/bin`. |
| `cargo` not found in a new terminal | Add `. "$HOME/.cargo/env"` to `~/.bashrc` / `~/.zshrc`. |

### Build failures

| Error during `cargo build` | Cause | Fix |
| --- | --- | --- |
| `The system library \`xkbcommon-x11\` required by crate … was not found` or `Package xkbcommon-x11 was not found in the pkg-config search path` | Missing `-devel`/`-dev` package | `scripts/linux_doctor.sh` names the package; or run step 1 again. Any `.pc` name maps to a row in `scripts/linux_packages.txt`. |
| `failed to run custom build command for \`yeslogic-fontconfig-sys\`` / `fontconfig` not found | fontconfig headers missing | Fedora `fontconfig-devel`, Debian `libfontconfig1-dev`, openSUSE `fontconfig-devel`, Arch `fontconfig`. |
| `Could not find directory of OpenSSL installation` | A crate outside this workspace wants OpenSSL | Fedora/openSUSE `openssl-devel`, Debian `libssl-dev`, Arch `openssl`. |
| `is \`cmake\` not installed?` | cmake missing | Install `cmake`. |
| `Unable to find libclang` | clang missing (bindgen) | Install `clang` (Debian also `libclang-dev`). |
| `linker \`cc\` not found` | No C compiler | Fedora `gcc`, Debian `build-essential`, Arch `base-devel`. |
| `fatal error: 'cstddef' file not found` / `cannot find -lstdc++` (clang) | clang picked a newer GCC whose libstdc++ headers aren't installed (Ubuntu/Debian) | Install the matching `libstdc++-<N>-dev` (Ubuntu 24.04: `libstdc++-14-dev`, 22.04: `libstdc++-12-dev`), as Zed's script does. |
| `rustc … is not supported by the following package` / `requires rustc 1.xx` | Old toolchain | `rustup update stable`, or run step 2. |

### Runtime failures

| Symptom or message | Cause | Fix |
| --- | --- | --- |
| `ERROR_INCOMPATIBLE_DRIVER` / `vkCreateInstance failed` / wgpu: no suitable adapter | No working Vulkan driver | Install the driver for your GPU ([GPU and Vulkan drivers](#gpu-and-vulkan-drivers)); `vulkaninfo --summary` must list the GPU. |
| `vulkaninfo` lists only `llvmpipe` | GPU driver not loading | NVIDIA: install the proprietary driver (or NVK) and reboot. AMD/Intel: install Mesa Vulkan drivers. Also check `VK_DRIVER_FILES` / `VK_ICD_FILENAMES` aren't set. |
| `libvulkan.so.1: cannot open shared object file` | Loader missing | Fedora `vulkan-loader`, Debian/openSUSE `libvulkan1`, Arch `vulkan-icd-loader`. |
| Windows don't open; `Failed to connect to Wayland` / `couldn't open display` | No display (SSH, console), or `XDG_RUNTIME_DIR` unset | Run from a desktop terminal, or use the headless setup in `.github/linux/README.md`. |
| App works on X11 but not Wayland (or the reverse) | Backend-specific bug | Retry with `env -u WAYLAND_DISPLAY` to force X11. Report both doctor outputs on the issue. |
| File dialogs don't open; `org.freedesktop.portal.Desktop was not provided` | Portal or its backend missing | Install `xdg-desktop-portal` plus your desktop's backend (`xdg-desktop-portal-gnome`, `-gtk` or `-kde`), then `systemctl --user restart xdg-desktop-portal`. |
| Wrong scale, theme or accent color | Settings portal not answering | Same as above. The doctor's "Desktop portal" section shows the state. |
| No sound / ALSA errors in the logs | Normal on headless machines | Ignore unless an audio feature is being tested. |

## Keeping the lists current

- `scripts/linux_packages.txt` is the only package list. Edit a row and both
  the setup script and the doctor pick it up.
- The **debian** column mirrors the `apt-get install` lines in
  `.github/workflows/*.yml`. When CI adds a library, add a row with the
  `pkg-config` module (or command) and every family's package name.
- To check a change without a Linux machine, run each family with
  `OS_RELEASE=<file> scripts/linux_setup.sh --all --dry-run`, using a small
  os-release file with `ID=…` (and `ID_LIKE=…`). This shows the selected
  list. Whether the names exist can only be checked on that distribution.
- Run `shellcheck -x -P SCRIPTDIR scripts/linux_*.sh` before committing.

## Status

Tracked in [#389](https://github.com/gpui-luma-dev/xgpui/issues/389).

- Verified: `shellcheck` is clean. Dry runs and stubbed installs were run on
  macOS against os-release files for Fedora 43, Rocky 9, Debian 13, Ubuntu
  24.04/20.04, openSUSE Tumbleweed, Arch and Void. They covered the
  missing-package, idempotent re-run, undo-record, root-check and
  unsupported-distribution paths.
- Not yet verified on real systems:
  - Running the scripts on Fedora or any other distribution.
  - The package names for openSUSE and Arch in general.
  - Fedora's `xdpyinfo`, `xprop`, `mesa-libgbm-devel`, `libglvnd-devel`,
    `pipewire-devel` and `vulkan-loader-devel`.
  - `dnf history list` output under dnf5 (the transaction id for undo).

  The CI Ubuntu list is the only one exercised continuously.
