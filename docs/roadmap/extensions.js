'use strict';

function isExtensionRoute(route) {
  return route === 'extensions' || Object.hasOwn(data.extension_documents, route);
}
function extensionTitle(route) {
  return route === 'extensions' ? 'Extensions to GPUI' : data.extension_documents[route].title;
}
function extensionCode(language, source) {
  return '<pre><code class="language-' + language + '">' + escapeHtml(source) + '</code></pre>';
}
function extensionPage(route) {
  const banner = '<p class="guide-intro"><strong>Downstream extension to GPUI.</strong> Opt in through <code>xgpui-shader</code>. High-gamut window support is separate.</p>';
  if (route !== 'extensions') {
    const doc = data.extension_documents[route];
    return '<p><a href="#extensions">← Extensions to GPUI</a></p>' + banner + '<article class="extension-document">' + doc.html + (route === 'gpui-public-api' ? publicApiSourceLinks() : '') + '</article><p class="evidence">Source: <code>' + escapeHtml(doc.source) + '</code></p>';
  }
  return header('Downstream / opt-in', 'Extensions to GPUI', 'Mesh, gradients and portable shaders through a separate extension crate.') + banner +
    '<div class="route"><h3><a href="#gpui-public-api">GPUI public API changes →</a></h3><p>Added APIs, altered scene types and source compatibility risks, shown as code.</p></div>' +
    '<h2>1. Add the crate</h2>' + extensionCode('toml', `[dependencies]
xgpui-shader = { path = "../../extensions/xgpui-ext/crates/xgpui-shader" }
# Add features = ["portable-shaders"] for user WGSL.`) +
    '<h2>2. Build a paint</h2>' + extensionCode('rust', `use gpui::{prelude::*, *};
use std::sync::Arc;
use xgpui_shader::{painted, Gradient, GradientStop, WindowPaintExt};

let paint = Arc::new(Gradient::radial(
    [0.5, 0.5], [0.5, 0.5],
    vec![
        GradientStop::new(0.0, [1.0, 0.2, 0.3, 1.0]),
        GradientStop::new(1.0, [0.1, 0.3, 1.0, 1.0]),
    ],
)?);`) +
    '<h2>3. Use it across paint sites</h2>' + extensionCode('rust', `painted()
    .w(px(320.0)).h(px(240.0))
    .rounded_lg().border_8()
    .bg_paint(paint.clone())
    .border_paint(paint.clone())
    .child(div().p_4().child("Native GPUI children"));

// Inside a paint callback, using an existing Path<Pixels>:
window.paint_path_with_paint(path, paint.clone());

// Native styling stays available:
div().bg(rgb(0x223344)).border_1().border_color(white());`) +
    '<p>Rebuild the container to change materials. Custom materials do not enter native hover/theme refinements.</p>' +
    '<h2>4. Run it</h2>' + extensionCode('sh', `just --justfile apps/paint-lab/justfile radial
just --justfile apps/paint-lab/justfile mesh
just --justfile apps/paint-lab/justfile portable-test`) +
    '<h2>API and integration</h2><div class="cards">' + extensionCard('extension-paints', 'Gradients, meshes, borders, paths and the core hook.') + extensionCard('paint-contract-hardening', 'Producer code, preparation, recording and failure behavior.') + '</div>' +
    '<h2>Extension notes</h2><div class="cards">' + extensionCard('portable-wgsl-v1', 'Rust parameters + WGSL source.') + extensionCard('portable-paints', 'wgpu support and validation.') + extensionCard('native-dx11-paints', 'D3D11 adapter and Windows acceptance.') + '</div>';
}
function extensionCard(route, description) {
  return '<a class="card" href="#' + route + '"><h3>' + escapeHtml(extensionTitle(route)) + ' →</h3><p>' + escapeHtml(description) + '</p></a>';
}

function publicApiSourceLinks() {
  const sites = [
    ['gpui', 'Scene', 'scene.rs'],
    ['gpui', 'Primitive', 'scene.rs'],
    ['gpui', 'PrimitiveBatch', 'scene.rs'],
    ['gpui', 'CustomContent', 'scene.rs'],
    ['gpui', 'PreparationRequest', 'scene.rs'],
    ['gpui', 'CustomGeometry', 'scene.rs'],
    ['gpui', 'CustomCoverage', 'scene.rs'],
    ['gpui', 'CustomDraw', 'scene.rs'],
    ['gpui', 'Window::paint_custom', 'window.rs'],
    ['gpui-apple', 'MetalCustomContext', 'custom_draw.rs'],
    ['gpui-apple', 'MetalCustomOutput', 'custom_draw.rs'],
    ['gpui-wgpu', 'WgpuCustomContext', 'custom_draw.rs'],
    ['gpui-wgpu', 'WgpuCustomOutput', 'custom_draw.rs'],
    ['gpui-windows', 'Dx11CustomContext', 'custom_draw.rs'],
    ['gpui-windows', 'Dx11CustomOutput', 'custom_draw.rs'],
    ['gpui-windows', 'Dx11HeadlessRenderer', 'directx_renderer.rs'],
  ];
  return '<h2>Jump to the current declarations</h2><ul>' + sites.map(([pkg, name, file]) => {
    const path = 'gpui/crates/' + pkg + '/src/' + file;
    const declaration = crates.find(c => c.package === pkg).declarations.find(d => d.name === name && d.file === path);
    return '<li>' + sourceLink(path, declaration.line, pkg + '::' + name) + ' <code>' + escapeHtml(file) + '</code></li>';
  }).join('') + '</ul>';
}
