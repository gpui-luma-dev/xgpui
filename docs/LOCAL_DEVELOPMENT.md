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
cargo run --release --locked --config .cargo/validation.toml --manifest-path apps/shader-examples/Cargo.toml --features gpui_platform/runtime_shaders --target-dir target
```

Runtime compilation still needs a functioning Metal-capable system. It avoids
the build-time Metal compiler invocation; it does not replace the native SDK.

## Without just

The recipes wrap ordinary Cargo commands. If `just` or Bash is unavailable, build
and launch directly (these commands also work in PowerShell):

```sh
cargo build --release --locked
cargo run --release --locked --config .cargo/validation.toml --manifest-path apps/shader-examples/Cargo.toml --target-dir target
```

## Understand the Cargo layout

The root workspace contains `xgpui-build`, a private harness that builds captured
GPUI libraries for the host platform. **It does not launch an application or build
all extension libraries and labs.** Apps, extensions and repository tools have
their own workspaces and lockfiles; select them with `--manifest-path`.

The checked-in `.cargo/config.toml` maps captured crate names to local source.
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

```sh
cargo check --workspace --locked
cargo test --locked --config .cargo/validation.toml --manifest-path extensions/xgpui-ext/crates/xgpui-color/Cargo.toml --lib
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
| Local GPUI changes do not appear | Run from the repository root and keep its checked-in Cargo configuration enabled. |
| Browser demos are absent or stale | Build the Wasm assets separately; regenerating the documentation alone does not rebuild them. |

Build harness details are maintained in
[BUILDING.md](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/BUILDING.md).

The tiled lab's blur controls use a bounded [sRGB ↗](https://en.wikipedia.org/wiki/SRGB) preview so the capture and halo
fit the compositor's 1,024-device-pixel limit. `just media --video` also offers
linear adjustments, vignette and blur on generated native macOS Metal frames;
no video file is needed. See the [media pipeline guide](#media-pipeline) for
resource reuse, output color and backend limits.
