# Extension browser examples

A standalone Wasm app for `xgpui-shader` and `xgpui-parley`. Both use the existing
GPUI web platform and wgpu renderer. The gallery offers automatic selection
(WebGPU with WebGL2 fallback), forced WebGPU, and forced WebGL2. A forced backend
fails explicitly if unavailable; check the browser console for adapter details.

From the repository root:

```sh
rustup target add wasm32-unknown-unknown
# Match the version recorded for wasm-bindgen in this app's Cargo.lock.
cargo install wasm-bindgen-cli --version 0.2.129 --locked
bash apps/extensions-web/build.sh
python3 apps/extensions-web/serve.py
```

Open <http://localhost:8081/>. Add `--release` to the build command for an optimized
build. The server accepts `--port 8082` if needed. Stop it with Ctrl-C; rebuild and
reload after changes. No Trunk, nightly toolchain, worker pool, or shared Wasm
memory configuration is required. The app opts into wgpu's
`fragile-send-sync-non-atomic-wasm` compatibility feature because GPUI paint
producers require `Send + Sync`; GPU handles stay on the browser thread. A
compile-time guard rejects atomic Wasm builds. The build script clears global
host Rust flags so native linker options cannot leak into the Wasm link. Serve on localhost (or HTTPS) for WebGPU and
clipboard access. The supplied server also sends cross-origin isolation headers.

## Local checks

- Shader: run both forced backends. All eleven cards should show the same paint
  in a rounded background, border and diamond path.
  Animate mesh/WGSL, pause, change group opacity, scroll, and resize. Check rounded
  edges, transparent stops, path coverage, scene order, and console errors.
- Parley: run both forced backends. Cycle the three samples; switch narrow/wide
  and resize. Verify Arabic shaping and Hebrew/Latin visual order. Click/drag,
  use arrows and Shift, type, delete grapheme clusters, copy/paste, and try IME
  composition/commit/cancel with the candidate window near the caret.
- The Parley view and input handler are shared directly with `apps/text-lab`.
  Timings use a browser clock; reported seed timings measure CPU layout only.
  Bundled IBM Plex Sans and Noto fonts need no system-font access or download.
  Their licenses remain beside the source font assets.

The demos exercise SDR output. `xgpui-gamut` currently has Metal and native D3D11
presentation adapters; a browser wide-gamut presentation path is separate work.
Compile/link checks do not establish browser GPU, clipboard, or IME correctness;
the checklist above is intended for local interactive acceptance.
