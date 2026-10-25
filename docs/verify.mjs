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
for (const demo of ['shader', 'parley']) {
  const html = run(`browserDemoLinks('${demo}')`);
  assert.ok(html.includes(`href="examples/extensions/?demo=${demo}"`));
  assert.ok(html.includes('https://github.com/gpui-luma-dev/xgpui/tree/main/apps/extensions-web'));
  assert.equal(new URL(`examples/extensions/?demo=${demo}`, 'https://example.github.io/xgpui/').pathname, '/xgpui/examples/extensions/');
}

for (const asset of ['style.css', 'extensions.js', 'guides.js', 'pipeline.js', 'app.js', 'data.js']) {
  assert.ok(index.includes(asset));
  await readFile(new URL(asset, site));
}
console.log('Verified ' + data.crates.length + ' crates, ' + pages.size + ' source pages, declaration anchors, routes, filtering and assets.');

assert.ok(!index.includes('http-equiv="refresh"'), 'Pages root must serve the website directly');
// Relative URLs must retain the repository prefix on project Pages sites.
const pagesRoot = new URL('https://example.github.io/xgpui/');
for (const match of index.matchAll(/(?:src|href)="([^"#][^"]*)"/g)) {
  assert.ok(new URL(match[1], pagesRoot).pathname.startsWith('/xgpui/'));
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
assert.equal(Object.keys(data.extension_documents).length, 10);
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
for (const term of ['svg_image', 'SchedulerLimits', 'Queued', '3 × output RGBA', '2 × RGBA', 'process/GPU-memory ceiling', 'samples', 'attributed']) assert.ok(svgDoc.includes(term), term);
assert.doesNotMatch(run('cratePage(crates.find(c => c.package === "xgpui-svg"), "overview")'), /Run browser demo/);
const svgCrate = data.crates.find(c => c.package === 'xgpui-svg');
for (const name of ['SvgImage', 'SvgDocument', 'SvgScheduler', 'SchedulerStats', 'RasterPolicy']) assert.ok(svgCrate.declarations.some(d => d.name === name), name);
assert.ok(svgCrate.dependencies.some(d => d.package === 'gpui'));
assert.match(run('extensionPage("parley-text")'), /Text backend extension/);
assert.match(data.extension_documents['high-gamut-windows'].html, /WideColor::linear_p3/);
assert.match(data.extension_documents['high-gamut-windows'].html, /xgpui-gamut/);

assert.match(run('extensionPage("extensions")'), /Downstream extensions to GPUI/);
assert.ok(!index.includes('<a href="#extensions">Extensions to GPUI</a>'), 'extensions belong in the crate group');
for (const [route, doc] of Object.entries(data.extension_documents)) {
  assert.equal(doc.source, 'docs/extensions/' + route + '.md');
  await readFile(new URL('../' + doc.source, import.meta.url));
  context.location.hash = '#' + route;
  run('render()');
  assert.ok(element('content').innerHTML.includes(route === 'high-gamut-windows' ? doc.html.replace(/^<h1[^>]*>.*?<\/h1>\s*/, '') : doc.html));
  assert.ok(context.document.title.includes(doc.title));
  assert.ok(!doc.html.includes('.md"'), 'unconverted Markdown link: ' + route);
  for (const match of doc.html.matchAll(/href="#([^"&]+)(?:&amp;section=([^"&]+))?"/g)) {
    const target = data.extension_documents[match[1]];
    assert.ok(target, 'missing extension route: ' + match[1]);
    if (match[2]) assert.ok(target.html.includes('id="' + match[2] + '"'), 'missing section: ' + match[2]);
  }
}
assert.equal(run('parseRoute("#paint-contract-hardening&section=implemented-discardable-recording-transactions").global'), 'paint-contract-hardening');
assert.equal(run('parseRoute("#paint-contract-hardening&section=implemented-discardable-recording-transactions").section'), 'implemented-discardable-recording-transactions');
for (const file of [
  'extensions/xgpui-ext/README.md',
  'extensions/xgpui-ext/crates/xgpui-shader/README.md',
  'apps/paint-lab/README.md',
  'apps/gamut-lab/README.md',
  'extensions/xgpui-ext/crates/xgpui-gamut/README.md',
  'extensions/xgpui-ext/crates/xgpui-svg/README.md',
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
assert.ok(!gamutCrate.dependencies.some(d => d.package === 'xgpui-shader'));
assert.ok(!shaderCrate.dependencies.some(d => d.package === 'xgpui-gamut'));
console.log('Verified extension crate navigation, individual code-first guides, filtering and source links.');

const apiChanges = data.extension_documents['gpui-public-api'].html;
for (const term of ['Scene', 'custom_draws', 'PrimitiveBatch', 'CustomContent', 'PreparationRequest', 'CustomGeometry', 'CustomCoverage', 'CustomDraw', 'paint_custom', 'MetalCustomContext', 'WgpuCustomContext', 'Dx11CustomContext', 'source compatibility risks', 'WindowColorMode', 'WindowColorStatus', 'MetalWideSdrOutput', 'Dx11WideSdrOutput', 'set_color_mode', 'ParsedSvg::size()', 'xgpui-svg']) assert.ok(apiChanges.includes(term), term);
assert.match(index, /href="#gpui-public-api"/);
assert.match(run('extensionPage("extensions")'), /href="#gpui-public-api"/);
console.log('Verified public API delta documentation and prominent navigation.');

for (const match of run('publicApiSourceLinks()').matchAll(/href="source\/([^"#]+)\.html#L(\d+)"/g)) {
  const file = decodeURIComponent(match[1]);
  assert.ok(pages.has(file), file);
  assert.ok(pages.get(file).includes('id="L' + match[2] + '"'), file + ':' + match[2]);
}
console.log('Verified API-change source links against regenerated declaration anchors.');
