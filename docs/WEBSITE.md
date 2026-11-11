# Website

The website at **https://xgpui.dev/** is the static `pages/` directory. Edit an
HTML file, refresh the browser. There is no generator, Markdown conversion or
content index. The only build step is compiling the two browser demo
collections to WebAssembly.

```sh
just docs-serve     # serve pages/ at http://localhost:8000
just docs-check     # check links, anchors, assets and legacy routes
```

`docs-check` runs `scripts/check_pages.py` (Python 3, standard library only). It
reads the checked-in pages and runs no build. It does not render pages or run the
demos.

## Layout

- `index.html`: overview. Also forwards bookmarks from the former single-page
  site (`#route&section=…`, `#crate=…&tab=…`) to the new pages.
- `<guide>.html`: one page per guide, for example `media-images.html`. Each
  guide is its only copy; repository Markdown links to `https://xgpui.dev/<guide>.html`.
- `extensions.html`: the extension overview.
- `crates/<crate>.html`: one page per captured and extension crate: role,
  extension guide, source and manifest links, rustdoc on docs.rs, and the review
  hypothesis.
- `404.html`: forwards former `source/<path>.html#L<line>` URLs to the same
  lines on GitHub.
- `style.css`, `code-examples.js` and `vendor/prism/` (Rust highlighting; keep
  the license), `images/`, `benchmarks/`, favicons.
- `examples/extensions/` and `examples/shaders/`: hand-written demo pages,
  `examples/graphics.mjs` (WebGPU preflight and recovery) and
  `examples/gallery.css`.

Pages work without JavaScript. Scripts only highlight code and forward legacy
URLs. Every page carries the same navigation rail; when adding a guide or crate,
add it to the rail on every page and to the forwarding lists in `index.html`.
`docs-check` reports anything missing.

`docs/` holds repository documentation (building, releasing, local development,
investigations and release records). It is not published.

`index.html` states the upstream source baseline. `docs-check` fails when it no
longer matches `upstream.json`, so update it after a release sync.

## Browser demos

Both entry points belong to `apps/xgpui-ext-lab/Cargo.toml`. Install the WASM
target and the wasm-bindgen CLI pinned in `.github/workflows/pages.yml`:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129 --locked
just web-build      # write bundles into pages/examples/
just docs-serve
just web-clean      # remove only the generated bundle files
```

`build.sh` writes the JavaScript bindings, the `.wasm` module, `build-id.txt`
and the data files shared with native labs (Italy photos, SVG samples, color
reference data and font licenses). These are ignored by Git. The extension
gallery compares `build-id.txt` with the ID compiled into the module and asks
for a reload when a browser cached a mismatched pair.

Open `http://localhost:8000/examples/extensions/` and
`http://localhost:8000/examples/shaders/`. Use `localhost` or `127.0.0.1`.
Wildcard bind addresses such as `[::]` and `0.0.0.0` are not trustworthy WebGPU
origins, so the demo loaders redirect them to localhost.

Rebuild WASM after changing demo code or switching branches. After rendering
changes, smoke-test the affected demos with `&backend=webgpu` and
`&backend=webgl`, and check the console and network panels for errors.

## Publishing

`.github/workflows/pages.yml` runs on manual dispatch only. It checks `pages/`,
builds both demo collections, checks that the bundles exist and uploads `pages/`
as is. Internal documentation and tooling are never uploaded.

Select **Settings → Pages → Source: GitHub Actions**. After deployment, repeat
the browser checks at `https://xgpui.dev/` and refresh nested URLs directly to
catch stale assets.

## Custom domain

Production traffic for `xgpui.dev` was migrated in
[issue #61](https://github.com/gpui-luma-dev/xgpui/issues/61) and
[PR #64](https://github.com/gpui-luma-dev/xgpui/pull/64) (verified 2026-10-28).

- The repository Pages custom domain is `xgpui.dev`, with **Enforce HTTPS**. The
  certificate covers `xgpui.dev` and `www.xgpui.dev`.
- Organization ownership is verified by the TXT record at
  `_github-pages-challenge-gpui-luma-dev.xgpui.dev`. Retain it.
- Apex A records: `185.199.108.153`, `185.199.109.153`, `185.199.110.153`,
  `185.199.111.153`. `www` is a CNAME to `gpui-luma-dev.github.io`. Preserve
  unrelated DNS records.
- No `CNAME` file is needed: Actions publishing uses the repository setting.
- `http://xgpui.dev/`, `https://www.xgpui.dev/` and the former project URL
  `https://gpui-luma-dev.github.io/xgpui/…` redirect to the HTTPS apex, keeping
  path and query. Keep site links relative so localhost previews also work.

```sh
dig A xgpui.dev +short
dig CNAME www.xgpui.dev +short
dig TXT _github-pages-challenge-gpui-luma-dev.xgpui.dev +short
curl -IL https://www.xgpui.dev/
curl -IL 'https://gpui-luma-dev.github.io/xgpui/examples/extensions/?demo=color&backend=webgl'
```

For rollback, redeploy a known-good commit through the workflow. If removing the
domain mapping, change Pages DNS before removing the GitHub custom-domain
association, and keep the ownership record. GitHub's
[custom domain](https://docs.github.com/en/pages/configuring-a-custom-domain-for-your-github-pages-site/managing-a-custom-domain-for-your-github-pages-site)
and [troubleshooting](https://docs.github.com/en/pages/configuring-a-custom-domain-for-your-github-pages-site/troubleshooting-custom-domains-and-github-pages)
guides have the details.
