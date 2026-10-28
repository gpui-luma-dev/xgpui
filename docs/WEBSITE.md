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


## Production custom domain

The public documentation URL is **https://xgpui.dev/**. The migration tracked in
[issue #61](https://github.com/gpui-luma-dev/xgpui/issues/61) is complete.
[PR #64](https://github.com/gpui-luma-dev/xgpui/pull/64) prepared the repository;
DNS and repository Pages settings now route production traffic to the domain.
The repository About website URL is also `https://xgpui.dev/`.

### Final configuration (verified 2026-10-28)

- Repository **Settings → Pages → Source** remains **GitHub Actions**
  (`build_type: workflow`). `.github/workflows/pages.yml` builds the docs and
  both WASM collections, uploads `docs/`, and deploys to the `github-pages`
  environment on pushes to `main` or manual dispatch.
- The repository Pages custom domain is `xgpui.dev`; **Enforce HTTPS** is enabled.
  GitHub reports verified domain ownership and an approved certificate covering
  both `xgpui.dev` and `www.xgpui.dev`.
- Domain ownership is verified for the `gpui-luma-dev` organization using the TXT
  record at `_github-pages-challenge-gpui-luma-dev.xgpui.dev`. Retain this record.
  The custom-domain routing setting belongs to this repository; other repositories
  in the organization can keep their own Pages domains.
- Apex A records are `185.199.108.153`, `185.199.109.153`, `185.199.110.153` and
  `185.199.111.153`. The `www` CNAME points to `gpui-luma-dev.github.io` without
  a scheme or repository path. Preserve unrelated DNS records.
- No `docs/CNAME` is required: custom Actions publishing ignores that file;
  the repository Pages setting controls the mapping. Keep asset URLs, source
  links, demo navigation and JavaScript/WASM loaders relative for localhost
  previews and domain-root hosting.

### Deployment and verification evidence

The [migration deployment](https://github.com/gpui-luma-dev/xgpui/actions/runs/40732570766)
passed. A [subsequent deployment](https://github.com/gpui-luma-dev/xgpui/actions/runs/40737138680)
after PR #65 also passed and preserved the custom domain.

Live HTTP checks on 2026-10-28 confirmed:

- `https://xgpui.dev/` serves the documentation with HTTP 200.
- `http://xgpui.dev/`, `https://www.xgpui.dev/` and the previous project URL,
  `https://gpui-luma-dev.github.io/xgpui/`, redirect to the HTTPS apex domain.
- The legacy nested route
  `/xgpui/examples/extensions/?demo=color&backend=webgl` redirects to
  `https://xgpui.dev/examples/extensions/?demo=color&backend=webgl`, retaining
  the demo path and query parameters.
- Both `/examples/extensions/` and `/examples/shaders/` serve HTTP 200 with
  backend-selection queries. Their `extensions_web_bg.wasm` and
  `shader_examples_bg.wasm` assets serve HTTP 200 with `application/wasm`.

These HTTP checks verify routing and asset delivery, not GPU rendering. When
changing demos, run the browser checks below as well as the automated verifier.

### Ongoing checks

```sh
dig A xgpui.dev +short
dig CNAME www.xgpui.dev +short
dig TXT _github-pages-challenge-gpui-luma-dev.xgpui.dev +short
curl -IL http://xgpui.dev/
curl -IL https://www.xgpui.dev/
curl -IL 'https://gpui-luma-dev.github.io/xgpui/examples/extensions/?demo=color&backend=webgl'
```

For website changes, run `just docs-check`. For demo changes, also run
`just web-build` and `just docs-serve`. At `http://localhost:8000/`, check search,
crate routes, source line links and section anchors. Exercise
`examples/extensions/?demo=color` and `examples/shaders/?example=minimal`, with
`&backend=webgpu` and `&backend=webgl`. Inspect console/network requests for
failures and obsolete `/xgpui/` prefixes. The Node verifier checks links at the
domain root, localhost and a project prefix; it does not execute the browser
applications. After deployment, repeat relevant checks at `https://xgpui.dev/`
and refresh nested URLs directly to check for stale assets.

For rollback, restore the previous known-good site through the reviewed workflow.
If removing the domain mapping, remove or restore Pages routing DNS before
removing the GitHub custom-domain association; retain ownership verification and
unrelated records. Recheck the previous project URL after rollback.

Official instructions (recheck when executing):
[custom domain configuration](https://docs.github.com/en/pages/configuring-a-custom-domain-for-your-github-pages-site/managing-a-custom-domain-for-your-github-pages-site),
[ownership verification](https://docs.github.com/en/pages/configuring-a-custom-domain-for-your-github-pages-site/verifying-your-custom-domain-for-github-pages),
and [troubleshooting](https://docs.github.com/en/pages/configuring-a-custom-domain-for-your-github-pages-site/troubleshooting-custom-domains-and-github-pages).
