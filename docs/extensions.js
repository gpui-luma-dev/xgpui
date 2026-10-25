'use strict';

const shaderGuideRoutes = ['extension-paints', 'shader-producers', 'paint-contract-hardening', 'portable-wgsl-v1', 'instanced-wgsl-v2', 'portable-paints', 'native-dx11-paints'];

function isExtensionRoute(route) {
  return route === 'extensions' || Object.hasOwn(data.extension_documents, route);
}
function extensionTitle(route) {
  return route === 'extensions' ? 'Extensions to GPUI' : data.extension_documents[route].title;
}
function browserDemoLinks(demo) {
  return '<p><a href="examples/extensions/?demo=' + demo + '">Run browser demo →</a> · <a href="https://github.com/gpui-luma-dev/xgpui/tree/main/apps/extensions-web">Demo source</a></p>';
}
function instancedDemoLinks() {
  return '<h2>Instanced WGSL v2 · native app first</h2>' +
    '<p>Typed fragment instances, ordered batching, configurable gradient shadows, soft animated borders and direct main-pass rendering. Experimental SDR subset; Windows runtime acceptance remains outstanding.</p>' +
    extensionCode('sh', 'just --justfile apps/shader-examples/justfile ui\njust --justfile apps/shader-examples/justfile composition') +
    '<p><a href="#instanced-wgsl-v2">API, examples and implementation status →</a> · <a href="examples/shaders/">WASM companion →</a> · <a href="https://github.com/gpui-luma-dev/xgpui/tree/main/apps/shader-examples">Demo source</a></p>';
}
function extensionCode(language, source) {
  return '<pre><code class="language-' + language + '">' + escapeHtml(source) + '</code></pre>';
}
function extensionPage(route) {
  if (route === 'high-gamut-windows') {
    const doc = data.extension_documents[route];
    return header('Extensions', 'xgpui-gamut', 'Explicit wide SDR content alongside normal sRGB controls.') +
      '<p class="muted">' + crateLink('xgpui-gamut') + ' · Backend owners: ' + crateLink('gpui-apple') + ' / ' + crateLink('gpui-windows') + '.</p>' +
      '<nav class="tabs" aria-label="Wide-gamut sections"><a href="#high-gamut-windows" aria-current="page">Use &amp; contract</a><a href="#high-gamut-windows&section=typed-windows-output">Windows output</a><a href="#gpui-public-api">GPUI API changes</a></nav>' +
      '<article class="extension-document">' + doc.html.replace(/^<h1[^>]*>.*?<\/h1>\s*/, '') + '</article>';
  }

  const banner = route === 'color-math'
    ? '<p class="guide-intro"><strong>Standalone color math.</strong> CPU conversions, gamut mapping, gradients and WGSL source without GPUI.</p>'
    : route === 'svg-rendering'
    ? '<p class="guide-intro"><strong>Experimental SVG extension.</strong> Progressive rasterization, shared admission and accounted image budgets.</p>'
    : route === 'parley-text'
    ? '<p class="guide-intro"><strong>Text backend extension.</strong> Parley shaping and geometry; GPUI caches, atlas and native input.</p>'
    : route === 'high-gamut-windows'
    ? '<p class="guide-intro"><strong>Wide-gamut presentation extension.</strong> Native xgpui-gamut producers; per-window Metal presentation.</p>'
    : route === 'extensions' || route === 'gpui-public-api'
      ? '<p class="guide-intro"><strong>Downstream extensions to GPUI.</strong> Paints, window color presentation, text backends and SVG rendering are independent opt-in capabilities.</p>'
      : '<p class="guide-intro"><strong>Paint and shader extension.</strong> Opt in through <code>xgpui-shader</code>; ordinary output is sRGB, with explicit native wide-SDR programs.</p>';
  if (route !== 'extensions') {
    const doc = data.extension_documents[route];
    return '<p><a href="#extensions">← Extensions to GPUI</a></p>' + banner + (route === 'color-math' ? browserDemoLinks('color') : route === 'svg-rendering' ? browserDemoLinks('svg') : route === 'instanced-wgsl-v2' ? instancedDemoLinks() : '') + '<article class="extension-document">' + doc.html + (route === 'gpui-public-api' ? publicApiSourceLinks() : '') + '</article><p class="evidence">Source: <code>' + escapeHtml(doc.source) + '</code></p>';
  }
  return header('Downstream / opt-in', 'Extensions to GPUI', 'Independent paint, presentation, text and SVG extensions.') + banner +
    '<p><a href="examples/extensions/">Browser extension demos →</a> · <a href="https://github.com/gpui-luma-dev/xgpui/tree/main/apps/extensions-web">Demo source</a></p>' +
    '<h2>1. Paints and shaders</h2>' +
    extensionCode('rust', `use xgpui_shader::{painted, SolidColor};
use std::sync::Arc;

painted().bg_paint(Arc::new(SolidColor::new([1.0, 0.2, 0.0, 1.0])?));`) +
    '<p><code>xgpui-shader</code>: gradients, meshes and portable WGSL. Metal, wgpu and native D3D11 adapters; ordinary programs use sRGB. Explicit linear-P3 programs require a native wide-gamut window.</p>' +
    '<div class="cards">' + extensionCard('extension-paints', 'Start here: dependencies, containers, borders, paths and interaction.') + extensionCard('shader-producers', 'Mesh sources, parameters, limits and caches.') + extensionCard('paint-contract-hardening', 'Preparation, recording and failure behavior.') + extensionCard('portable-wgsl-v1', 'Rust parameters + portable WGSL source.') + extensionCard('instanced-wgsl-v2', 'Typed main-pass instances, ordered batching, native app and WASM companion.') + extensionCard('portable-paints', 'wgpu support and validation.') + extensionCard('native-dx11-paints', 'Native D3D11 adapter and Windows acceptance.') + '</div>' +
    extensionCode('sh', `just --justfile apps/paint-lab/justfile radial
just --justfile apps/paint-lab/justfile --list`) +
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
    extensionCode('sh', `just --justfile apps/gamut-lab/justfile ui
just --justfile apps/gamut-lab/justfile test`) +
    '<h2>3. Parley text</h2><p>Shared text backend with flat-LTR compatibility, BiDi paragraph layout and editing/IME geometry. Existing GPUI caches and glyph atlas; Windows/Linux runtime validation remains outstanding.</p>' +
    '<div class="cards">' + extensionCard('parley-text', 'Construction, paragraph/input seams, cache and face identity, supported subset and validation.') + '</div>' +
    '<h2>4. Progressive SVG</h2><p>Experimental full-color static SVG elements with preview/detail refinement, shared FIFO admission and aggregate image accounting. Native lab and attributed offline samples; smaller budgets reduce deep-zoom detail.</p>' +
    '<div class="cards">' + extensionCard('svg-rendering', 'Use SvgImage and SvgScheduler, understand accounting limits, and run the native sample.') + '</div>' +
    '<h2>5. Standalone color math</h2><p><code>xgpui-color</code> provides color conversions, gamut mapping, OkLab/OkLCh interpolation and reusable WGSL helpers with zero runtime dependencies. Use it without GPUI or a GPU context.</p>' +
    '<div class="cards">' + extensionCard('color-math', 'CPU types, usage, dependency boundaries and validation.') + '</div>' +
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

function shaderCrateGuide() {
  return instancedDemoLinks() + browserDemoLinks('shader') + '<article class="extension-document">' +
    data.extension_documents['extension-paints'].html.replace(/^<h1[^>]*>.*?<\/h1>\s*/, '') +
    '</article><h2>Producer and backend references</h2><div class="cards">' +
    extensionCard('shader-producers', 'Mesh, gradient parameters and caches.') +
    extensionCard('portable-wgsl-v1', 'Write a portable WGSL producer.') +
    extensionCard('instanced-wgsl-v2', 'Typed direct main-pass shaders and native-first examples.') +
    extensionCard('paint-contract-hardening', 'Preparation and recording transactions.') +
    extensionCard('portable-paints', 'wgpu backend.') +
    extensionCard('native-dx11-paints', 'Native Windows D3D11 backend.') + '</div>';
}

function gamutCrateGuide() {
  return '<article class="extension-document">' +
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
  return browserDemoLinks('color') + '<article class="extension-document">' +
    data.extension_documents['color-math'].html.replace(/^<h1[^>]*>.*?<\/h1>\s*/, '') + '</article>';
}
