'use strict';

const shaderGuideRoutes = ['shader-models', 'typography', 'brushes', 'paint-validation', 'extension-paints', 'shader-producers', 'paint-contract-hardening', 'portable-wgsl-v1', 'instanced-wgsl-v2', 'portable-paints', 'native-dx11-paints'];

function isExtensionRoute(route) {
  return route === 'extensions' || Object.hasOwn(data.extension_documents, route);
}
function extensionTitle(route) {
  return route === 'extensions' ? 'Extensions to GPUI' : data.extension_documents[route].title;
}
const browserDemoNames = {
  canvas: 'spatial card canvas', mandelbrot: 'Mandelbrot explorer',
  typography: 'Selectable typography', brushes: 'Brush gallery', layers: 'Layer effects', shader: 'Gradient paints',
  chromaticity: 'Chromaticity & gamut', 'pixel-metadata': 'Pixels + metadata', hermite: 'Hermite editor', color: 'Color & gamut', svg: 'Progressive SVG', parley: 'Text & paragraphs',
};
function browserDemoLinks(demo) {
  return '<p class="demo-actions"><a class="demo-open" href="examples/extensions/?demo=' + demo + '">Open ' + browserDemoNames[demo] + ' ↗</a><a href="https://github.com/gpui-luma-dev/xgpui/tree/main/apps/xgpui-ext-lab/extensions-web">Demo source</a></p>';
}
function brushDemoLinks() {
  return '<section class="demo-entry" aria-label="Brush and layer live demos"><div class="demo-entry-heading"><span>TRY IT LIVE</span><a href="examples/extensions/">All browser demos →</a></div><div class="demo-launch-grid">' +
    '<a class="demo-launch" href="examples/extensions/?demo=brushes"><div class="demo-thumb brush-thumb"></div><strong>Open brush gallery ↗</strong><span>Gradients, freeform &amp; Hermite meshes, noise, text and paths.</span></a>' +
    '<a class="demo-launch" href="examples/extensions/?demo=hermite"><div class="demo-thumb brush-thumb"></div><strong>Open Hermite editor ↗</strong><span>Live mesh points, U/V tangent angles and lengths, and direct/legacy comparison.</span></a>' +
    '<a class="demo-launch" href="examples/extensions/?demo=layers"><div class="demo-thumb layer-thumb"></div><strong>Open layer effects ↗</strong><span>Subtree filters, blur, destination blending and backdrops.</span></a></div></section>';
}
function liveDemoOverview() {
  return '<section class="demo-entry" aria-label="Live browser demos"><div class="demo-entry-heading"><span>LIVE BROWSER DEMOS</span><a href="examples/extensions/">Browse all demos →</a></div><div class="demo-launch-grid">' +
    '<a class="demo-launch" href="examples/extensions/?demo=brushes"><div class="demo-thumb brush-thumb"></div><strong>Brushes &amp; gradients ↗</strong><span>Color fields, mesh, noise, glyphs and paths.</span></a>' +
    '<a class="demo-launch" href="examples/extensions/?demo=layers"><div class="demo-thumb layer-thumb"></div><strong>Layer effects ↗</strong><span>Blur, filters and destination blending.</span></a>' +
    '<a class="demo-launch" href="examples/shaders/"><div class="demo-thumb shader-thumb"></div><strong>Shader playground ↗</strong><span>Particles, composition, shadows and animated borders.</span></a></div></section>';
}
function gamutDemoPanel() {
  return '<section class="demo-entry" aria-label="Wide-gamut demos"><div class="demo-entry-heading"><span>EXPLORE COLOR &amp; WIDE GAMUT</span><a href="examples/extensions/">All demos →</a></div><div class="demo-launch-grid">' +
    '<article class="demo-launch gamut-native"><div class="demo-thumb gamut-thumb" aria-hidden="true"></div><strong>Native Display P3 lab</strong><span>Compare wide SDR content with ordinary sRGB controls on macOS or Windows. Run from the repository root:</span>' +
    extensionCode('sh', 'cargo run --locked --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin gamut-lab') +
    '<span>Move the window to a wide-gamut display. Check <code>Effective: WideGamutSdr</code> and <code>Display wide gamut: Some(true)</code>. These report negotiated support, not display calibration.</span></article>' +
    '<a class="demo-launch" href="examples/extensions/?demo=color"><div class="demo-thumb gamut-preview-thumb" aria-hidden="true"></div><strong>Open color comparison ↗</strong><span>Interactive browser preview of interpolation and gamut mapping. Rendered as sRGB; use the native lab to compare actual Display P3 output.</span></a></div></section>';
}
function colorDemoLinks() {
  return '<h2>Color comparison · browser and native desktop</h2>' +
    browserDemoLinks('chromaticity') + browserDemoLinks('pixel-metadata') + browserDemoLinks('color') +
    '<p><a href="#color-workflows">Start with a color task →</a> · <a href="#color-labs">What each lab proves →</a></p>' +
    '<p>The browser compares interpolation and gamut mapping as an sRGB preview. Its runtime panel reports source space, mapping, actual canvas color space (or unknown) and browser-reported wide-gamut capability (at least P3).</p>' +
    extensionCode('sh', 'cargo run --locked --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin color-lab') +
    '<p>The native companion pairs mapped sRGB and Display P3 swatches using the same 32 reference samples per row. Check effective <code>WideGamutSdr</code> and display support, then compare on your wide-gamut monitor. Display capability does not verify calibration.</p>';
}
function instancedDemoLinks() {
  return '<h2>Instanced shaders · native app first</h2>' +
    '<p>Typed fragment instances, ordered batching, configurable gradient shadows, soft animated borders and direct main-pass rendering. Experimental SDR subset; Windows runtime acceptance remains outstanding.</p>' +
    extensionCode('sh', 'just --justfile apps/xgpui-ext-lab/shader-examples/justfile ui\njust --justfile apps/xgpui-ext-lab/shader-examples/justfile composition') +
    '<p><a href="#instanced-wgsl-v2">API, examples and implementation status →</a> · <a href="examples/shaders/">WASM companion →</a> · <a href="https://github.com/gpui-luma-dev/xgpui/tree/main/apps/xgpui-ext-lab/shader-examples">Demo source</a></p>';
}
function extensionCode(language, source) {
  return '<pre><code class="language-' + language + '">' + escapeHtml(source) + '</code></pre>';
}
function extensionPage(route) {
  if (route === 'whats-new') {
    return '<p><a href="#overview">← Overview</a></p><article class="extension-document">' + data.extension_documents[route].html + '</article>';
  }
  if (route === 'local-development' || route === 'extension-lab') {
    const doc = data.extension_documents[route];
    return '<p><a href="#overview">← Overview</a></p><article class="extension-document">' + doc.html + '</article>';
  }

  if (route === 'high-gamut-windows') {
    const doc = data.extension_documents[route];
    return header('Extensions', 'xgpui-gamut', 'Explicit wide SDR content alongside normal sRGB controls.') +
      '<p class="muted">' + crateLink('xgpui-gamut') + ' · Backend owners: ' + crateLink('gpui-apple') + ' / ' + crateLink('gpui-windows') + '.</p>' +
      '<nav class="tabs" aria-label="Wide-gamut sections"><a href="#high-gamut-windows" aria-current="page">Use &amp; contract</a><a href="#high-gamut-windows&section=typed-windows-output">Windows output</a><a href="#gpui-public-api">GPUI API changes</a></nav>' +
      gamutDemoPanel() + '<article class="extension-document">' + doc.html.replace(/^<h1[^>]*>.*?<\/h1>\s*/, '') + '</article>';
  }

  const banner = ['color-workflows', 'pixels-and-metadata', 'icc-profiles', 'color-labs'].includes(route)
    ? '<p class="guide-intro"><strong>Color tasks.</strong> Pixels, metadata, storage and presentation. <a href="#color-workflows">Choose a task →</a></p>'
    : route === 'color-math'
    ? '<p class="guide-intro"><strong>Standalone color math.</strong> CPU conversions, gamut mapping, gradients and WGSL source without GPUI.</p>'
    : route === 'canvas'
    ? '<p class="guide-intro"><strong>Experimental spatial canvas.</strong> Camera, retained scenes and clipped procedural grids.</p>'
    : route === 'svg-rendering'
    ? '<p class="guide-intro"><strong>Experimental SVG extension.</strong> Progressive rasterization, shared admission and accounted image budgets.</p>'
    : route === 'media-pipeline'
    ? '<p class="guide-intro"><strong>Pixel and media pipeline.</strong> Explicit metadata, linear adjustments, RGBA16F sources, progressive tiled canvases, bounded preview blur and native video effects.</p>'
    : route === 'media-images'
    ? '<p class="guide-intro"><strong>Images and video frames.</strong> Practical examples for loading, adjusting and displaying media in GPUI.</p>'
    : route === 'parley-text'
    ? '<p class="guide-intro"><strong>Text backend extension.</strong> Parley shaping and geometry; GPUI caches, atlas and native input.</p>'
    : route === 'high-gamut-windows'
    ? '<p class="guide-intro"><strong>Wide-gamut presentation extension.</strong> Native xgpui-gamut producers; per-window Metal presentation.</p>'
    : route === 'extensions' || route === 'gpui-public-api'
      ? '<p class="guide-intro"><strong>Downstream extensions to GPUI.</strong> Paints, window color presentation, text backends and SVG rendering are independent opt-in capabilities.</p>'
      : '<p class="guide-intro"><strong>Paint and shader extension.</strong> Opt in through <code>xgpui-shader</code>; ordinary output is sRGB, with explicit native wide-SDR programs.</p>';
  if (route !== 'extensions') {
    const doc = data.extension_documents[route];
    return '<p><a href="#extensions">← Extensions to GPUI</a></p>' + banner + (route === 'canvas' ? browserDemoLinks('canvas') + browserDemoLinks('mandelbrot') : route === 'typography' ? browserDemoLinks('typography') : route === 'brushes' ? brushDemoLinks() : route === 'color-math' ? colorDemoLinks() : route === 'svg-rendering' ? browserDemoLinks('svg') : route === 'instanced-wgsl-v2' ? instancedDemoLinks() : '') + '<article class="extension-document">' + doc.html + (route === 'gpui-public-api' ? publicApiSourceLinks() : '') + '</article><p class="evidence">Source: <code>' + escapeHtml(doc.source) + '</code></p>';
  }
  return header('Downstream / opt-in', 'Extensions to GPUI', 'Independent paint, presentation, text and SVG extensions.') + banner +
    '<p><a href="examples/extensions/">Browser extension demos →</a> · <a href="https://github.com/gpui-luma-dev/xgpui/tree/main/apps/xgpui-ext-lab/extensions-web">Demo source</a></p>' +
    liveDemoOverview() + '<h2>Color tasks</h2><div class="cards">' + extensionCard('color-workflows', 'Start here: wide gamut versus Display P3; choose a practical task.') + extensionCard('pixels-and-metadata', 'Supply pixels, transfer, alpha and storage explicitly.') + extensionCard('icc-profiles', 'Use a supplied RGB matrix/TRC profile within the supported subset.') + extensionCard('color-labs', 'Browser previews, native presentation and what the measurements prove.') + '</div><h2>1. Paints and shaders</h2>' +
    extensionCode('rust', `use xgpui_shader::{painted, SolidColor};
use std::sync::Arc;

painted().bg_paint(Arc::new(SolidColor::new([1.0, 0.2, 0.0, 1.0])?));`) +
    '<p><code>xgpui-shader</code>: gradients, meshes and portable WGSL. Metal, wgpu and native D3D11 adapters; ordinary programs use sRGB. Explicit linear-P3 programs require a native wide-gamut window.</p>' +
    '<div class="cards">' + extensionCard('shader-models', 'Choose shader paints or instanced shaders; migrate existing names.') + extensionCard('extension-paints', 'Start here: dependencies, containers, borders, paths and interaction.') + extensionCard('typography', 'Gradient fills, glyph glow/shadows, typewriter, wave and scramble with stable selection and native accessibility.') + extensionCard('brushes', 'Universal brushes, glyphs, paths, mesh gradients and cached layer effects; pastel waves demo.') + extensionCard('shader-producers', 'Mesh sources, parameters, limits and caches.') + extensionCard('paint-contract-hardening', 'Preparation, recording and failure behavior.') + extensionCard('portable-wgsl-v1', 'Rust parameters + portable WGSL source.') + extensionCard('instanced-wgsl-v2', 'Typed main-pass instances, ordered batching, native app and WASM companion.') + extensionCard('portable-paints', 'wgpu support and validation.') + extensionCard('native-dx11-paints', 'Native D3D11 adapter and Windows acceptance.') + '</div>' +
    extensionCode('sh', `just --justfile apps/xgpui-ext-lab/paint-lab/justfile radial
just --justfile apps/xgpui-ext-lab/paint-lab/justfile --list`) +
    instancedDemoLinks() +
    '<h2>2. Wide-gamut presentation</h2>' +
    extensionCode('rust', `use gpui::WindowColorMode;

// During window construction or an update outside drawing:
window.set_color_mode(WindowColorMode::WideGamutSdr)?;
let status = window.color_status();

// Explicit producer output; legacy paints and controls remain sRGB:
MetalWideSdrOutput::from_linear_p3_premultiplied(context, texture, commands)?;`) +
    '<p><code>xgpui-gamut</code> produces bounded linear-P3 SDR surfaces. GPUI backends own window negotiation and composition; Metal and native D3D11 presentation are implemented. Windows native execution/display validation remains outstanding; Linux is deferred.</p>' +
    '<div class="cards">' + extensionCard('high-gamut-windows', 'Start here: window setup, complete Metal producer, color contract, composition and validation.') + '</div>' +
    extensionCode('sh', `just --justfile apps/xgpui-ext-lab/gamut-lab/justfile ui
just --justfile apps/xgpui-ext-lab/gamut-lab/justfile test`) +
    colorDemoLinks() +
    '<h2>3. Parley text</h2><p>Shared text backend with flat-LTR compatibility, BiDi paragraph layout and editing/IME geometry. Existing GPUI caches and glyph atlas; Windows/Linux runtime validation remains outstanding.</p>' +
    '<div class="cards">' + extensionCard('parley-text', 'Construction, paragraph/input seams, cache and face identity, supported subset and validation.') + '</div>' +
    '<h2>4. Progressive SVG</h2><p>Experimental full-color static SVG elements with preview/detail refinement, shared FIFO admission and aggregate image accounting. Native lab and attributed offline samples; smaller budgets reduce deep-zoom detail.</p>' +
    '<div class="cards">' + extensionCard('svg-rendering', 'Use SvgImage and SvgScheduler, understand accounting limits, and run the native sample.') + '</div>' +
    '<h2>5. Standalone color math</h2><p><code>xgpui-color</code> provides color conversions, gamut mapping, OkLab/OkLCh interpolation and reusable WGSL helpers with zero runtime dependencies in its default configuration. Optional ICC parsing adds a parser dependency. Use it without GPUI or a GPU context.</p>' +
    '<div class="cards">' + extensionCard('color-math', 'CPU types, browser runtime diagnostics, native P3 comparison, dependency boundaries and validation.') + '</div>' +
    '<h2>6. Dedicated media images</h2><p>Display large images at several sizes, adjust their colors, or show frames from a native producer. Start with small examples and the browser demos.</p><div class="cards">' + extensionCard('media-images', 'Examples, loading limits, platform support and test results.') + '</div>' +
    '<h2>7. Spatial canvas</h2><div class="cards">' + extensionCard('canvas', 'Native lab, camera and scene API, controls and known limits. Explore the card canvas and CPU Mandelbrot browser companions.') + '</div>' +
    '<h2>Shared GPUI integration</h2><div class="cards">' + extensionCard('gpui-public-api', 'Public additions and altered upstream types, separated by capability.') + '</div>';

}
function extensionCard(route, description) {
  return '<a class="card" href="#' + route + '"><h3>' + escapeHtml(extensionTitle(route)) + ' →</h3><p>' + escapeHtml(description) + '</p></a>';
}

function publicApiSourceLinks() {
  const sites = [
    ['gpui', 'Application::with_platform_and_text_system', 'app.rs'],
    ['gpui', 'PlatformTextSystem', 'platform.rs'],
    ['gpui', 'WindowTextSystem::layout_paragraph', 'text_system.rs'],
    ['gpui', 'PlatformParagraphShape', 'text_system/paragraph.rs'],
    ['gpui', 'PlatformParagraphLayout', 'text_system/paragraph.rs'],
    ['gpui', 'ParagraphRequest', 'text_system/paragraph.rs'],
    ['gpui', 'ParagraphPosition', 'text_system/paragraph.rs'],
    ['gpui', 'paint_paragraph', 'text_system/paragraph.rs'],
    ['gpui', 'ParsedSvg::size', 'svg_renderer.rs'],
    ['gpui', 'Scene', 'scene.rs'],
    ['gpui', 'Primitive', 'scene.rs'],
    ['gpui', 'PrimitiveBatch', 'scene.rs'],
    ['gpui', 'CustomContent', 'scene.rs'],
    ['gpui', 'PreparationRequest', 'scene.rs'],
    ['gpui', 'CustomGeometry', 'scene.rs'],
    ['gpui', 'CustomCoverage', 'scene.rs'],
    ['gpui', 'CustomDraw', 'scene.rs'],
    ['gpui', 'Window::paint_custom', 'window.rs'],
    ['gpui', 'Window::set_color_mode', 'window.rs'],
    ['gpui', 'Window::color_status', 'window.rs'],
    ['gpui', 'WindowColorMode', 'platform/color_presentation.rs'],
    ['gpui', 'WindowColorStatus', 'platform/color_presentation.rs'],
    ['gpui-apple', 'MetalWideSdrOutput', 'custom_draw.rs'],
    ['gpui-apple', 'MetalRenderer::set_color_mode', 'metal_renderer.rs'],
    ['gpui-apple', 'MetalCustomContext', 'custom_draw.rs'],
    ['gpui-apple', 'MetalCustomOutput', 'custom_draw.rs'],
    ['gpui-wgpu', 'WgpuCustomContext', 'custom_draw.rs'],
    ['gpui-wgpu', 'WgpuCustomOutput', 'custom_draw.rs'],
    ['gpui-windows', 'Dx11CustomContext', 'custom_draw.rs'],
    ['gpui-windows', 'Dx11CustomOutput', 'custom_draw.rs'],
    ['gpui-windows', 'Dx11WideSdrOutput', 'custom_draw/wide_output.rs'],
    ['gpui-windows', 'Dx11HeadlessRenderer', 'directx_renderer.rs'],
  ];
  return '<h2>Jump to the current declarations</h2><ul>' + sites.map(([pkg, name, file]) => {
    const path = 'gpui/crates/' + pkg + '/src/' + file;
    const declaration = crates.find(c => c.package === pkg).declarations.find(d => d.name === name && d.file === path);
    return '<li>' + sourceLink(path, declaration.line, pkg + '::' + name) + ' <code>' + escapeHtml(file) + '</code></li>';
  }).join('') + '</ul>';
}

function typographyCallout() {
  return '<section class="demo-entry" aria-label="Selectable kinetic typography"><div class="demo-entry-heading"><span>SELECTABLE KINETIC TYPOGRAPHY</span><a href="#typography">Read the typography guide →</a></div>' +
    '<h2>Animated glyphs. Authentic text.</h2><p>Typewriter reveal, wave motion and deterministic scramble with gradient fills, glow and drop shadows. Selection and copy preserve the original text; native accessibility exposes the complete source.</p>' +
    browserDemoLinks('typography') +
    '<p class="muted">Read-only, single-line left-to-right text. The browser platform has no canvas accessibility bridge yet; use the native demo for accessibility review.</p></section>';
}
function shaderCrateGuide() {
  return typographyCallout() + brushDemoLinks() + instancedDemoLinks() + browserDemoLinks('shader') + '<article class="extension-document">' +
    data.extension_documents['extension-paints'].html.replace(/^<h1[^>]*>.*?<\/h1>\s*/, '') +
    '</article><h2>Producer and backend references</h2><div class="cards">' +
    extensionCard('typography', 'Gradient fills, glyph glow/shadows, typewriter, wave and scramble with stable selection and native accessibility.') + extensionCard('brushes', 'Universal brushes, glyphs, paths, mesh gradients and cached layer effects; pastel waves demo.') + extensionCard('shader-producers', 'Mesh, gradient parameters and caches.') +
    extensionCard('portable-wgsl-v1', 'Write a portable WGSL producer.') +
    extensionCard('instanced-wgsl-v2', 'Typed direct main-pass shaders and native-first examples.') +
    extensionCard('paint-contract-hardening', 'Preparation and recording transactions.') +
    extensionCard('portable-paints', 'wgpu backend.') +
    extensionCard('native-dx11-paints', 'Native Windows D3D11 backend.') + '</div>';
}

function gamutCrateGuide() {
  return gamutDemoPanel() + '<article class="extension-document">' +
    data.extension_documents['high-gamut-windows'].html.replace(/^<h1[^>]*>.*?<\/h1>\s*/, '') + '</article>';
}

function parleyCrateGuide() {
  return browserDemoLinks('parley') + '<article class="extension-document">' +
    data.extension_documents['parley-text'].html.replace(/^<h1[^>]*>.*?<\/h1>\s*/, '') + '</article>';
}

function svgCrateGuide() {
  return browserDemoLinks('svg') + '<article class="extension-document">' +
    data.extension_documents['svg-rendering'].html.replace(/^<h1[^>]*>.*?<\/h1>\s*/, '') + '</article>';
}

function colorCrateGuide() {
  return '<p><a href="#color-workflows">Choose a color task →</a> · <a href="#color-labs">Demo walkthroughs and limits →</a></p>' + browserDemoLinks('pixel-metadata') + browserDemoLinks('color') + '<article class="extension-document">' +
    data.extension_documents['color-math'].html.replace(/^<h1[^>]*>.*?<\/h1>\s*/, '') + '</article>';
}

function mediaCrateGuide() {
  return '<article class="extension-document">' +
    data.extension_documents['media-images'].html.replace(/^<h1[^>]*>.*?<\/h1>\s*/, '') + '</article>';
}

function canvasCrateGuide() {
  return browserDemoLinks('canvas') + browserDemoLinks('mandelbrot') + '<article class="extension-document">' + data.extension_documents['canvas'].html.replace(/^<h1[^>]*>.*?<\/h1>\s*/, '') + '</article>';
}
