# Italy browser photo comparison

Route: `examples/extensions/?demo=italy` in the extension Wasm gallery (24 demos).
Uses the same three WebKit Italy JPEGs and prepared linear-P3 pixel fixtures as
xgpui-lab, with linked WebKit credit and local provenance/checksums.

The browser presents original, unmodified JPEGs through image elements and its
own profile handling. The Rust/Wasm `italy_srgb_preview` export creates bounded
P3-storage-equivalent, clipped sRGB PNG derivatives using xgpui-color. Radio
controls switch presentation for the same selected photo, including thumbnails.
There is one main image and three selectable thumbnails. No top comparison row.
The control does not change monitor mode or GPUI window presentation. This route
does not use a GPUI GPU canvas; forced GPU backend parameters do not affect it.

Validation on macOS:

- The full-photo test processes all three 600×400 fixtures, decodes generated
  PNGs, checks dimensions/opaque alpha, and verifies orange-umbrella sRGB
  clipping against independently recorded source behavior. Invalid selection
  returns an error.
- Locked/offline release Wasm build and strict Wasm Clippy pass.
- Website generation, documentation verification and website staging pass;
  all original JPEGs, provenance and preparation script are packaged locally.
- Local Chromium browser: route loads, display-gamut report is separate from
  presentation selection, all three thumbnails change the main image, both
  presentation modes update status and image, and the highlighted photograph
  remains a photograph. Verified scrolling to the full main image and credit.

This is browser runtime evidence on one platform, not physical color accuracy,
Safari/Firefox acceptance or native GPUI wide-gamut delivery. Original and sRGB
source versions need not become identical on an sRGB display. The original
highlight JPEG contains markings, independent of presentation mode.

Commands:

```sh
cargo test --offline --locked --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --no-default-features --features web --test italy-photo
cargo clippy --offline --locked --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --no-default-features --features web --bin extensions-web --target wasm32-unknown-unknown -- -D warnings
bash apps/xgpui-ext-lab/extensions-web/build.sh --release --offline
cargo run --locked --offline --manifest-path tools/crate-roadmap/Cargo.toml
node docs/verify.mjs
python3 scripts/stage_website.py --require-web
```
