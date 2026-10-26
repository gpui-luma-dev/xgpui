# Crate roadmap website

From the repository root:

```sh
cargo run --locked --manifest-path tools/crate-roadmap/Cargo.toml
```

Open `docs/index.html` in your browser. The generated site works
offline, including search, hash navigation and line-linked source pages. No
server is needed. Generated `data.js` and `source/` pages are ignored by Git.

The standalone Rust tool reads the captured package map in `upstream.json`,
Cargo manifests and library module trees. It excludes the transformed staging
copies. `notes.json` contains manually reviewed descriptions and review
hypotheses. After syncing a release, regenerate the site and review those notes.
Adding or removing captured crates requires updating the notes; the generator
rejects an incomplete package map.

This is a syntactic source inventory, not a resolved API stability report.
It does not expand macros or resolve glob re-exports, evaluate cfg combinations,
or prove that private-module declarations are consumer-reachable. Trait impl
headers are candidates too. Binary entrypoints, examples and benches are outside
the library traversal. Use compiler/rustdoc inspection and representative
consumer builds before changing visibility or removing APIs.

The dependency view covers captured packages and preserves manifest dependency
kinds, optional flags and target conditions. It does not include external
packages or resolve feature activation. Source pages reflect the local capture,
including downstream edits, rather than claiming byte identity with upstream.

Validation:

```sh
cargo fmt --manifest-path tools/crate-roadmap/Cargo.toml --check
cargo test --locked --manifest-path tools/crate-roadmap/Cargo.toml
cargo clippy --locked --manifest-path tools/crate-roadmap/Cargo.toml --all-targets -- -D warnings
node docs/verify.mjs
```

The JavaScript verifier checks the generated artifact and interaction logic
without opening a GUI. It does not perform visual browser validation.

Website source: `index.html`, `style.css`, `app.js`, `guides.js`, `extensions.js`, `notes.json`.
The architecture reference lives in `guides.js`; review its source references after syncing.
The sidebar links to **Local Development** (`#local-development`), embedded from
[LOCAL_DEVELOPMENT.md](LOCAL_DEVELOPMENT.md). It covers native builds and app
launches on Windows, Linux and macOS, plus website/Wasm previews.
The root `justfile` lists every native app (including the two benchmark apps),
uses release mode by default and provides `docs`, `docs-check`, `docs-serve` and
`web-build` recipes. The guide retains direct Cargo commands as a fallback.
Build harness details remain in [BUILDING.md](BUILDING.md).
Generator source: `tools/crate-roadmap` (independent Cargo workspace).

The rendering explorer lives in `pipeline.js`. It uses SVG and a deliberately
small scene model, not the native renderer. Its model tests cover overlap
ordering, clipping, explicit layers, batch texture boundaries and replay.
Playback is opt-in, stops after one sequence, and is cleaned up on navigation.
Reduced-motion preferences suppress SVG movement.

Capture packages use normalized local names. The original gpui-unofficial
repository and release remain the provenance source; upstream.json records both
original and local package maps. Review narrative links and examples after a
normalization-map change as well as after a release sync.

## Extensions to GPUI

`xgpui-color` is indexed with its own Use/source/API/dependency tabs and the
`color-math.md` guide. It is a standalone math crate with no runtime dependencies
and introduces no GPUI API changes. Gamut depends on color; its shader rendering adapter is optional and comparison
tests use shader as a development dependency. Shader remains independent of both crates.

The sidebar group **Extensions** immediately precedes **Core**, using the same
heading and link styles. `xgpui-shader` is indexed from its actual manifest and
entrypoint, with usage code as its default crate page plus source/API/dependency
tabs. `xgpui-gamut` has its own manifest, source/API/dependency tabs and code-first
producer/presentation guide. Native backend ownership is documented separately. `#extensions` remains a linkable index.
The generator keeps local `extension_packages` separate from upstream provenance.
`extensions.js` owns the feature guides and reference links. All published extension guides live in `docs/extensions/` at the
repository root, including integration, API, producer and backend references.
Source-side READMEs link to these maintained guides.

`xgpui-parley` is indexed with its source/API/dependency tabs and a code-first
text guide (`parley-text.md`). The generator renders the extension Markdown documents into `data.js`, including
tables, code blocks and heading anchors. Internal document links become site
routes; section links use `&section=heading-id`. No network fetch or Markdown
runtime is needed in the browser. Edit the maintained sources, then regenerate;
do not edit generated `data.js` or `source/` output. The verifier checks extension routes,
source locations, internal links and section targets as well as the existing site.

`xgpui-svg` is indexed as an experimental extension with Use/source/API/dependency
tabs. Its maintained contract is `docs/extensions/svg-rendering.md`; the crate
README points to that source to avoid a second copy. The guide covers native
samples and attribution, shared scheduling/accounting, measurements and deferred
work. The browser gallery includes `?demo=svg`, with attributed source downloads,
synchronized bounded/viewport-rerasterized views, zoom/pan, shared budget controls
and a ten-tile gallery. Wasm rasterization is
currently on the browser thread; native responsiveness results do not transfer.

## GitHub Pages

Select **Settings → Pages → Source: GitHub Actions**. The Pages workflow builds and verifies the website, then publishes `docs/` on pushes to `main` or manual dispatch. Relative asset and source links support the repository URL prefix.


## Browser extension demos

The sidebar's **Browser extension demos** link opens `examples/extensions/`, the
WebAssembly demo index. It lists 17 built browser demos across both apps, seven older
interactive source-gallery examples, and three developer Wasm harnesses. Source-only
entries are labeled and do not link to nonexistent hosted routes.
The gallery and the extension crate guides link to the demo source on GitHub.
The Pages workflow builds `apps/extensions-web` in release mode with the pinned
wasm-bindgen CLI and copies its generated assets into `docs/examples/extensions/`
before uploading the site. These artifacts are ignored by Git. The same build
script refreshes both `apps/extensions-web/dist/` and `docs/examples/extensions/`,
so a local docs server serves the current Wasm app too:

```sh
bash apps/extensions-web/build.sh --release
python3 -m http.server 8000 --bind 127.0.0.1 --directory docs
```

Open `http://localhost:8000/examples/extensions/?demo=svg`. Rebuild after changing
branches or demo code, then reload the browser; an older copied HTML file may
reject new demo names. The standalone demo server is also available; see
[extensions-web](../apps/extensions-web/README.md).

## Instanced WGSL v2

The maintained guide is `extensions/instanced-wgsl-v2.md`, embedded in `data.js`
under `#instanced-wgsl-v2`. The main navigation, Extensions overview and shader
crate guide link to it. It records implementation status, limits, validation and
remaining acceptance rather than treating the RFC as fully complete.

Native launch is primary: `just --justfile apps/shader-examples/justfile ui`.
The companion uses the same Rust example views and WGSL sources, including the
cyan clipping boundary, Clipping ON/OFF toggle and draw-order legend. Browser
startup is separate; rebuilding the native app alone does not refresh WASM.

The Pages workflow builds `apps/shader-examples` in release mode. Its build script
copies generated assets to both the app's `dist/` and ignored
`docs/examples/shaders/`. To preview the current companion with the site:

```sh
bash apps/shader-examples/build.sh --release
python3 -m http.server 8000 --bind 127.0.0.1 --directory docs
```

Open `http://localhost:8000/#instanced-wgsl-v2` or
`http://localhost:8000/examples/shaders/`. These are separate from the older
`examples/extensions/` gallery. Deploying updated sources still requires the
normal Pages workflow; local regeneration alone does not update the public site.

## To-do

- [x] Update the web docs crate list with `xgpui-color`, including its standalone
  color-math role and dependency relationships; regenerate and verify the site.


## Color runtime diagnostics and desktop comparison

The Color / Gamut browser demo is linked from the Extensions overview and color
crate Use guide. It reports P3 source endpoints, sRGB mapping, renderer canvas
color space (or unknown), and `(color-gamut: p3)` display capability. Native P3
output is demonstrated by `color-lab`, with paired sRGB/P3 reference swatches and
window negotiation status. Launch instructions and interpretation are maintained
in `extensions/color-math.md`, `extensions/high-gamut-windows.md` and the native
app README, with a link from the instanced WGSL guide. These are demo additions;
no new GPUI API or crate dependencies between extension libraries are introduced.
Rebuild the Wasm assets for runtime changes and regenerate the site for guide
changes before previewing or publishing. The sidebar wordmark has no boxed icon.

### Local WebGPU origin

Use `http://localhost:8000/` when opening the documentation preview. Python's
server may display `[::]` as its listening address; `http://[::]:8000/` is a
wildcard bind address, not a trustworthy browser loopback origin. At that URL,
WebGPU is hidden even when the same browser supports it at localhost. The demo
loaders correct HTTP `[::]` and `0.0.0.0` URLs to localhost while preserving the
port, example and forced backend. They do not rewrite LAN or remote addresses.

## Selectable typography milestones

The maintained `typography.md` guide is embedded at `#typography` and linked from
the shader guide. The Text gallery includes `?demo=typography`, sharing its Rust
view with the native shader examples. This covers the static, motion and styling milestones of issue #28: selectable
LTR text, authentic copy, source inspection, native accessibility, cluster-aware
reveal, deterministic scramble, bounded wave motion, gradient/glow/drop-shadow styling and
reduced-motion fallback. Wrapping and bidirectional selection remain deferred.

## Dedicated media images

`xgpui-media` is indexed under Extensions with its Use/source/API/dependency tabs.
Its maintained guide, `extensions/media-images.md`, is embedded under
`#media-images` and linked from the Extensions overview. It records delivered
features, test commands, backend evidence and pending work separately. Update
that status with each completed media milestone, regenerate the website and run
`node docs/verify.mjs`. A cross-compile is recorded as compilation evidence;
backend execution is marked verified only after its runtime tests pass.

The native `apps/media-lab` provides synchronized atlas/mipmap viewports, loading
stress controls and upload diagnostics. Its launch and Metal/wgpu capture commands
are maintained in `extensions/media-images.md` and embedded in the media guide.

The media guide also covers ordered SDR color filters and their native lab controls,
including alpha preservation, encoded-sRGB semantics and source-upload reuse.

The SVG guide now documents retained contour trim/dash effects and the media lab
path view, with launch commands, geometry limits and native capture checks.

The media guide includes the first native video slice: bounded latest-frame
presentation, macOS BGRA/IOSurface import, GPU lifetime rules, and video-lab
controls/capture. The initial Windows adapter accepts dedicated immutable
same-device BGRA textures and keyed NT-handle snapshots with nonblocking
acquisition and retained ownership. The initial Linux DMA-BUF adapter covers
dedicated immutable linear BGRA Vulkan snapshots, with device capability gating
and host-recorded ownership acquisition. Windows/Linux runtime verification,
Linux explicit fences/pool reuse and broader formats remain pending. Playback adapters are tracked separately in issue #40.

All media-lab launch/build/capture commands default to release; debug is explicit.

The media guide documents a generated Linux DMA-BUF native validation recipe,
including Vulkan export/import prerequisites, release execution, pixel capture,
validation-error counting and native runtime status. Host layout checks and Linux
cross-compilation remain separate evidence from a successful native GPU run.

The macOS surface lab now cycles BGRA8 and NV12 full/video range with BT.601/709.
The media guide documents direct two-plane sampling, explicit matrix/range and
chroma assumptions, retained native lifetime, five release capture modes and
interactive UI verification. Transfer/gamut/HDR conversion remain outside this slice.

The media guide includes a repeatable release upload CPU benchmark and versioned
macOS Metal/wgpu JSON observations under `benchmarks/`. Each observation keeps raw
frame samples and resource-pool evidence. The guide distinguishes CPU measurements
from GPU timestamps and whole-window frame times; its numbers are not thresholds.
Benchmark JSON links resolve relative to the documentation root when embedded.

The media guide also includes a visible release-window cadence recipe, three raw
Metal observations and branch-review status. UI interval diagnostics retain long
stalls during continuous redraw; idle periods are excluded by request state.
Render-start cadence remains distinct from GPU and presentation timing.

The media guide now defines 60 Hz as the baseline for sustained large-image
scrolling. Its release native feed combines 24 cards with repeated 3072×2048
source replacements, five-second cached phases and 30 seconds of loading. Raw
reports preserve long intervals and explicit provisional acceptance criteria;
ProMotion observations remain additional coverage, not the baseline requirement.


The WASM gallery now includes image filters (`?demo=media-filters`), mipmapping
(`?demo=mipmaps`) and contour trim/dash (`?demo=path-effects`), each with automatic,
forced WebGPU and forced WebGL2 launch links. Release browser checks and the
synchronous-fixture/native-worker distinction are recorded in the media guide.
The gallery has 17 demos; the media/vector demos share the actual Rust algorithms
and renderer paths. Building regenerates local Pages assets without deploying.

## Code examples

Rust examples use the same Prism 1.30.0 core and Rust grammar as Luma, vendored
locally under `vendor/prism/` with its MIT license. `code-examples.js` applies
Rust syntax colors after page changes and API inventory updates. Rust fence flags
(`rust,ignore`, `rust,no_run`) select Rust. There are no language labels, toolbars
or Copy controls; other languages keep their existing plain-text presentation.
