# Linear P3 GPU readback

`test-support` exposes `Window::render_to_linear_p3()` and Metal's
`MetalHeadlessRenderer::render_scene_to_linear_p3()`. Both synchronously re-render
through the production renderer and copy the associated extended linear-P3
RGBA16Float composition target before display color management. They preserve
binary16 words, including negative and above-one channels. The older image API
continues to return an 8-bit sRGB preview.

The window API returns an explicit unsupported error on other backends and
requires WideGamutSdr on Metal. It does not switch the window's color mode.
Readback uses a shared staging buffer, aligned rows stripped from the result,
and checks completion of both scene and copy commands. Existing wide-target
extent/memory limits apply. This blocks on GPU completion and is intended for
on-demand validation, not per-frame telemetry.

Commands (run from the repository root):

```sh
cargo test --offline --config .cargo/validation.toml --manifest-path gpui/crates/gpui-apple/Cargo.toml --features test-support wide_sdr
cargo test --offline --config .cargo/validation.toml --manifest-path extensions/crates/xgpui-shader/Cargo.toml --features portable-shaders wide_capture_tests
```

Results on macOS/Metal: 13 native wide tests and 7 wide-capture tests pass.
The new capture test covers 96 combinations: P3/Rec.2020, primary colors and
near-black, alpha 0/0.5/1, direct/two nested captures, and blur radius 0/4/8.
References use an independent 120-digit primary/white xy solve. Error bounds
account for binary16 rounding stages, subnormal absolute error, matrix term
magnitudes and f32 arithmetic; assertions also explicitly reject loss of
negative or above-one channels. Readback tests cover odd-width row padding,
resize, exact extended FP16 words and rejection of the default color mode.

The sibling chromaticity lab checks the current native sample's 3×3 center
pixels against independently calculated composition starting at the actual
packed FP16 source bytes. Adobe RGB green at half alpha passed in the real
window, directly (17.2% of budget) and with nested opacity/blur (5.6%). This
isolates GPU conversion/composition from ingestion; source/ICC precision has
separate tests. A constant center check does not validate blur edges, and
neither these tests nor screenshots measure the monitor's physical output.
Windows/Linux floating-point readback is not implemented by this change.

A broader strict Clippy run on the entire macOS crate reports 18 existing
findings in unchanged code (display link, event handling, pasteboard, callback
types and native range casts). No readback code is flagged. The standalone lab
passes strict Clippy. The shader test suite requires `portable-shaders`; its
unfeatured test build has existing typography feature-gating errors.
