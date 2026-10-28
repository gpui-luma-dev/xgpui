# Crate roadmap website

From the repository root:

```sh
cargo run --locked --manifest-path tools/crate-roadmap/Cargo.toml
```

Open `docs/index.html` in your browser. The generated site works
offline, including search, hash navigation and line-linked source pages. No
server is needed. Generated `data.js` and `source/` pages are ignored by Git. Regeneration rebuilds
`source/` so removed local crates do not leave stale source pages.

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

The crate inventory covers the nine local framework/backend packages and six
local extensions. Published supporting packages from `gpui-unofficial` are
documented in [Local Development](LOCAL_DEVELOPMENT.md#local-crates-and-published-dependencies),
not indexed as local source. Original upstream package names remain in provenance
metadata and historical release reviews.

The dependency view covers local packages and preserves manifest dependency
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

Inventory routes and local directories use normalized local names. Each crate
page displays its downstream package identity and version from the manifest.
Dependency edges map downstream identities back to the stable local routes. The original gpui-unofficial
repository and release remain the provenance source; upstream.json records both
original and local package maps. Review narrative links and examples after a
normalization-map change as well as after a release sync.

## Maintaining guides

Edit maintained Markdown in `docs/extensions/` or `LOCAL_DEVELOPMENT.md`, then
regenerate the site. The generator embeds guides into `data.js`, including tables,
code blocks and heading anchors. Internal document links become site routes;
section links use `&section=heading-id`. No network fetch or Markdown runtime is
needed. Do not edit generated `data.js` or `source/` output.

Feature status, examples, measurements and limitations belong in the relevant
guide rather than this maintenance document:

- [Labs & benchmarks](extensions/extension-lab.md): the unpublished
  `xgpui-ext-lab` package, native commands and browser targets.
- [Color](extensions/color-math.md) and [wide-gamut windows](extensions/high-gamut-windows.md).
- [Brushes](extensions/brushes.md) and [extension paints](extensions/extension-paints.md).
- [Instanced shaders](extensions/instanced-wgsl-v2.md) and [typography](extensions/typography.md).
- [Text](extensions/parley-text.md) and [SVG](extensions/svg-rendering.md).
- [Media images](extensions/media-images.md) and [media pipeline](extensions/media-pipeline.md).

The verifier checks embedded routes, source locations, internal links and section
targets. The unpublished lab package is documented at `#extension-lab` but is not
part of the reusable library API inventory.

Code examples use vendored Prism assets in `vendor/prism/`; `code-examples.js`
applies Rust highlighting to rendered pages. Preserve the accompanying license.

## Browser demos and local preview

Both browser entry points belong to `apps/xgpui-ext-lab/Cargo.toml`. Build scripts
select the appropriate binary with `--no-default-features --features web` and
preserve the existing JavaScript/WASM names and hosted URLs.

Install the WASM target and the CLI version pinned in `.github/workflows/pages.yml`:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129 --locked
just docs
just web-build
just docs-serve
```

`just web-build` runs both release packaging scripts:

- `apps/xgpui-ext-lab/extensions-web/build.sh` refreshes its `dist/` and
  `docs/examples/extensions/`.
- `apps/xgpui-ext-lab/shader-examples/build.sh` refreshes its `dist/` and
  `docs/examples/shaders/`.

Generated demo assets are ignored by Git. The unified gallery at
`http://localhost:8000/examples/extensions/` lists 19 hosted demos across both
entry points, with native labs and launch instructions listed separately.
The shader companion is at `http://localhost:8000/examples/shaders/`.

Use `localhost` or `127.0.0.1` for browser previews. Wildcard bind addresses such
as `[::]` and `0.0.0.0` are not trustworthy WebGPU origins. Demo loaders redirect
those HTTP addresses to localhost while preserving the route and backend choice.

Rebuild WASM after changing demo code or branches, and regenerate documentation
after editing guides. Rebuilding a native app does not update browser assets.
Smoke-test the relevant WebGPU and WebGL2 routes after rendering changes;
`node docs/verify.mjs` does not execute the WASM applications.

## GitHub Pages

Select **Settings → Pages → Source: GitHub Actions**. The Pages workflow builds
and verifies the website, packages both browser applications and publishes
`docs/` on pushes to `main` or manual dispatch. Relative asset and source links
support the domain root, localhost and the legacy repository URL prefix.
Local regeneration does not deploy the site.


## Custom-domain migration

The intended public documentation URL is **https://xgpui.dev/**. Repository
preparation is tracked in [issue #61](https://github.com/gpui-luma-dev/xgpui/issues/61).
The previous URL is https://gpui-luma-dev.github.io/xgpui/.
Preparing these files does not configure the domain: DNS, Pages settings and the
production cutover remain pending until the migration is approved.

Keep the GitHub Actions publishing source and the `docs/` upload artifact.
Do not add `docs/CNAME`: custom Actions publishing ignores that file; the
repository Pages custom-domain setting controls the mapping. Keep asset URLs,
source links, demo navigation and JavaScript/WASM loaders relative so the same
build works at the domain root and in localhost previews.

Before cutover, run `just docs-check`, `just web-build` and `just docs-serve`.
At `http://localhost:8000/`, check search, crate routes, source line links and
section anchors. Exercise both `examples/extensions/?demo=color` and
`examples/shaders/?example=minimal`, with `&backend=webgpu` and `&backend=webgl`.
Inspect console/network requests for failures and obsolete `/xgpui/` prefixes.
The Node verifier checks links at the domain root, localhost and a project prefix;
it does not replace running the browser applications.

After approval, perform the following in order:

1. Save existing DNS records and the known-good deployment run for rollback.
2. Verify `xgpui.dev` in **gpui-luma-dev organization Settings → Pages** using
   GitHub's exact TXT challenge hostname/value. Retain the verification record.
3. In repository **Settings → Pages**, retain **GitHub Actions** and save custom
   domain `xgpui.dev` before changing routing DNS.
4. Replace conflicting parking/forwarding records while preserving unrelated
   mail/TXT records. Set apex A records to `185.199.108.153`, `185.199.109.153`,
   `185.199.110.153` and `185.199.111.153`; set `www` CNAME to
   `gpui-luma-dev.github.io` without a scheme or repository path. Recheck GitHub's
   current records before applying them; avoid wildcard records.
5. Wait for DNS checks and certificate provisioning, then enable **Enforce HTTPS**.
   DNS propagation and HTTPS availability can take up to 24 hours.
6. Merge the reviewed migration PR only with explicit approval, watch
   **Publish documentation**, and confirm its environment URL is `https://xgpui.dev/`.
7. Check HTTP/HTTPS redirects, `www`, the old project URL (including nested demo
   paths and queries), hash routes, direct nested refreshes and both demo backends.
   Inspect for 404s, mixed content, WASM failures and incorrect MIME types.
8. Update the repository About website URL and record deployment, DNS, HTTPS and
   browser evidence in issue #61. Confirm a later Actions deployment preserves
   the custom domain.

For rollback, restore the previous known-good site through the reviewed workflow.
If removing the domain mapping, remove or restore Pages routing DNS before
removing the GitHub custom-domain association; retain ownership verification and
unrelated records. Recheck the previous project URL after rollback.

Official instructions (recheck when executing):
[custom domain configuration](https://docs.github.com/en/pages/configuring-a-custom-domain-for-your-github-pages-site/managing-a-custom-domain-for-your-github-pages-site),
[ownership verification](https://docs.github.com/en/pages/configuring-a-custom-domain-for-your-github-pages-site/verifying-your-custom-domain-for-github-pages),
and [troubleshooting](https://docs.github.com/en/pages/configuring-a-custom-domain-for-your-github-pages-site/troubleshooting-custom-domains-and-github-pages).
