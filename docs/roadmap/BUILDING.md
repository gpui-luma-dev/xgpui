# Crate roadmap website

From the repository root:

```sh
cargo run --locked --manifest-path tools/crate-roadmap/Cargo.toml
```

Open `docs/roadmap/site/index.html` in your browser. The generated site works
offline, including search, hash navigation and line-linked source pages. No
server is needed. The generated `site/` directory is ignored by Git.

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
node docs/roadmap/verify.mjs
```

The JavaScript verifier checks the generated artifact and interaction logic
without opening a GUI. It does not perform visual browser validation.

Website source: `template.html`, `style.css`, `app.js`, `guides.js`, `build-guide.js`, `extensions.js`, `notes.json`.
The parent `index.html` redirects to the generated site. The architecture and
execution guides live in `guides.js`; review their source references after syncing.
The build-options/app/WASM guide lives in `build-guide.js`. Its manifest evidence
and feature reference are refreshed by the Rust generator.
Generator source: `tools/crate-roadmap` (independent Cargo workspace).
No upstream Rust sources, release synchronization scripts or README are changed.

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
`extensions.js` owns the feature guides and reference links. All published extension guides live in `docs/roadmap/extensions/` at the
repository root, including integration, API, producer and backend references.
Source-side READMEs link to these maintained guides.

`xgpui-parley` is indexed with its source/API/dependency tabs and a code-first
text guide (`parley-text.md`). The generator renders the extension Markdown documents into `data.js`, including
tables, code blocks and heading anchors. Internal document links become site
routes; section links use `&section=heading-id`. No network fetch or Markdown
runtime is needed in the browser. Edit the maintained sources, then regenerate;
do not edit the ignored `site/` output. The verifier checks extension routes,
source locations, internal links and section targets as well as the existing site.
