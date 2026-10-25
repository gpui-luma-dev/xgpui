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
Native build prerequisites and commands remain in [BUILDING.md](BUILDING.md).
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
WebAssembly demo index. It lists the three hosted extension demos, seven older
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
python3 -m http.server 8000 --directory docs
```

Open `http://localhost:8000/examples/extensions/?demo=svg`. Rebuild after changing
branches or demo code, then reload the browser; an older copied HTML file may
reject new demo names. The standalone demo server is also available; see
[extensions-web](../apps/extensions-web/README.md).
