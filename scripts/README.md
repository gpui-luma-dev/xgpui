# scripts

## Linux setup

These scripts set up a Linux machine to build and run xgpui. They work on
Fedora/RHEL, Debian/Ubuntu, openSUSE and Arch. Run them from the repository
root, as your normal user, in this order:

| Step | Script | What it does |
| --- | --- | --- |
| 1 | `linux_setup.sh` | Installs the system packages with the distribution's package manager (uses sudo). `--dry-run` shows the plan without installing. Add `--luma` for xgpui-luma, `--headless-tests` for the headless window probes. |
| 2 | `linux_rust.sh` | Installs stable Rust with rustfmt and clippy through rustup. |
| 3 | `linux_doctor.sh` | Checks compilers, libraries, Rust, the display session, Vulkan and the desktop portal. Changes nothing; run it first when a build or window fails. |

`linux_packages.txt` is the package list steps 1 and 3 read, with each
distribution's package names. `linux_common.sh` holds helpers the three scripts
share, and `linux_setup_selftest.sh` tests them with fake package managers
(part of `just test`). With `just`, the scripts also run as `just linux-setup`,
`just linux-rust` and `just linux-doctor`.

Prerequisites (git, Chrome, optional rustup, optional xgpui-luma clone), what
each script changes, how to undo it and how to diagnose failures are in the
guide: **[docs/LINUX-SETUP.md](../docs/LINUX-SETUP.md)**.
