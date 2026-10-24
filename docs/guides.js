'use strict';

function guideSource(file, line, label, pkg = 'gpui') {
  return sourceLink('gpui/crates/' + pkg + '/src/' + file, line, label);
}
function guideSection(title, body, evidence) {
  return '<section class="guide-section"><h2>' + title + '</h2>' + body + '<p class="evidence"><strong>Source trail:</strong> ' + evidence + '</p></section>';
}
function architecturePage() {
  const s = guideSource;
  return header('Architecture / captured source', 'The architecture of GPUI', 'GPUI coordinates persistent Rust application state, temporary element trees, a frame scene and native platform services. These layers meet through explicit context, element and platform contracts.') +
    guideSection('Package boundaries',
      '<p>The core <code>gpui</code> crate owns entities, contexts, elements, input, text coordination and scene construction. <code>gpui-platform</code> selects the platform; native packages implement windowing and OS services. Metal lives in <code>gpui-apple</code>, wgpu in <code>gpui-wgpu</code>, and DirectX in the Windows implementation. Supporting crates provide scheduling, data structures, macros, transport and instrumentation. Manifest dependencies describe package coupling, not runtime stages or API stability.</p>',
      s('gpui.rs',34,'Core module structure') + ' · ' + s('gpui_platform.rs',1,'Platform facade','gpui-platform')) +
    guideSection('Runtime ownership',
      '<p><code>Application::run</code> delegates lifecycle control to <code>Platform::run</code>. <code>App</code> coordinates entities, windows, subscriptions and effects. Each <code>Window</code> owns its root view, layout engine, focus/dispatch state, rendered/next frames and <code>PlatformWindow</code>. The latter supplies native geometry, callbacks, text input, frame scheduling, atlas access and scene submission. <code>Context&lt;T&gt;</code> pairs mutable App access with a weak handle to the current entity.</p>',
      s('app.rs',249,'Application::run') + ' · ' + s('app.rs',1500,'App::open_window') + ' · ' + s('app/context.rs',21,'Context fields') + ' · ' + s('platform.rs',1193,'PlatformWindow')) +
    guideSection('Entities and invalidation',
      '<p><code>Entity&lt;T&gt;</code> is a strong handle to managed state; clones share the value. Reads require an App context; updates use a closure with the value and typed Context. <code>WeakEntity&lt;T&gt;</code> does not retain the value, so async re-entry can fail after destruction. Entities become views by implementing <code>Render</code>; shared models need not render. <code>notify</code> explicitly invalidates tracked views; ordinary mutation does not imply notification. Observers and typed event subscriptions require retained <code>Subscription</code> ownership.</p>',
      s('app/entity_map.rs',435,'Entity and read/update methods') + ' · ' + s('app/entity_map.rs',765,'WeakEntity') + ' · ' + s('element.rs',164,'Render contract')) +
    guideSection('Views and elements',
      '<p><code>Render::render</code> produces <code>IntoElement</code> from mutable view state, Window and Context. <code>RenderOnce</code> consumes a component without requiring a separate entity. <code>Element</code> carries associated state through <code>request_layout</code>, <code>prepaint</code> and <code>paint</code>. EntityId identifies persistent state; ElementId and its enclosing path preserve element state across frames. Cached views can reuse paint ranges.</p>',
      s('element.rs',49,'Element lifecycle') + ' · ' + s('element.rs',145,'IntoElement / Render / RenderOnce') + ' · ' + s('view.rs',591,'Cached paint reuse')) +
    guideSection('Layout, input and accessibility',
      '<p>Taffy-backed layout computes bounds from styles and constraints. Prepaint commits geometry and hitboxes; paint records output. Focus, keyboard/action dispatch, mouse hit testing and accessibility must agree with the rendered frame. Custom elements must implement the required accessibility identity, role and properties, and preserve text-input and event-propagation contracts.</p>',
      s('window.rs',3669,'Window::draw_roots') + ' · ' + s('element.rs',107,'Accessibility hooks') + ' · ' + s('window.rs',5593,'Input dispatch')) +
    guideSection('Scene and GPU contracts',
      '<p><code>Scene</code> records quads, shadows, paths, underlines, sprites, surfaces and downstream custom draws with ordering/layer metadata. Window finalizes and swaps frames, then presents through <code>PlatformWindow::draw</code>. A primitive change can affect Metal, wgpu and DirectX, including CPU layouts, instance-buffer uploads and shader ABI. GPU-facing padding must be initialized; this capture uses a u32-backed padded boolean.</p>',
      s('scene.rs',23,'GPU-facing layout and Scene') + ' · ' + s('window.rs',3604,'Scene presentation') + ' · ' + s('metal_renderer.rs',492,'Metal draw','gpui-apple') + ' · ' + s('wgpu_renderer.rs',1117,'wgpu draw','gpui-wgpu')) +
    guideSection('Text and async services',
      '<p>The shared text system shapes text and decoration runs; <code>PlatformTextSystem</code> supplies fonts, metrics, line layout and glyph rasterization. Measurement affects layout; glyphs use atlas-backed drawing. Window installs the focused platform input handler during frame processing. Foreground futures run locally on the main thread; background futures and outputs require Send. <code>Context::spawn</code> provides WeakEntity and AsyncApp for re-entry after await. Retain or detach the Task and reject stale results at the application boundary.</p>',
      s('text_system.rs',638,'Text shaping') + ' · ' + s('platform.rs',1494,'PlatformTextSystem') + ' · ' + s('executor.rs',103,'Background spawn') + ' · ' + s('executor.rs',354,'Foreground spawn') + ' · ' + s('app/context.rs',230,'Entity-scoped async work')) +
    guideSection('Extension boundaries',
      '<p>Custom elements integrate layout and existing paint operations. New scene primitives change the shared renderer contract; backend shaders add resource-lifetime and synchronization requirements. Downstream <a href="#extensions">shader, gamut and Parley extensions</a> document their contracts separately; see <a href="#gpui-public-api">core API changes</a> for integration points. Trace source color interpretation, intermediate textures, blending, output format and native window/display configuration together. Shared Apple code supports macOS/iOS cfgs, but the platform selector has no iOS construction branch.</p>',
      s('metal_renderer.rs',179,'Apple layer pixel format','gpui-apple') + ' · ' + s('wgpu_context.rs',11,'Exposed wgpu context','gpui-wgpu') + ' · ' + s('gpui_platform.rs',80,'Platform selection','gpui-platform'));
}
