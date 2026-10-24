'use strict';

const starterManifest = `[package]
name = "my-gpui-app"
version = "0.1.0"
edition = "2024"
publish = false

[workspace]

[lib]
crate-type = ["cdylib", "rlib"]

[features]
default = []
runtime-shaders = ["gpui_platform/runtime_shaders"]
web-threads = ["gpui_web/multithreaded"]

[dependencies]
gpui = { package = "gpui", path = "../../gpui/crates/gpui", default-features = false }

[target.'cfg(not(target_family = "wasm"))'.dependencies]
gpui_platform = { package = "gpui-platform", path = "../../gpui/crates/gpui-platform", default-features = false, features = ["font-kit", "wayland", "x11"] }

[target.'cfg(target_family = "wasm")'.dependencies]
gpui_web = { package = "gpui-web", path = "../../gpui/crates/gpui-web", default-features = false }
wasm-bindgen = "0.2.120"
`;
const starterLibrary = `use gpui::{App, WindowOptions, div, prelude::*};
use std::borrow::Cow;

struct Root;

impl Render for Root {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        div().font_family("IBM Plex Sans").child("Hello from GPUI")
    }
}

pub fn run(application: gpui::Application) {
    application.run(|cx: &mut App| {
        cx.text_system().add_fonts(vec![Cow::Borrowed(
            include_bytes!("../assets/AppFont.ttf").as_slice(),
        )]).expect("load application font");
        cx.open_window(WindowOptions::default(), |_, cx| cx.new(|_| Root))
            .expect("open application window");
    });
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    use std::{rc::Rc, sync::Arc};
    gpui_web::init_logging();
    let platform = Rc::new(gpui_web::WebPlatform::new_with_backend(
        cfg!(feature = "web-threads"),
        gpui_web::WebBackendPreference::Auto,
    ));
    let http = Arc::new(platform.fetch_http_client());
    run(gpui::Application::with_platform(platform).with_http_client(http));
}
`;
const starterMain = `fn main() {
    my_gpui_app::run(gpui_platform::application());
}
`;

function buildCode(value) { return '<pre class="build-code"><code>' + escapeHtml(value) + '</code></pre>'; }
function buildFile(path) {
  const text = data.build_files[path];
  return '<details><summary>' + escapeHtml(path) + '</summary>' + buildCode(text) + '</details>';
}
function buildFeatureReference() {
  return crates.map(c => {
    const entries = Object.entries(c.features);
    return '<details><summary>' + escapeHtml(short(c.package)) + ' · ' + entries.length + ' declared feature entries</summary><p><code>' + escapeHtml(c.package) + '</code>. Defaults: <code>' + escapeHtml((c.features.default || []).join(', ') || '(none)') + '</code></p>' +
      (entries.length ? '<div class="table-wrap"><table><thead><tr><th>Feature</th><th>Enables / forwards to</th></tr></thead><tbody>' + entries.map(([key,value]) => '<tr><td><code>' + escapeHtml(key) + '</code></td><td><code>' + escapeHtml(value.join(', ') || '(no forwarding; source cfg flag)') + '</code></td></tr>').join('') + '</tbody></table></div>' : '<p>No explicit feature entries.</p>') +
      '<p>Optional dependencies can introduce additional implicit features. See the complete manifest below.</p>' + buildFile((data.extension_packages[c.package] || data.provenance.packages[c.package]) + '/Cargo.toml') + '</details>';
  }).join('');
}
function buildGuidePage() {
  const s = guideSource;
  return header('Build guide / native applications & WASM', 'Build GPUI and your app', 'Choose the target platform, then its integration and renderer, then the optional services. Cargo features, shader compilation and browser threading are separate choices.') +
    '<div class="notice"><strong>Validation scope:</strong> the existing repository guide/CI covers native macOS builds. Temporary projects checked the exact starter below on native macOS (runtime shaders) and on stable-Rust WASM with web threading disabled. Cross-platform recipes and threaded WASM below are not claims that this session compiled and launched every configuration. No GUI was launched.</div>' +
    '<h2>1. Choose the build you actually need</h2><div class="table-wrap"><table><thead><tr><th>Goal / target</th><th>Platform and rendering</th><th>Important options</th></tr></thead><tbody><tr><td>Build this repository</td><td>Private library harness; no application UI</td><td>Debug/release; root runtime-shaders</td></tr><tr><td>macOS app</td><td>MacPlatform → shared Apple Metal renderer</td><td>font-kit, runtime_shaders, screen-capture</td></tr><tr><td>Linux / FreeBSD app</td><td>Wayland/X11 integration → wgpu; native context requests Vulkan or GL</td><td>wayland, x11, platform services and system libraries</td></tr><tr><td>Windows app</td><td>WindowsPlatform → native DirectX renderer</td><td>windows-manifest, screen-capture</td></tr><tr><td>Browser WASM app</td><td>WebPlatform → wgpu browser WebGPU or WebGL2</td><td>Single-threaded versus shared-memory threaded build; runtime WebBackendPreference</td></tr><tr><td>Headless tests / benches</td><td>Platform/headless renderer contracts; availability varies</td><td>test-support or bench-support, compatible dependency patches</td></tr><tr><td>iOS / Android</td><td>Shared code contains mobile-oriented pieces</td><td>No corresponding construction branch in the captured current_platform facade; not a ready app recipe here</td></tr></tbody></table></div><p>Building the wgpu crate on macOS does not switch MacPlatform from its Metal renderer. Likewise, a shader-compilation option does not select a new rendering backend.</p><p class="evidence">Source trail: ' + s('gpui_platform.rs',80,'Target selection','gpui-platform') + ' · ' + s('wgpu_context.rs',475,'Native wgpu backend request','gpui-wgpu') + ' · ' + s('gpui_web.rs',1,'Browser backend behavior','gpui-web') + '</p>' +
    '<h2>2. Build the captured native libraries</h2><p>Run from the repository root. This uses the committed root lockfile and the private build-all harness. The host toolchain, native SDKs and system libraries must match the target. A successful macOS build does not validate Linux, Windows or WASM.</p>' +
    buildCode('cargo build --locked\ncargo build --release --locked\n\n# macOS: embed Metal source for compilation at runtime\ncargo build --locked --features runtime-shaders\n\ncargo check --workspace --locked\ncargo clippy --workspace --locked') +
    '<p>The root spelling is <code>runtime-shaders</code> (hyphen). Captured Apple/macOS/platform manifests use <code>runtime_shaders</code> (underscore). Default Apple builds compile shaders during the build and need the selected Xcode/Metal toolchain. Runtime shaders avoid that compile step, but still require the platform SDK/frameworks and compile shaders when the renderer initializes. This does not implement hot reload or a custom shader extension API.</p><p>The root harness includes target-selected libraries and several support crates. It is deliberately broad. Its <code>--no-default-features</code> does not remove the target dependencies or explicit features written into the harness manifest. Do not use that harness as the application manifest for a minimal browser app.</p>' + buildFile('tools/build-all/Cargo.toml') +
    '<h2>3. Compile existing examples and tests</h2><p>These commands build examples without launching them. Standalone captured workspaces use their own lockfiles. The repository compatibility config restores the Proptest and async-process revisions required by this capture.</p>' +
    buildCode('cargo build --config .cargo/validation.toml \\\n  --manifest-path gpui/crates/gpui/Cargo.toml \\\n  --examples --features gpui_platform/font-kit --target-dir target\n\n# Runtime Metal variant: use both forwarded features\ncargo build --config .cargo/validation.toml \\\n  --manifest-path gpui/crates/gpui/Cargo.toml \\\n  --example hello_world \\\n  --features gpui_platform/font-kit,gpui_platform/runtime_shaders \\\n  --target-dir target\n\n# Library tests; not an exhaustive OS/backend test matrix\ncargo test --config .cargo/validation.toml \\\n  --manifest-path gpui/crates/gpui/Cargo.toml \\\n  --lib --features gpui_platform/runtime_shaders') +
    '<p>The root <code>cargo test</code> only tests the root harness. The platform facade exposes headless renderer construction only under test/bench support, and returns no renderer for some targets. Tests requiring real windows or platform main-thread integration need separate treatment.</p>' + buildFile('.cargo/validation.toml') +
    '<h2>4. Understand feature ownership and defaults</h2><p><code>--features</code> is interpreted for the selected package. A core feature named <code>wayland</code> is not a substitute for configuring the Linux backend. Features are additive: another dependency can re-enable a default or a forwarded feature. <code>--no-default-features</code> affects the selected package, not every dependency recursively. <a href="https://doc.rust-lang.org/cargo/reference/features.html">Cargo feature semantics</a>.</p><div class="table-wrap"><table><thead><tr><th>Option</th><th>Meaning / owner</th></tr></thead><tbody><tr><td>font-kit</td><td>Platform forwards to macOS/shared Apple font support; wgpu has a separate optional font-kit feature. Core has a same-named flag with no dependency forwarding.</td></tr><tr><td>wayland / x11</td><td>Platform forwards to Linux; the Linux package defaults to both. Core flags alone do not disable the Linux package defaults.</td></tr><tr><td>runtime_shaders</td><td>Apple Metal compilation strategy; facade forwards through macOS. Root harness renames it runtime-shaders.</td></tr><tr><td>multithreaded</td><td>Web/facade defaults. Web enables wasm_thread and scheduler/wasm-threads. Shared-memory setup is independent of WebGPU/WebGL selection.</td></tr><tr><td>test-support / bench-support</td><td>Test APIs, headless support or benchmarks depending on package. Core bench-support includes profiler; bench is its compatibility alias.</td></tr><tr><td>screen-capture</td><td>Target-specific capture integrations, not a renderer selector. Platform forwards to relevant backends.</td></tr><tr><td>windows-manifest</td><td>Windows resource/manifest build support in core; facade enables it for Windows.</td></tr><tr><td>profiler / leak-detection / stacker / proptest</td><td>Core instrumentation, entity-handle tracking, stack support and property-test integration. Not interchangeable rendering options.</td></tr></tbody></table></div><p>Use the full feature reference at the end for exact forwarding. Avoid treating <code>--all-features</code> as an all-platform rendering test: it can enable threading, capture and test dependencies together while still compiling only one target.</p>' +
    '<h2>5. Create a downstream application against this fork</h2><p>This is a documentation starter for <code>apps/my-app</code>, not a new app installed by this documentation work. Its separate workspace keeps it out of the generated root build harness. Invoke Cargo from the repository root so <code>.cargo/config.toml</code> patches all captured transitive packages to local paths. External consumers must supply equivalent patches; a direct path dependency by itself does not redirect every versioned GPUI dependency to your fork.</p><p>Create <code>apps/my-app/Cargo.toml</code> with:</p>' + buildCode(starterManifest) +
    '<p>The native platform dependency enables both Linux integrations and macOS font support; target gating determines which implementation builds. This is a cross-platform starter, not an exclusive Wayland-only configuration. To make an exclusive configuration, audit the Linux dependency defaults too.</p><p>Create <code>src/lib.rs</code>:</p>' + buildCode(starterLibrary) +
    '<p>Create <code>src/main.rs</code>:</p>' + buildCode(starterMain) +
    '<p>Supply the font asset used above. The captured example uses IBM Plex Sans; retain its font license when distributing it. The browser platform starts with an empty font database, so loading application fonts before opening the window matters.</p>' +
    buildCode('mkdir -p apps/my-app/assets\ncp gpui/crates/gpui/assets/fonts/ibm-plex-sans/IBMPlexSans-Regular.ttf \\\n  apps/my-app/assets/AppFont.ttf\n\n# First build creates the application lockfile\ncargo build --config .cargo/validation.toml --manifest-path apps/my-app/Cargo.toml\n\n# Subsequent reproducible optimized build\ncargo build --locked --config .cargo/validation.toml \\\n  --manifest-path apps/my-app/Cargo.toml --release\n\n# macOS runtime Metal compilation\ncargo build --config .cargo/validation.toml \\\n  --manifest-path apps/my-app/Cargo.toml --features runtime-shaders') +
    '<p>Commit the app lockfile for reproducibility. If copying the app outside this checkout, adjust the paths and copy/regenerate the complete local-package patch map plus compatibility overrides. <a href="https://doc.rust-lang.org/cargo/reference/config.html">Cargo config discovery</a> follows the invocation directory; <code>--manifest-path</code> alone does not make Cargo discover config beside an arbitrary manifest.</p>' + buildFile('.cargo/config.toml') +
    '<h2>6. Build the browser app: single-threaded baseline</h2><p>Use <code>wasm32-unknown-unknown</code>, not WASI. The starter deliberately depends directly on <code>gpui_web</code> with default features off for WASM. The facade target dependency currently enables gpui_web defaults, so disabling the facade default alone does not establish a thread-free graph. The runtime <code>allow_multi_threading=false</code> switch is not a replacement for disabling the Cargo threading feature.</p>' +
    buildCode('rustup target add wasm32-unknown-unknown\n\ncargo build --config .cargo/validation.toml \\\n  --manifest-path apps/my-app/Cargo.toml \\\n  --lib --target wasm32-unknown-unknown --release\n\n# Inspect the actual dependency feature graph\ncargo tree --manifest-path apps/my-app/Cargo.toml \\\n  --target wasm32-unknown-unknown -e features\n\n# Find the resolved wasm-bindgen version before installing its matching CLI\ncargo tree --manifest-path apps/my-app/Cargo.toml \\\n  --target wasm32-unknown-unknown -i wasm-bindgen') +
    '<p>Install <code>wasm-bindgen-cli</code> at the version shown by that dependency graph (substitute the actual version for VERSION), then generate browser glue. The manifest range is not necessarily the resolved version.</p>' +
    buildCode('cargo install wasm-bindgen-cli --version VERSION --locked\n\nwasm-bindgen \\\n  apps/my-app/target/wasm32-unknown-unknown/release/my_gpui_app.wasm \\\n  --target web --out-dir apps/my-app/dist/pkg') +
    '<p>The independent app workspace normally writes its artifacts under apps/my-app/target unless CARGO_TARGET_DIR or Cargo config overrides it. Create <code>apps/my-app/dist/index.html</code>:</p>' +
    buildCode('<!doctype html>\n<html lang="en"><head><meta charset="utf-8"><title>My GPUI app</title></head>\n<body style="margin:0">\n<script type="module">\n  import init from "./pkg/my_gpui_app.js";\n  await init();\n</script>\n</body></html>') +
    '<p>Serve the dist directory over HTTP for local development and HTTPS for deployment, using a static server that serves WASM correctly. Do not open this application HTML with file://. The module initializer invokes the wasm-bindgen start function; the web platform creates its document-owned canvas. <a href="https://wasm-bindgen.github.io/wasm-bindgen/reference/cli.html">CLI reference</a> and <a href="https://wasm-bindgen.github.io/wasm-bindgen/reference/deployment.html">browser deployment modes</a>.</p><p>Single-threaded does not mean background CPU work moves off the browser main thread. Dedicated-thread operations are not generally available without scheduler wasm-threads; source comments explicitly describe a runtime panic for that path when threading is disabled.</p>' +
    '<h2>7. Browser rendering: Auto, WebGPU or WebGL2</h2><p>In the starter start function, choose <code>WebBackendPreference::Auto</code>, <code>::WebGpu</code> or <code>::WebGl</code>. Auto probes WebGPU, removes the failed canvas if needed and retries WebGL2 on a replacement canvas. Forced choices try only that backend. They are runtime constructor choices, not Cargo features named webgpu/webgl on the facade.</p><p>Browser WebGPU requires an appropriate secure context and a supported browser/adapter (<a href="https://developer.mozilla.org/en-US/docs/Web/API/WebGPU_API">WebGPU requirements</a>). WebGL2 fallback is a separate capability path, not a promise of feature/performance equivalence. This capture supports one top-level browser window; opening another or reopening a closed one produces WebWindowError. Native multi-window application behavior therefore needs a web-specific design.</p><p class="evidence">Source trail: ' + s('platform.rs',147,'WebPlatform constructors','gpui-web') + ' · ' + s('platform.rs',238,'Graphics fallback','gpui-web') + ' · ' + s('wgpu_context.rs',93,'Backend preference enum','gpui-wgpu') + ' · ' + s('gpui_web.rs',1,'Window lifecycle restriction','gpui-web') + '</p>' +
    '<h2>8. Threaded WASM is a separate build and hosting configuration</h2><p>The app feature <code>web-threads</code> forwards to gpui_web/multithreaded and sets the runtime constructor flag in the starter. Compile with a nightly toolchain and rust-src so the standard library can be rebuilt for atomics/shared memory. The captured gallery supplies the following target settings; place them in a separate app build config for the threaded variant, not in the default native build path.</p>' + buildCode(data.build_files['gpui/crates/gpui-web/examples/hello_web/.cargo/config.toml'].replace('["std,panic_abort"]', '["std", "panic_abort"]')) +
    '<p>Save the displayed settings as <code>apps/my-app/threaded.toml</code>. They retain the captured gallery target flags and spell build-std as separate standard-library entries. The complete threaded path remains unvalidated in this session.</p>' +
    buildCode('rustup toolchain install nightly --component rust-src --target wasm32-unknown-unknown\n\ncargo +nightly build --config .cargo/validation.toml \\\n  --config apps/my-app/threaded.toml \\\n  --manifest-path apps/my-app/Cargo.toml --lib \\\n  --target wasm32-unknown-unknown --features web-threads --release') +
    '<p>Then regenerate matching wasm-bindgen output and deploy all generated worker/module assets. Shared memory needs cross-origin isolation and a secure context; configure the server with <code>Cross-Origin-Opener-Policy: same-origin</code> and <code>Cross-Origin-Embedder-Policy: require-corp</code>, and ensure dependent resources satisfy that policy. Verify <a href="https://developer.mozilla.org/en-US/docs/Web/API/Window/crossOriginIsolated"><code>crossOriginIsolated</code></a>, SharedArrayBuffer availability, worker loading and shared WASM memory in the browser. <a href="https://wasm-bindgen.github.io/wasm-bindgen/examples/raytrace.html">Official threaded-WASM build explanation</a>.</p><p><strong>Not validated here:</strong> the threaded application and worker packaging. The captured gallery uses a pinned Zed wasm_thread Git fork, while the published web/scheduler manifests use the registry crate; this also needs review before calling the gallery a reproducible threaded reference.</p>' +
    '<h2>9. Captured gallery and cross-platform limitations</h2><p>The existing gallery is at <code>gpui/crates/gpui-web/examples/hello_web</code>. Its manifest still refers to paths such as <code>../../../gpui</code> and <code>../../../gpui_platform</code>, rather than the captured package directory names. Its other binaries, watch paths and build settings need adjustment as well. It is not currently a drop-in gallery build command. Its serve config also enables automatic browser opening, so inspect that before using a server command.</p>' + buildFile('gpui/crates/gpui-web/examples/hello_web/Cargo.toml') + buildFile('gpui/crates/gpui-web/examples/hello_web/trunk.toml') +
    '<p>On Linux install the headers/libraries required by the enabled Wayland/X11, keyboard, font and graphics dependencies for your distribution; this repository does not provide a verified universal package-manager command. On Windows build in a matching MSVC/Windows SDK environment. Adding a Rust target is not by itself a cross-compilation SDK setup. The macOS CI run does not establish either native target as tested.</p><p>Root native support and this browser library check do not establish an iOS/Android app build. Start from a working platform construction, event loop and renderer integration before describing a mobile application recipe.</p>' +
    '<h2>10. Complete captured feature reference</h2><p>Generated from every captured Cargo manifest on each roadmap refresh. Includes default entries and exact forwarding strings. Implicit optional-dependency features remain visible in the full manifest. This table inventories options; it does not assert that every combination compiles.</p>' + buildFeatureReference();
}
