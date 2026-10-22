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

Website source: `template.html`, `style.css`, `app.js`, `guides.js`, `build-guide.js`, `notes.json`.
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
