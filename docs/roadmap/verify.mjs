import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import vm from 'node:vm';

const site = new URL('./site/', import.meta.url);
const source = await readFile(new URL('app.js', site), 'utf8');
const inventory = await readFile(new URL('data.js', site), 'utf8');
const context = vm.createContext({ window: {}, URLSearchParams });
vm.runInContext(inventory, context);
vm.runInContext(await readFile(new URL("guides.js", site), "utf8"), context);
vm.runInContext(await readFile(new URL("build-guide.js", site), "utf8"), context);
vm.runInContext(await readFile(new URL("pipeline.js", site), "utf8"), context);
vm.runInContext(await readFile(new URL("extensions.js", site), "utf8"), context);
vm.runInContext(source, context);
const run = code => vm.runInContext(code, context);
const data = context.window.ROADMAP;
assert.equal(data.crates.length, Object.keys(data.provenance.packages).length);
assert.equal(Object.keys(data.notes).sort().join(), data.crates.map(c => c.package).sort().join());
assert.equal(run('parseRoute(crateUrl("gpui", "surface")).tab'), 'surface');
assert.equal(run('parseRoute("#method").global'), 'method');
assert.equal(run('escapeHtml("<script>&")'), "&lt;script&gt;&amp;");
assert.equal(run('filterDeclarations([{name:"Example", signature:"", module:"crate", file:"", guards:[],kind:"struct", root_export:false}], "example").length'), 1);
assert.equal(run('filterDeclarations([{name:"Example", signature:"", module:"crate", file:"", guards:[],kind:"struct", root_export:false}], "", "", true).length'), 0);
assert.match(run('overview()'), /Know the code/);
assert.match(run('method()'), /Syntactic Rust inventory/);
assert.match(run('review()'), /Review hypotheses/);
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
  for (const dep of c.dependencies) assert.ok(data.provenance.packages[dep.package]);
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
assert.match(element('content').innerHTML, /Know the code/);
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
for (const route of ['#extensions', ...Object.keys(data.extension_documents).map(route => '#' + route), '#pipeline', '#build', '#architecture', '#how-gpui-works', '#method', '#review', '#crate=unknown']) {
  context.location.hash = route;
  run('render()');
  assert.ok(element('content').innerHTML.length);
}
const index = await readFile(new URL('index.html', site), 'utf8');
for (const asset of ['style.css', 'extensions.js', 'guides.js', 'build-guide.js', 'pipeline.js', 'app.js', 'data.js']) {
  assert.ok(index.includes(asset));
  await readFile(new URL(asset, site));
}
console.log('Verified ' + data.crates.length + ' crates, ' + pages.size + ' source pages, declaration anchors, routes, filtering and assets.');

const entry = await readFile(new URL("./index.html", import.meta.url), "utf8");
assert.match(entry, /http-equiv="refresh" content="0;url=site\/index.html"/);
assert.match(entry, /href="site\/index.html"/);
assert.ok(!entry.includes("src=\"data.js\""));

for (const [fn, title, sections] of [["architecturePage", "The architecture of GPUI", 8], ["howGpuiWorksPage", "How GPUI works", 10]]) {
 const html = run(fn + "()");
 assert.ok(html.includes(title));
 assert.equal((html.match(/class="guide-section"/g) || []).length, sections);
 assert.ok(!html.includes("undefined"));
 for (const match of html.matchAll(/href="source\/([^"#]+)\.html#L(\d+)"/g)) {
  const file = decodeURIComponent(match[1]);
  assert.ok(pages.has(file), file);
  assert.ok(pages.get(file).includes('id="L' + match[2] + '"'), file + ":" + match[2]);
 }
}
console.log("Verified both detailed guides and their source references.");

const buildHtml = run("buildGuidePage()");
for (const term of ["Build GPUI and your app", "wasm32-unknown-unknown", "WebGL2", "runtime-shaders", "Complete captured feature reference"]) assert.ok(buildHtml.includes(term));
for (const c of data.crates) {
 assert.ok(buildHtml.includes(c.package));
 const manifest = data.build_files["gpui/crates/" + c.package + "/Cargo.toml"];
 assert.ok(manifest);
 for (const name of Object.keys(c.features)) assert.ok(manifest.includes(name));
}
context.location.hash = "#build";
run("render()");
assert.match(element("content").innerHTML, /Build GPUI and your app/);
assert.match(context.document.title, /Build GPUI/);
assert.ok(!buildHtml.includes("undefined"));
console.log("Verified build guide route, complete feature reference and manifest evidence.");


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
assert.equal(Object.keys(data.extension_documents).length, 8);
assert.match(run('extensionPage("extensions")'), /Downstream extension to GPUI/);
assert.match(index, /href="#extensions"/);
for (const [route, doc] of Object.entries(data.extension_documents)) {
  assert.equal(doc.source, 'docs/roadmap/extensions/' + route + '.md');
  await readFile(new URL('../../' + doc.source, import.meta.url));
  context.location.hash = '#' + route;
  run('render()');
  assert.ok(element('content').innerHTML.includes(doc.html));
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
  'apps/shader-lab/README.md',
  'apps/gamut-lab/README.md',
]) {
  const url = new URL('../../' + file, import.meta.url);
  const markdown = await readFile(url, 'utf8');
  for (const [, link] of markdown.matchAll(/\]\(([^)]+)\)/g)) {
    if (/^(?:https?:|#)/.test(link)) continue;
    await readFile(new URL(link.split('#')[0], url));
  }
}
console.log('Verified extension navigation, offline contracts, consolidated sources, README links and cross-document links.');

const apiChanges = data.extension_documents['gpui-public-api'].html;
for (const term of ['Scene', 'custom_draws', 'PrimitiveBatch', 'CustomContent', 'PreparationRequest', 'CustomGeometry', 'CustomCoverage', 'CustomDraw', 'paint_custom', 'MetalCustomContext', 'WgpuCustomContext', 'Dx11CustomContext', 'source compatibility risks']) assert.ok(apiChanges.includes(term), term);
assert.match(index, /href="#gpui-public-api"/);
assert.match(run('extensionPage("extensions")'), /href="#gpui-public-api"/);
console.log('Verified public API delta documentation and prominent navigation.');

for (const match of run('publicApiSourceLinks()').matchAll(/href="source\/([^"#]+)\.html#L(\d+)"/g)) {
  const file = decodeURIComponent(match[1]);
  assert.ok(pages.has(file), file);
  assert.ok(pages.get(file).includes('id="L' + match[2] + '"'), file + ':' + match[2]);
}
console.log('Verified API-change source links against regenerated declaration anchors.');
