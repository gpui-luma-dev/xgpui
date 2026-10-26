import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import vm from 'node:vm';

const site = new URL('./', import.meta.url);
const source = await readFile(new URL('app.js', site), 'utf8');
const inventory = await readFile(new URL('data.js', site), 'utf8');
const context = vm.createContext({ window: {}, URLSearchParams });
vm.runInContext(inventory, context);
vm.runInContext(await readFile(new URL("guides.js", site), "utf8"), context);
vm.runInContext(await readFile(new URL("pipeline.js", site), "utf8"), context);
vm.runInContext(await readFile(new URL("extensions.js", site), "utf8"), context);
vm.runInContext(source, context);
const run = code => vm.runInContext(code, context);
const data = context.window.ROADMAP;
const indexedPackages = {...data.provenance.packages, ...data.extension_packages};
assert.equal(data.crates.length, Object.keys(indexedPackages).length);
assert.equal(data.extension_packages['xgpui-shader'], 'extensions/xgpui-ext/crates/xgpui-shader');
assert.ok(!data.provenance.packages['xgpui-shader'], 'local extension must not claim upstream provenance');
assert.equal(Object.keys(data.notes).sort().join(), data.crates.map(c => c.package).sort().join());
assert.equal(run('parseRoute(crateUrl("gpui", "surface")).tab'), 'surface');
assert.equal(run('escapeHtml("<script>&")'), "&lt;script&gt;&amp;");
assert.equal(run('filterDeclarations([{name:"Example", signature:"", module:"crate", file:"", guards:[],kind:"struct", root_export:false}], "example").length'), 1);
assert.equal(run('filterDeclarations([{name:"Example", signature:"", module:"crate", file:"", guards:[],kind:"struct", root_export:false}], "", "", true).length'), 0);
assert.match(run('overview()'), /GPUI architecture and extensions/);
assert.doesNotMatch(run('overview()'), /Inventory limits|compiler-resolved public API report|#method/);
assert.equal(run('typeof method'), 'undefined');
assert.ok(data.crates.every(c => c.warnings.every(w => !w.includes('/Users/') && !w.includes('/private/'))));
const gpuiInventory = data.crates.find(c => c.package === 'gpui');
assert.ok(gpuiInventory.files.includes('gpui/crates/gpui/src/text_system/paragraph_cache.rs'));
assert.ok(!gpuiInventory.warnings.some(w => w.includes('paragraph_cache')));
assert.ok(data.crates.find(c => c.package === 'gpui-windows').warnings.some(w => w.includes('Unexpanded include! at gpui/crates/gpui-windows/')));

assert.match(run('review()'), /<h1>API review<\/h1>/);
assert.doesNotMatch(run('review()'), /Make the contract deliberate|Suggested order of work/);
const pages = new Map();
for (const c of data.crates) {
  assert.ok(data.notes[c.package].role);
  assert.ok(c.files.includes(c.entry));
  for (const tab of ['overview', 'surface', 'dependencies', 'review']) {
    context.testPackage = c.package;
    context.testTab = tab;
    const html = run('cratePage(crates.find(c => c.package === testPackage), testTab)');
    assert.ok(html.includes(c.package));
    assert.ok(!html.includes('undefined'));
  }
  for (const dep of c.dependencies) assert.ok(indexedPackages[dep.package]);
  for (const file of c.files) {
    assert.ok(!file.includes('..'));
    if (!pages.has(file)) pages.set(file, await readFile(new URL('source/' + file + '.html', site), 'utf8'));
  }
  for (const d of c.declarations) {
    assert.ok(Number.isInteger(d.line) && d.line > 0);
    assert.ok(pages.get(d.file).includes('id="L' + d.line + '"'), d.file + ':' + d.line);
  }
}
// Exercise route rendering and event callbacks without opening a browser.
const elements = new Map();
const element = id => {
  if (!elements.has(id)) elements.set(id, {innerHTML:'', value:'', checked:false, setAttribute(name,value) {this[name]=value;}, addEventListener(type, callback) {this[type] = callback;}});
  return elements.get(id);
};
context.document = {getElementById:element, title:''};
context.location = {hash:''};
context.window.addEventListener = () => {};
context.window.scrollTo = () => {};
run('render()');
assert.match(element('content').innerHTML, /GPUI architecture and extensions/);
for (const c of data.crates) {
  for (const tab of ['overview', 'surface', 'dependencies', 'review']) {
    context.location.hash = '#crate=' + c.package + '&tab=' + tab;
    run('render()');
    if (tab === 'surface') {
      element('api-search').value = 'no-such-symbol-93f3';
      element('api-search').input();
      assert.match(element('api-results').innerHTML, /No declarations match/);
      element('api-search').value = '';
      element('api-search').input();
    }
  }
}
for (const route of ['#extensions', ...Object.keys(data.extension_documents).map(route => '#' + route), '#pipeline', '#architecture', '#review', '#crate=unknown']) {
  context.location.hash = route;
  run('render()');
  assert.ok(element('content').innerHTML.length);
}
const index = await readFile(new URL('index.html', site), 'utf8');
assert.ok(!index.includes('#method'));
assert.ok(index.includes('href="examples/extensions/"'));
assert.ok(index.includes('href="https://github.com/gpui-luma-dev/xgpui">GitHub repository ↗</a>'));
assert.ok(index.indexOf('GitHub repository ↗') < index.indexOf('<nav aria-label="Documentation">'));
assert.ok(index.indexOf('>Live demos ↗</a>') < index.indexOf('>Instanced WGSL v2</a>'));
for (const demo of ['shader', 'brushes', 'layers', 'hermite', 'parley', 'svg', 'color', 'typography', 'media-filters', 'mipmaps', 'path-effects']) {
  const html = run(`browserDemoLinks('${demo}')`);
  assert.ok(html.includes(`href="examples/extensions/?demo=${demo}"`));
  assert.ok(html.includes('https://github.com/gpui-luma-dev/xgpui/tree/main/apps/extensions-web'));
  assert.equal(new URL(`examples/extensions/?demo=${demo}`, 'https://example.github.io/xgpui/').pathname, '/xgpui/examples/extensions/');
}

for (const asset of ['favicon.svg', 'favicon.ico', 'style.css', 'extensions.js', 'guides.js', 'pipeline.js', 'app.js', 'data.js']) {
  assert.ok(index.includes(asset));
  await readFile(new URL(asset, site));
}
// Keep the Wasm landing page complete as either gallery gains new demos.
const wasmIndex = await readFile(new URL('../apps/extensions-web/index.html', import.meta.url), 'utf8');
const legacyGallery = await readFile(new URL('../gpui/crates/gpui-web/examples/hello_web/index.html', import.meta.url), 'utf8');
const legacyManifest = await readFile(new URL('../gpui/crates/gpui-web/examples/hello_web/Cargo.toml', import.meta.url), 'utf8');
const listedModules = [...wasmIndex.matchAll(/data-wasm-module="([^"]+)"/g)].map(m => m[1]);
const legacyModules = [...legacyManifest.matchAll(/\[\[bin\]\]\s+name = "([^"]+)"/g)].map(m => m[1]);
assert.deepEqual(listedModules.slice().sort(), legacyModules.sort(), 'every legacy Wasm binary needs an index entry');
for (const [, module] of legacyGallery.matchAll(/data-module="([^"]+)"/g)) assert.ok(listedModules.includes(module), module);
for (const demo of ['shader', 'brushes', 'layers', 'hermite', 'parley', 'svg', 'color', 'typography', 'media-filters', 'mipmaps', 'path-effects']) {
  for (const suffix of ['', '&backend=webgpu', '&backend=webgl']) {
    assert.ok(wasmIndex.includes('href="?demo=' + demo + suffix + '"'), demo + suffix);
  }
}
for (const [, path] of wasmIndex.matchAll(/href="https:\/\/github.com\/gpui-luma-dev\/xgpui\/blob\/main\/([^"]+)"/g)) {
  await readFile(new URL('../' + path, import.meta.url));
}
assert.ok(wasmIndex.includes('These examples are not published on this site.'));
assert.ok(wasmIndex.includes('not interactive hosted demos'));
console.log('Verified complete Wasm demo index, backend links and source targets.');

console.log('Verified ' + data.crates.length + ' crates, ' + pages.size + ' source pages, declaration anchors, routes, filtering and assets.');

assert.ok(!index.includes('http-equiv="refresh"'), 'Pages root must serve the website directly');
// The brand intentionally links to the host root; other local URLs retain the project prefix.
assert.ok(index.includes('<a class="brand" href="/">xgpui</a>'));
const pagesRoot = new URL('https://example.github.io/xgpui/');
for (const match of index.matchAll(/(?:src|href)="([^"#][^"]*)"/g)) {
  const url = new URL(match[1], pagesRoot);
  if (url.origin === pagesRoot.origin && match[1] !== '/') assert.ok(url.pathname.startsWith('/xgpui/'));
}

for (const [fn, title, sections] of [["architecturePage", "The architecture of GPUI", 8]]) {
 const html = run(fn + "()");
 assert.ok(html.includes(title));
 assert.equal((html.match(/class="guide-section"/g) || []).length, sections);
 assert.ok(!html.includes('class="guide-flow"'));
 assert.ok(!html.includes("undefined"));
 for (const match of html.matchAll(/href="source\/([^"#]+)\.html#L(\d+)"/g)) {
  const file = decodeURIComponent(match[1]);
  assert.ok(pages.has(file), file);
  assert.ok(pages.get(file).includes('id="L' + match[2] + '"'), file + ":" + match[2]);
 }
}
console.log("Verified architecture guide and its source references.");

// Source-grounded fixtures for the illustrative scene model.
const basePipeline = run('composePipeline(pipelineState)');
assert.equal(basePipeline.primitives.length, 7);
assert.equal(basePipeline.rejected.length, 1);
assert.equal(basePipeline.batches.length, 6);
assert.equal(basePipeline.primitives.find(p=>p.id==='photo').bounds.w, 205);
assert.equal(basePipeline.primitives.find(p=>p.id==='photo').clipped.w, 170);
assert.equal(run('composePipeline({...pipelineState,atlas:true}).batches.length'), 7);
assert.equal(run('composePipeline({...pipelineState,mask:false}).primitives.length'), 8);
assert.equal(run('composePipeline({...pipelineState,badgeX:232}).primitives.find(p=>p.id==="badge").order'), 2);
assert.equal(basePipeline.primitives.find(p=>p.id==='badge').order, 3);
assert.equal(run('new Set(composePipeline({...pipelineState,layer:true}).primitives.filter(p=>p.layer).map(p=>p.order)).size'), 1);
assert.equal(run('composePipeline({...pipelineState,layer:true}).operations.length'), 9);
assert.equal(run('pipelineOverlap({x:0,y:0,w:10,h:10},{x:10,y:0,w:10,h:10})'), false);
for(const badgeX of [60,190,232]) for(const mask of [true,false]) for(const layer of [true,false]) for(const atlas of [true,false]) {
  context.fixture={badgeX,mask,layer,atlas,stage:4,selected:'photo',backend:'metal'};
  const model=run('composePipeline(fixture)');
  const all=model.batches.flatMap(b=>b.items);
  assert.equal(all.length,model.primitives.length);
  assert.equal(new Set(all.map(p=>p.id)).size,model.primitives.length);
  assert.equal(model.replayed.map(p=>p.id+':'+p.order).join(),model.primitives.map(p=>p.id+':'+p.order).join());
  for(let i=0;i<model.primitives.length;i++) for(let j=i+1;j<model.primitives.length;j++) {
    const a=model.primitives[i], b=model.primitives[j];
    context.pair={a,b};
    if(run('pipelineOverlap(pair.a.clipped,pair.b.clipped)') && !(a.layer && b.layer)) assert.ok(b.order>a.order);
  }
  for(const batch of model.batches) if(batch.kind.endsWith('Sprite')) assert.equal(new Set(batch.items.map(p=>p.texture)).size,1);
}
context.location.hash='#pipeline';
run('render()');
assert.match(element('content').innerHTML,/From elements to pixels/);
assert.match(element('pipeline-preview').innerHTML,/<svg/);
for(let stage=0;stage<8;stage++) {
  element('pipeline-stage-'+stage).onclick();
  assert.equal(run('pipelineState.stage'),stage);
  assert.match(element('pipeline-caption').innerHTML,new RegExp(run('pipelineStages['+stage+'][0]').replace(/[.*+?^${}()|[\]\\]/g,'\\$&')));
}
element('pipeline-layer').checked=true;element('pipeline-layer').onchange();
assert.equal(run('pipelineState.layer'),true);
element('pipeline-overlap').value='232';element('pipeline-overlap').oninput();
assert.equal(run('pipelineState.badgeX'),232);
assert.match(element('pipeline-preview').innerHTML,/<animateTransform/);
context.window.matchMedia=()=>({matches:true});
element('pipeline-overlap').value='190';element('pipeline-overlap').oninput();
assert.ok(!element('pipeline-preview').innerHTML.includes('<animate'));
let tick=null;
context.setInterval=callback=>{tick=callback;return 1;};
context.clearInterval=()=>{tick=null;};
element('pipeline-play').onclick();
assert.equal(run('pipelineState.stage'),0);
for(let i=0;i<7;i++) {assert.ok(tick);tick();}
assert.equal(run('pipelineState.stage'),7);
assert.equal(tick,null);
element('pipeline-play').onclick();
context.location.hash='#overview';run('render()');
assert.equal(tick,null);
context.location.hash='#pipeline';run('render()');
element('pipeline-reset').onclick();
assert.equal(run('pipelineState.stage'),0);
assert.equal(run('pipelineState.layer'),false);
for(const match of run('pipelinePage()').matchAll(/href="source\/([^"#]+)\.html#L(\d+)"/g)) {
 const file=decodeURIComponent(match[1]);
 assert.ok(pages.has(file),file);
 assert.ok(pages.get(file).includes('id="L'+match[2]+'"'),file+':'+match[2]);
}
console.log('Verified scene ordering, clipping, layers, sprite texture boundaries, replay, SVG, reduced motion and playback cleanup.');

// Every extension contract is part of the offline artifact, with working routes
// and section targets; all published sources must stay in the web docs tree.
assert.equal(Object.keys(data.extension_documents).length, 17);
assert.equal(data.extension_packages['xgpui-parley'], 'extensions/xgpui-ext/crates/xgpui-parley');
assert.ok(!data.provenance.packages['xgpui-parley']);
const extensionOverview = run('extensionPage("extensions")');
assert.match(extensionOverview, /1\. Paints and shaders/);
assert.match(extensionOverview, /2\. Wide-gamut presentation/);
assert.match(extensionOverview, /href="#extension-paints"/);
assert.match(extensionOverview, /href="#high-gamut-windows"/);
assert.match(extensionOverview, /href="#parley-text"/);
assert.match(run('cratePage(crates.find(c => c.package === "xgpui-parley"), "overview")'), /layout_paragraph/);
assert.equal(data.extension_packages['xgpui-svg'], 'extensions/xgpui-ext/crates/xgpui-svg');
assert.ok(!data.provenance.packages['xgpui-svg']);
assert.match(extensionOverview, /4\. Progressive SVG/);
assert.match(extensionOverview, /href="#svg-rendering"/);
assert.match(run('extensionPage("svg-rendering")'), /Experimental SVG extension/);
assert.match(run('cratePage(crates.find(c => c.package === "xgpui-svg"), "overview")'), /SvgScheduler/);
const svgDoc = data.extension_documents['svg-rendering'].html;
for (const term of ['svg_image', 'SchedulerLimits', 'Queued', '3 × output RGBA', '2 × RGBA', 'process/GPU-memory ceiling', 'samples', 'attributed', 'SvgSize::Viewport']) assert.ok(svgDoc.includes(term), term);
assert.match(run('cratePage(crates.find(c => c.package === "xgpui-svg"), "overview")'), /Open Progressive SVG/);
const svgCrate = data.crates.find(c => c.package === 'xgpui-svg');
for (const name of ['SvgImage', 'SvgDocument', 'SvgScheduler', 'SchedulerStats', 'RasterPolicy']) assert.ok(svgCrate.declarations.some(d => d.name === name), name);
assert.ok(svgCrate.dependencies.some(d => d.package === 'gpui'));
assert.match(run('extensionPage("parley-text")'), /Text backend extension/);
assert.match(data.extension_documents['high-gamut-windows'].html, /WideColor::linear_p3/);
assert.match(data.extension_documents['high-gamut-windows'].html, /xgpui-gamut/);

assert.match(run('extensionPage("extensions")'), /Downstream extensions to GPUI/);
assert.ok(!index.includes('<a href="#extensions">Extensions to GPUI</a>'), 'extensions belong in the crate group');
for (const [route, doc] of Object.entries(data.extension_documents)) {
  assert.equal(doc.source, route === 'local-development' ? 'docs/LOCAL_DEVELOPMENT.md' : 'docs/extensions/' + route + '.md');
  await readFile(new URL('../' + doc.source, import.meta.url));
  context.location.hash = '#' + route;
  run('render()');
  assert.ok(element('content').innerHTML.includes(route === 'high-gamut-windows' ? doc.html.replace(/^<h1[^>]*>.*?<\/h1>\s*/, '') : doc.html));
  assert.ok(context.document.title.includes(doc.title));
  assert.ok(!/href="(?!https?:)[^"]*\.md"/.test(doc.html), 'unconverted Markdown link: ' + route);
  for (const match of doc.html.matchAll(/href="#([^"&]+)(?:&amp;section=([^"&]+))?"/g)) {
    const target = data.extension_documents[match[1]];
    assert.ok(target, 'missing extension route: ' + match[1]);
    if (match[2]) assert.ok(target.html.includes('id="' + match[2] + '"'), 'missing section: ' + match[2]);
  }
}
assert.ok(data.extension_documents.brushes, 'universal brushes must be published');
assert.match(run('extensionPage("extensions")'), /href="#brushes"/);
assert.match(run('shaderCrateGuide()'), /href="#brushes"/);
assert.match(data.extension_documents.brushes.html, /--pastel-waves/);
assert.match(data.extension_documents.brushes.html, /issues\/35/);
assert.equal(run('parseRoute("#paint-contract-hardening&section=implemented-discardable-recording-transactions").global'), 'paint-contract-hardening');
assert.equal(run('parseRoute("#paint-contract-hardening&section=implemented-discardable-recording-transactions").section'), 'implemented-discardable-recording-transactions');
for (const file of [
  'extensions/xgpui-ext/README.md',
  'extensions/xgpui-ext/crates/xgpui-shader/README.md',
  'apps/paint-lab/README.md',
  'apps/gamut-lab/README.md',
  'extensions/xgpui-ext/crates/xgpui-gamut/README.md',
  'extensions/xgpui-ext/crates/xgpui-svg/README.md',
  'extensions/xgpui-ext/crates/xgpui-parley/README.md',
]) {
  const url = new URL('../' + file, import.meta.url);
  const markdown = await readFile(url, 'utf8');
  for (const [, link] of markdown.matchAll(/\]\(([^)]+)\)/g)) {
    if (/^(?:https?:|#)/.test(link)) continue;
    await readFile(new URL(link.split('#')[0], url));
  }
}
context.location.hash = '#crate=xgpui-shader&tab=overview';
run('render()');
assert.match(element('content').innerHTML, /fn gradient_panel/);
assert.match(element('content').innerHTML, /API inventory/);
assert.match(element('content').innerHTML, /Role &amp; sources|Role & sources/);
let extensionRail = element('crate-list').innerHTML;
assert.ok(extensionRail.indexOf('>Extensions</p>') < extensionRail.indexOf('>Core</p>'));
assert.match(extensionRail, /class="crate-link" aria-current="page" href="#crate=xgpui-shader/);
assert.match(extensionRail, /class="crate-link" href="#crate=xgpui-gamut&amp;tab=overview">xgpui-gamut|class="crate-link" href="#crate=xgpui-gamut&tab=overview">xgpui-gamut/);
run('sidebar("linear-p3")');
assert.match(element('crate-list').innerHTML, />Extensions<.*xgpui-gamut/s);
assert.ok(!element('crate-list').innerHTML.includes('>Core</p>'));
context.location.hash = '#high-gamut-windows';
run('render()');
assert.match(element('crate-list').innerHTML, /aria-current="page" href="#crate=xgpui-gamut/);
assert.match(element('content').innerHTML, /Backend owners:/);
assert.match(element('content').innerHTML, /set_color_mode/);
assert.match(element('content').innerHTML, /Dx11WideSdrOutput/);
context.location.hash = '#crate=xgpui-gamut&tab=overview';
run('render()');
assert.match(element('content').innerHTML, /WideColor::linear_p3/);
assert.match(element('content').innerHTML, /API inventory/);
for (const route of ['#high-gamut-windows', '#crate=xgpui-gamut&tab=overview']) {
  context.location.hash = route;
  run('render()');
  const gamutPage = element('content').innerHTML;
  assert.match(gamutPage, /aria-label="Wide-gamut demos"/);
  assert.match(gamutPage, /cargo run --locked --config \.cargo\/validation\.toml --manifest-path apps\/gamut-lab\/Cargo\.toml/);
  assert.match(gamutPage, /href="examples\/extensions\/\?demo=color"/);
  assert.match(gamutPage, /Rendered as sRGB/);
}
context.location.hash = '#svg-rendering';
run('render()');
assert.match(element('crate-list').innerHTML, /aria-current="page" href="#crate=xgpui-svg/);
assert.match(element('content').innerHTML, /SvgScheduler/);
run('sidebar("aggregate image")');
assert.match(element('crate-list').innerHTML, />Extensions<.*xgpui-svg/s);
context.location.hash = '#crate=xgpui-svg&tab=overview';
run('render()');
assert.match(element('content').innerHTML, /Native sample and validation/);
assert.match(element('content').innerHTML, /API inventory/);
const gamutCrate = data.crates.find(c => c.package === 'xgpui-gamut');
const shaderCrate = data.crates.find(c => c.package === 'xgpui-shader');
assert.ok(!gamutCrate.dependencies.some(d => d.package === 'xgpui-shader' && d.kind !== 'dev' && !d.optional));
assert.ok(gamutCrate.dependencies.some(d => d.package === 'xgpui-color' && d.kind === 'normal'));
assert.ok(gamutCrate.dependencies.some(d => d.package === 'xgpui-shader' && d.kind === 'normal' && d.optional));
assert.ok(gamutCrate.features.instanced.includes('dep:xgpui-shader'));
assert.ok(!shaderCrate.dependencies.some(d => d.package === 'xgpui-gamut'));
console.log('Verified extension crate navigation, individual code-first guides, filtering and source links.');

const apiChanges = data.extension_documents['gpui-public-api'].html;
for (const term of ['Scene', 'custom_draws', 'PrimitiveBatch', 'CustomContent', 'PreparationRequest', 'CustomGeometry', 'CustomCoverage', 'CustomDraw', 'paint_custom', 'MetalCustomContext', 'WgpuCustomContext', 'Dx11CustomContext', 'source compatibility risks', 'WindowColorMode', 'WindowColorStatus', 'MetalWideSdrOutput', 'Dx11WideSdrOutput', 'set_color_mode', 'ParsedSvg::size()', 'xgpui-svg']) assert.ok(apiChanges.includes(term), term);
assert.match(index, /href="#gpui-public-api"/);
assert.match(run('extensionPage("extensions")'), /href="#gpui-public-api"/);
const extensionApiGuides = {
  'xgpui-shader': ['extension-paints', 'altered-existing-public-types--source-compatibility-risks'],
  'xgpui-gamut': ['high-gamut-windows', 'added-opt-in-window-presentation'],
  'xgpui-parley': ['parley-text', 'added-text-backend-and-paragraph-seams'],
  'xgpui-svg': ['svg-rendering', 'added-svg-viewport-size'],
  'xgpui-media': ['media-images', 'added-source-sized-media-textures'],
};
assert.deepEqual(Object.keys(extensionApiGuides).sort(), Object.keys(data.extension_packages).filter(p => p !== 'xgpui-color').sort(), 'every rendering extension needs an API compatibility guide');
for (const [pkg, [route, section]] of Object.entries(extensionApiGuides)) {
  const guide = data.extension_documents[route].html;
  assert.ok(guide.includes('id="gpui-public-api-changes-additions-and-compatibility"'), pkg);
  assert.ok(guide.includes('#gpui-public-api&amp;section=' + section), pkg);
  assert.ok(apiChanges.includes('id="' + section + '"'), pkg);
  assert.ok(run(`cratePage(crates.find(c => c.package === "${pkg}"), "overview")`).includes(guide.replace(/^<h1[^>]*>.*?<\/h1>\s*/, '')), pkg);
  const readme = await readFile(new URL('../' + data.extension_packages[pkg] + '/README.md', import.meta.url), 'utf8');
  assert.ok(readme.includes('## GPUI public API changes: additions and compatibility'), pkg);
  assert.ok(readme.includes('gpui-public-api.md#' + section), pkg);
}
console.log('Verified public API additions and compatibility sections for every extension crate.');

for (const match of run('publicApiSourceLinks()').matchAll(/href="source\/([^"#]+)\.html#L(\d+)"/g)) {
  const file = decodeURIComponent(match[1]);
  assert.ok(pages.has(file), file);
  assert.ok(pages.get(file).includes('id="L' + match[2] + '"'), file + ':' + match[2]);
}
console.log('Verified API-change source links against regenerated declaration anchors.');

// Instanced shaders must be discoverable, rendered offline, and packaged for Pages.
assert.ok(data.extension_documents['instanced-wgsl-v2']);
assert.ok(index.includes('href="#instanced-wgsl-v2"'));
assert.ok(index.includes('href="examples/extensions/"'));
assert.match(run('shaderCrateGuide()'), /just --justfile apps\/shader-examples\/justfile ui/);
assert.match(run('extensionPage("instanced-wgsl-v2")'), /Clipping ON\/OFF/);
assert.match(run('instancedDemoLinks()'), /href="examples\/shaders\/"/);
const shaderIndex = await readFile(new URL('../apps/shader-examples/index.html', import.meta.url), 'utf8');
for (const example of ['minimal', 'glow', 'particles', 'composition', 'shadows', 'borders']) {
  assert.ok(shaderIndex.includes(`href="?example=${example}"`), 'automatic backend link: ' + example);
  for (const backend of ['webgpu', 'webgl']) {
    assert.ok(shaderIndex.includes(`href="?example=${example}&backend=${backend}"`));
  }
}
const pagesWorkflow = await readFile(new URL('../.github/workflows/pages.yml', import.meta.url), 'utf8');
assert.ok(pagesWorkflow.includes('bash apps/shader-examples/build.sh --release'));
const shaderBuild = await readFile(new URL('../apps/shader-examples/build.sh', import.meta.url), 'utf8');
assert.ok(shaderBuild.includes('docs/examples/shaders/'));
console.log('Verified instanced WGSL guide, native-first launch, companion links and Pages packaging.');

// The standalone color crate must have its own guide and no rendering dependencies.
const colorCrate = data.crates.find(c => c.package === 'xgpui-color');
assert.equal(data.extension_packages['xgpui-color'], 'extensions/xgpui-ext/crates/xgpui-color');
assert.ok(!data.provenance.packages['xgpui-color']);
assert.equal(colorCrate.dependencies.length, 0);
assert.ok(!shaderCrate.dependencies.some(d => ['xgpui-color', 'xgpui-gamut'].includes(d.package)));
assert.match(run('cratePage(crates.find(c => c.package === "xgpui-color"), "overview")'), /EncodedRgb/);
assert.match(run('extensionPage("extensions")'), /href="#color-math"/);
context.location.hash = '#color-math';
run('render()');
assert.match(element('content').innerHTML, /zero runtime dependencies/);
assert.match(element('crate-list').innerHTML, /aria-current="page" href="#crate=xgpui-color/);
context.location.hash = '#crate=xgpui-color&tab=overview';
run('render()');
assert.match(element('content').innerHTML, /GPUI API compatibility/);
run('sidebar("standalone")');
assert.match(element('crate-list').innerHTML, />Extensions<.*xgpui-color/s);
console.log('Verified standalone color crate navigation, guide and dependency boundaries.');

assert.match(run('cratePage(crates.find(c => c.package === "xgpui-color"), "overview")'), /examples\/extensions\/\?demo=color/);
assert.match(wasmIndex, /Color \/ Gamut/);

for (const app of ['extensions-web', 'shader-examples']) {
  const html = await readFile(new URL('../apps/' + app + '/index.html', import.meta.url), 'utf8');
  const build = await readFile(new URL('../apps/' + app + '/build.sh', import.meta.url), 'utf8');
  assert.ok(html.includes("from './graphics.mjs'"), app + ': shared startup diagnostics');
  assert.ok(html.includes('canonicalLocalDemoUrl(location.href)'), app + ': correct wildcard origins');
  assert.ok(html.includes('location.replace(canonical)'), app + ': preserve the explicit backend on localhost');
  assert.ok(build.includes('cp apps/web-support/graphics.mjs'), app + ': diagnostics packaged');
  assert.ok(build.includes('cp apps/web-support/gallery.css'), app + ': gallery styling packaged');
}
console.log('Verified automatic backend links and packaged browser graphics recovery.');

const brushLaunch = run('brushDemoLinks()');
assert.match(brushLaunch, /Open brush gallery/);
assert.match(brushLaunch, /Open layer effects/);
assert.match(brushLaunch, /Open Hermite editor/);
assert.equal((brushLaunch.match(/href="examples\/extensions\/\?demo=/g) || []).length, 3);
assert.ok(!brushLaunch.includes('Run browser demo'));
assert.match(run('overview()'), /LIVE BROWSER DEMOS/);
assert.ok(index.includes('>Live demos ↗</a>'));
for (const app of ['extensions-web', 'shader-examples']) {
  const html = await readFile(new URL('../apps/' + app + '/index.html', import.meta.url), 'utf8');
  assert.ok(html.includes('gallery.css'));
  assert.ok(html.includes('gallery-grid'));
  assert.ok(html.includes('demo-preview'));
  assert.ok(html.includes('../extensions/'));
}
console.log('Verified demo discovery, distinct brush/layer launch labels and visual gallery entry points.');

// "All demos" includes both hosted apps, without duplicating launch cards.
const allLaunches = [...wasmIndex.matchAll(/<a href="([^"]+)">Open demo ↗<\/a>/g)].map(m => m[1]);
assert.equal(allLaunches.length, 17);
assert.equal(new Set(allLaunches).size, 17);
for (const example of ['minimal', 'glow', 'particles', 'composition', 'shadows', 'borders']) {
  for (const suffix of ['', '&backend=webgpu', '&backend=webgl']) {
    assert.ok(wasmIndex.includes('href="../shaders/?example=' + example + suffix + '"'));
  }
}
for (const id of ['brushes-paints','layer-effects','shaders-motion','typography','svg-color','media-effects','native-labs']) {
  assert.ok(wasmIndex.includes('id="' + id + '"'));
}
assert.ok(wasmIndex.includes('--pastel-waves'));
assert.ok(wasmIndex.includes('Show launch command'));
console.log('Verified unified index covers all 17 hosted demos and separates native launch commands.');

assert.match(run('extensionPage("typography")'), /Selectable typography/);
assert.match(run('extensionPage("typography")'), /demo=typography/);

// Versioned benchmark observations are evidence assets, never speed thresholds.
for (const file of ['media-upload-metal-m4-max-2026-10-26.json', 'media-upload-wgpu-macos-2026-10-26.json']) {
  const html = data.extension_documents['media-images'].html;
  assert.ok(html.includes(`href="benchmarks/${file}"`));
  const report = JSON.parse(await readFile(new URL(`benchmarks/${file}`, site), 'utf8'));
  assert.equal(report.schema_version, 1);
  assert.equal(report.profile, 'release');
  assert.equal(report.source_count, 6);
  assert.equal(report.waves.length, 3);
  assert.equal(report.cached_frames.length, 120);
}
console.log('Verified packaged benchmark observations and embedded report links.');

const visibleReportFile = 'media-visible-metal-m4-max-2026-10-26.json';
assert.ok(data.extension_documents['media-images'].html.includes(`href="benchmarks/${visibleReportFile}"`));
const visibleReport = JSON.parse(await readFile(new URL(`benchmarks/${visibleReportFile}`, site), 'utf8'));
assert.equal(visibleReport.observations.length, 3);
for (const observation of visibleReport.observations) {
  assert.equal(observation.profile, 'release');
  assert.equal(observation.completed_sources, 12);
  assert.deepEqual(observation.errors, []);
  assert.equal(observation.baseline.raw_ms.length, 180);
  assert.equal(observation.cached.raw_ms.length, 180);
  assert.ok(observation.burst.raw_ms.length > 0);
  for (const phase of ['baseline', 'burst', 'cached']) {
    assert.ok(observation[phase].raw_ms.every(v => Number.isFinite(v) && v >= 0));
    assert.equal(observation[phase].max_ms, Math.max(...observation[phase].raw_ms));
  }
}
console.log('Verified visible-window observations and embedded report link.');

const internalReportFile = 'media-visible-internal-m4-max-2026-10-26.json';
assert.ok(data.extension_documents['media-images'].html.includes(`href="benchmarks/${internalReportFile}"`));
const internalReport = JSON.parse(await readFile(new URL(`benchmarks/${internalReportFile}`, site), 'utf8'));
assert.equal(internalReport.observations.length, 3);
for (const observation of internalReport.observations) {
  assert.equal(observation.profile, 'release');
  assert.equal(observation.completed_sources, 12);
  assert.deepEqual(observation.errors, []);
  assert.equal(observation.baseline.raw_ms.length, 180);
  assert.equal(observation.cached.raw_ms.length, 180);
  for (const phase of ['baseline', 'burst', 'cached']) {
    assert.ok(observation[phase].raw_ms.length > 0);
    assert.ok(observation[phase].raw_ms.every(v => Number.isFinite(v) && v >= 0));
    assert.equal(observation[phase].max_ms, Math.max(...observation[phase].raw_ms));
  }
}
console.log('Verified internal-display observations and embedded report link.');

const scrollReportFile = 'media-scroll-60hz-m4-max-2026-10-26.json';
assert.ok(data.extension_documents['media-images'].html.includes(`href="benchmarks/${scrollReportFile}"`));
const scrollReport = JSON.parse(await readFile(new URL(`benchmarks/${scrollReportFile}`, site), 'utf8'));
assert.equal(scrollReport.observations.length, 3);
for (const observation of scrollReport.observations) {
  assert.equal(observation.profile, 'release');
  assert.equal(observation.target_hz, 60);
  assert.deepEqual(observation.source_size, [3072, 2048]);
  assert.equal(observation.source_payload_bytes, 33554428);
  assert.equal(observation.replacements_completed, 84);
  assert.deepEqual(observation.errors, []);
  for (const phase of ['baseline', 'loading', 'cached']) {
    assert.ok(observation[phase].raw_ms.length > 0);
    assert.ok(observation[phase].raw_ms.every(v => Number.isFinite(v) && v >= 0));
    assert.equal(observation[phase].max_ms, Math.max(...observation[phase].raw_ms));
  }
}
console.log('Verified sustained-scroll 60 Hz observation assets and report link.');


for (const demo of ['media-filters', 'mipmaps', 'path-effects']) {
  const html = data.extension_documents['media-images'].html;
  assert.ok(html.includes(`href="examples/extensions/?demo=${demo}"`));
  assert.ok(!html.includes(`href="../examples/extensions/?demo=${demo}"`));
  assert.ok(wasmIndex.includes(`"${demo}"`), 'loader accepts ' + demo);
}
assert.ok(data.extension_documents['svg-rendering'].html.includes('href="examples/extensions/?demo=path-effects"'));
console.log('Verified portable media demo guide links and loader routes.');

// Keep the same locally hosted Prism core/Rust grammar as Luma; verify real tokenization.
const prismContext = vm.createContext({});
for (const file of ['prism-core.min.js', 'prism-rust.min.js']) {
  assert.ok(index.includes(`vendor/prism/${file}`));
  vm.runInContext(await readFile(new URL(`vendor/prism/${file}`, site), 'utf8'), prismContext);
}
const rustHighlight = vm.runInContext('Prism.highlight(\'let title = "Preview"; // note\', Prism.languages.rust, "rust")', prismContext);
assert.ok(rustHighlight.includes('token keyword'));
assert.ok(rustHighlight.includes('token string'));
assert.ok(rustHighlight.includes('token comment'));
assert.ok((await readFile(new URL('vendor/prism/LICENSE', site), 'utf8')).includes('MIT LICENSE'));
assert.ok(index.includes('code-examples.js'));
assert.ok(index.includes('data-manual'));
console.log('Verified local Prism Rust assets, tokenization and license.');
await import('./code-examples.test.mjs');
