# Local Development

Build xgpui and run its native examples on Windows, Linux or macOS. These
instructions assume Rust, the platform C/C++ toolchain, SDKs and native development
libraries are already installed. The commands below use `just` and Bash. On
Windows, put Git for Windows Bash on PATH (including when launching `just` from
PowerShell). Run commands from the repository root.

## Get the source

```sh
git clone https://github.com/gpui-luma-dev/xgpui.git
cd xgpui
```

Use a current stable Rust toolchain with Rust 2024 support. The first build needs
network access to download registry and pinned Git dependencies. Keep the committed
lockfiles. Recipes allow dependency downloads; they do not force offline mode.

## Local crates and published dependencies

XGPUI keeps nine framework and backend crates in `gpui/crates/`: `gpui`,
`gpui-macros`, `gpui-platform`, `gpui-apple`, `gpui-macos`, `gpui-linux`,
`gpui-windows`, `gpui-web` and `gpui-wgpu`. The seven XGPUI extension crates live
under `extensions/crates/`.

Downstream versions are independent of the captured GPUI release and vary by package. Local directory names and Rust import names stay stable; Cargo package
names use the XGPUI prefix:

| Local directory / import name | Release package |
| --- | --- |
| `gpui` | `xgpui` |
| `gpui-apple` | `xgpui-apple` |
| `gpui-linux` | `xgpui-linux` |
| `gpui-macos` | `xgpui-macos` |
| `gpui-macros` | `xgpui-macros` |
| `gpui-platform` | `xgpui-platform` |
| `gpui-web` | `xgpui-web` |
| `gpui-wgpu` | `xgpui-wgpu` |
| `gpui-windows` | `xgpui-windows` |

All seven extension packages keep their existing `xgpui-*` names. `scripts/release-packages.json` is the authoritative
name/version map. The `xgpui` facade is a subsequent packaging step; these package
identities prepare the release and do not mean it has been published.

The supporting crates below are dependencies from the published
[gpui-unofficial distribution](https://github.com/gpui-archipelago/gpui-unofficial).
They are no longer copied into this repository. Cargo downloads them from
crates.io; manifests keep familiar Rust import names using dependency aliases.
For example, the `collections` dependency uses the package
`collections-gpui-unofficial`, currently at `=1.25.3` to match the captured upstream release, rather than the
independent downstream package versions.

| Former local crate names | Published crates.io packages |
| --- | --- |
| `collections`, `gpui-util`, `gpui-shared-string` | `collections-gpui-unofficial`, `gpui-util-gpui-unofficial`, `gpui-shared-string-gpui-unofficial` |
| `scheduler`, `sum-tree` | `scheduler-gpui-unofficial`, `sum-tree-gpui-unofficial` |
| `refineable`, `derive-refineable` | `refineable-gpui-unofficial`, `derive-refineable-gpui-unofficial` |
| `http-client`, `http-client-tls`, `reqwest-client` | `http-client-gpui-unofficial`, `http-client-tls-gpui-unofficial`, `reqwest-client-gpui-unofficial` |
| `util`, `path`, `perf`, `util-macros` | `util-gpui-unofficial`, `path-gpui-unofficial`, `perf-gpui-unofficial`, `util-macros-gpui-unofficial` |
| `zlog`, `ztracing`, `ztracing-macro` | `zlog-gpui-unofficial`, `ztracing-gpui-unofficial`, `ztracing-macro-gpui-unofficial` |

Cargo builds only the dependencies needed for the selected platform, features
and targets. Some helpers are used only by optional functionality, examples or
tests. The mapping is maintained in `scripts/registry-crates.json`, and
`upstream.json` records the matching release and local source provenance.
The website's crate inventory and source pages cover local crates only.

When building a separate application against this checkout, override only the
retained framework/backend packages using their `xgpui-*` package identities.
For example, `gpui = { package = "xgpui", path = "../xgpui/gpui/crates/gpui" }`
keeps the `gpui` import name. Remove old patches pointing at helper
paths such as `gpui/crates/collections`; those directories no longer exist.
Run Cargo once without `--locked` to refresh the application's lockfile after
this dependency change, then resume locked builds.

## Extension labs and benchmarks

See [xgpui-ext-lab](extensions/extension-lab.md) for its targets, purpose and
unpublished status, and how it differs from the separate `xgpui-lab` application.

All native labs, repeatable benchmarks and browser demos live in the unpublished
`xgpui-ext-lab` package under `apps/xgpui-ext-lab/`. They share one manifest and
lockfile, with a separate binary target for each lab. Its `publish = false`
setting keeps development workloads out of the extension library release.

Use `just lab-build` to compile all native lab/benchmark binaries and
`just lab-test` to run unit and compatibility tests. The individual launch
recipes below retain their names. Browser build scripts select the `web` feature
without the default native targets; hosted demo URLs remain the same.

## Choose a sample app

Run `just` (or `just --list`) to list recipes without opening a window. All native
app recipes use release mode, committed lockfiles, the repository compatibility
configuration and the shared `target/` directory.

| Command | Opens |
| --- | --- |
| `just media` | Image [mipmapping ↗](https://en.wikipedia.org/wiki/Mipmap), filters, GPU surfaces and path effects |
| `just shaders` | Shader examples and typography |
| `just paints` | Paints, gradients, brushes and layer effects |
| `just gamut` | Native wide-SDR [gamut ↗](https://en.wikipedia.org/wiki/Gamut) comparison |
| `just color` | Color conversion and sRGB/P3 reference swatches |
| `just text` | Native/Parley text, paragraphs and scrolling |
| `just svg` | SVG rendering, zoom and scheduling |
| `just quad-bench` | Quad rendering benchmark |
| `just color-field-bench` | Color-field rendering benchmark |

Arguments after the recipe name go to the app. For example:

```sh
just shaders composition
just media --paths
just media --tiled # Generated 72 MP canvas; pan/zoom/cache pressure, blur/quality
just media --float-images # Supplied RGBA16F pixels; generated sample, native wide/sRGB comparison
just media --adjustments
just media --image "photos/my image.png"
just text --help
```

Select a debug build explicitly with `just profile=dev media`. App arguments are
not Cargo arguments. For extra Cargo features, use the direct Cargo command with
that app's manifest, as in the macOS runtime-shader example below. The app-level
justfiles remain available for specialized captures and benchmarks.

## Windows

Use PowerShell in a Visual Studio Developer terminal with the Windows SDK and
MSVC tools available. Select the native MSVC Rust toolchain, such as
`x86_64-pc-windows-msvc` on an x64 machine; the native renderer uses Direct3D 11.

```powershell
rustc -vV
just build
just shaders
```

The first recipe builds the libraries for Windows. The second builds and
opens the shader example application. Commands below also work in PowerShell;
they use single lines rather than shell-specific continuation characters.

## Linux

Build in a native Linux environment with a working graphical session and Vulkan
driver. The root harness enables both Wayland and X11; the corresponding headers
and libraries must be available, along with fontconfig, FreeType and xkbcommon.
The app uses the wgpu renderer. An SSH shell without a display can build the
libraries but cannot open the example window.

```sh
just build
just shaders
```

Run inside your normal Wayland or X11 desktop session. The repository's [Linux Vulkan CI workflow](https://github.com/gpui-luma-dev/xgpui/blob/main/.github/workflows/paints-backend.yml) lists the native packages used on Ubuntu if a linker reports
a missing system library.

## macOS

Use the native Rust toolchain for your machine. The default renderer is Metal;
the active Xcode installation must include the Metal compiler.

```sh
just build
just shaders
```

If the build fails because the Metal compiler is unavailable, the root harness
can embed shader source for compilation at runtime:

```sh
just build --features runtime-shaders
```

Features belong to the selected manifest. For the shader example application,
pass the platform dependency's feature explicitly:

```sh
cargo run --release --locked --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin shader-examples --features gpui_platform/runtime_shaders --target-dir target
```

Runtime compilation still needs a functioning Metal-capable system. It avoids
the build-time Metal compiler invocation; it does not replace the native SDK.

## Without just

The recipes wrap ordinary Cargo commands. If `just` or Bash is unavailable, build
and launch directly (these commands also work in PowerShell):

```sh
cargo build --release --locked
cargo run --release --locked --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin shader-examples --target-dir target
```

## Understand the Cargo layout

The root workspace contains `xgpui-build`, a private harness that builds captured
GPUI libraries for the host platform. **It does not launch an application or build
all extension libraries and labs.** The labs share the separate `xgpui-ext-lab` workspace and lockfile; extensions
and repository tools retain their own workspaces. Select a lab with
`--manifest-path apps/xgpui-ext-lab/Cargo.toml --bin <name>`.

The checked-in `.cargo/config.toml` maps the nine retained framework/backend
crate names to local source. Supporting crates retain their published identities
and are not overridden.
Standalone commands also pass `.cargo/validation.toml` for the pinned
compatibility overrides used by CI. Run from the root so Cargo discovers the
local configuration. Do not copy the command into an unrelated working directory.

Use `--target-dir target` to share the root output directory across native labs.
Without it, standalone workspaces use their own target directories. No personal
Cargo configuration or manifest edits are required for these recipes.

## Run the media lab

The lab generates its own image samples; no photo or video file is needed.

```sh
just media
```

Compare [atlas ↗](https://en.wikipedia.org/wiki/Texture_atlas) and mipmapped images, change filters, zoom and pan. Run
`just media --adjustments` for linear [exposure ↗](https://en.wikipedia.org/wiki/Exposure_value) and [vignette ↗](https://en.wikipedia.org/wiki/Vignetting) with a shared-source
original/adjusted comparison. Open the path
effects view with:

```sh
just media --paths
```

Use release mode for visual performance testing. Debug builds are useful for
stepping through code, but are not representative of rendering performance.
Keep 60 Hz as the baseline; a 120 Hz monitor is optional. Media surface runtime
validation on Windows/Linux remains tracked in
[issue #23](https://github.com/gpui-luma-dev/xgpui/issues/23).
See the [media guide](extensions/media-images.md) for backend limits and measurements.

## Check a change

Build the host framework/backend libraries without launching an app:

```sh
just build
just build-full # Also build every retained local library supported on this host.
```

For faster compile checks and an example extension test:

```sh
cargo check --workspace --locked
cargo test --locked --config .cargo/validation.toml --manifest-path extensions/crates/xgpui-color/Cargo.toml --lib
```

The first command checks the root harness and its dependencies. The second runs
the CPU color-math library tests as an example of selecting an extension directly.
Choose tests for the crate you changed. A root `cargo test` only tests the harness;
it is not a repository-wide test suite. Native GPU/window tests need the matching
platform and, where applicable, a graphical session.

## Preview this website

Generate the crate inventory and embedded guides, then serve `docs/` locally:

```sh
just docs-check
just docs-serve
```

The server uses `python3` on macOS/Linux and `python` on Windows. If Windows
only provides the Python launcher, use `just python=py docs-serve`. Choose another
port with `just docs-serve 8001`. Open
[localhost:8000](http://localhost:8000/#local-development). Node and Python are
only needed for the verification/server commands, not for native app builds.

The documentation generator does not build the WebAssembly demos. To include
them, install the Wasm target and the CLI version pinned by the build scripts:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129 --locked
just web-build
```

These scripts also require Bash and Python 3; on Windows run them in a Bash
environment with the same Rust and Python tools on PATH. Rebuild after changing
demo code, then reload the page. Use `localhost` for browser WebGPU access.

## Common build problems

| Symptom | What to check |
| --- | --- |
| Missing native library or linker | Check the selected host toolchain, SDK and platform libraries. On Windows use MSVC tools; on Linux check the development package named by the error. |
| Missing Metal compiler | Check the active Xcode installation, or use the macOS runtime-shader commands above. |
| Build succeeds but nothing opens | The root harness only builds libraries. Launch an app using its manifest. |
| Lockfile needs an update | Check the branch and manifest you selected. For an intentional dependency change, regenerate that workspace's lockfile and review the diff; do not delete all lockfiles. |
| Patch location does not contain `collections` or another removed helper | Remove stale helper path patches from the consuming app and refresh its lockfile; use the published packages listed above. |
| Local GPUI changes do not appear | Run from the repository root and keep its checked-in Cargo configuration enabled. |
| Browser demos are absent or stale | Build the Wasm assets separately; regenerating the documentation alone does not rebuild them. |

Build harness details are maintained in
[BUILDING.md](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/BUILDING.md).

The tiled lab's blur controls use a bounded [sRGB ↗](https://en.wikipedia.org/wiki/SRGB) preview so the capture and halo
fit the compositor's 1,024-device-pixel limit. `just media --video` also offers
linear adjustments, vignette and blur on generated native macOS Metal frames;
no video file is needed. See the [media pipeline guide](#media-pipeline) for
resource reuse, output color and backend limits.

## Run the repository test matrix

Run from the repository root with Python 3.11+, Rust/Cargo and `just` on PATH:

```sh
just test-all
just test-web
just test-ui
```

These are three distinct scopes. `test-all` runs the native automated matrix for
this host: repository Python/tool tests, framework/backend/extension unit and
integration tests, doctests, lab tests/builds, the original Metal deadlock
regression where supported, headless media/gradient/gamut checks, dense/instanced
pixel parity, benchmark captures and automated canvas input smoke. Some automatic
checks need a graphical session and may briefly open a window. It does not claim
all possible feature combinations, other operating systems, browser acceptance,
or manual UI inspection. The separate sibling `xgpui-lab` is not included; its
consumer migration is tracked in xgpui-lab#38.

`test-web` runs the browser calculation tests, generates/verifies the docs, builds
both Wasm collections, stages a fresh site and launches real Chromium. It discovers
all demo links in both galleries and forces WebGPU and WebGL2 separately, checking
startup, a visible canvas, the actual requested graphics context, resize and browser
errors. It saves screenshots. This is startup/resize smoke coverage, not pixel
reference parity or verification of every control. A missing browser backend is
recorded explicitly; it never counts as passing that backend. A failed build or
staging prerequisite prevents browser checks from testing stale assets.

`test-ui` builds and opens native labs sequentially, printing a specific checklist
for each. Exercise the controls, close the window, then enter `pass`, `fail`,
`unsupported` (with a capability reason), or `untested`, optionally followed by a
note. **Closing an app alone never passes it.** A crash overrides a claimed pass;
without a terminal, manual checks remain untested. This includes the dense and
instanced windows, paints/shaders, image/adjustment/float/tile/video/path views,
gamut, card/Mandelbrot canvas, text, SVG, color and the two benchmark windows.

Review the plan or select a smaller scope:

```sh
just test-all --list
just test-ui --list
just test-web --list
just profile=dev test-all --only 'media*,paint-*'
just test-ui --only 'portable-content*'
just test-all --timeout 1800
```

Use repeated `--only` options or comma-separated name globs. Selected runs are
marked as partial coverage in their reports; they do not establish acceptance of
the omitted checks. `--timeout` is a per-automated-command wall-clock limit,
including compilation. Timeouts terminate the command's process tree. Manual
inspection has no automatic pass or time limit. `--no-gpu` leaves GPU checks
untested. On Linux, use `--include-dma-buf` to attempt the native DMA-BUF fixture
with suitable hardware and Khronos validation; otherwise it remains untested.

Reports are saved under `target/test-runs/<UTC timestamp>-<mode>/`:

- `report.md` — readable per-check results and links to logs.
- `report.json` — commands, revision, dirty-tree indicator, host, profile, timing,
  operator notes, warning counts and result status.
- Per-command logs, screenshots, PNG captures and workload reports.
- Copies of standalone library resolution lockfiles for dependency evidence.

Results distinguish **passed**, **failed**, **unsupported** (not applicable to
this host), and **untested** (missing prerequisites, skipped, interrupted or not
inspected). Exit 0 means the selected applicable checks passed; 1 means at least
one failed; 2 means coverage is incomplete. A run with no passing checks cannot
report success. Ignored Rust tests and warnings remain visible in full Cargo logs;
the report does not redefine an ignored test as passed. In particular, subprocess
helper tests may be ignored as standalone entrypoints but executed by parent tests.

App/tool commands retain `--locked`. Standalone library manifests may not track a
lockfile and their test features may require a different resolution. They use
normal Cargo library resolution, which can create/update their local Cargo.lock;
the resolved locks are copied into the run artifacts. Use `--locked-libraries`
when intentionally enforcing existing standalone locks; missing/stale locks then
fail explicitly. The runner never retries a failed frozen build without its lock.
No dependencies or system software are installed automatically.

### Browser prerequisites

In addition to the toolchain required by `just web-build` (including the pinned
wasm-bindgen CLI), install Node.js on PATH and the optional test-only Python
browser dependency. Follow the [Playwright library setup](https://playwright.dev/python/docs/library):

```sh
python3 -m venv target/test-browser
target/test-browser/bin/python -m pip install -r scripts/browser-test-requirements.txt
target/test-browser/bin/python -m playwright install chromium
just python=target/test-browser/bin/python test-web
```

On Windows use `python` and `target/test-browser/Scripts/python.exe` for the
virtual environment interpreter. Linux additionally needs Chromium's OS runtime
libraries; provision them for the host rather than treating their absence as a
passing browser test. Browser screenshots and errors describe the tested
Chromium/adapter configuration, not every browser or physical display capability.
