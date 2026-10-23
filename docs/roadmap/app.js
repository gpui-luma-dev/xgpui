'use strict';
const data = window.ROADMAP;
const crates = [...data.crates].sort((a, b) => Number(b.package === "gpui") - Number(a.package === "gpui"));
const notes = data.notes;
const escapeHtml = value => String(value ?? '').replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const short = pkg => notes[pkg]?.short || pkg;
const crateUrl = (pkg, tab='overview') => '#crate=' + encodeURIComponent(pkg) + '&tab=' + tab;
const sourceUrl = (file, line=1) => 'source/' + file.split('/').map(encodeURIComponent).join('/') + '.html#L' + line;
const sourceLink = (file, line=1, label='Source') => '<a href="' + sourceUrl(file,line) + '">' + escapeHtml(label) + '</a>';
const crateLink = pkg => '<a href="' + crateUrl(pkg) + '">' + escapeHtml(short(pkg)) + '</a>';
const tag = label => '<span class="tag">' + escapeHtml(label) + '</span>';
const groups = ["Extensions", "Core", "Platforms & rendering", "Data & runtime", "Transport & tooling"];
let page = 0;
function parseRoute(hash) {
  const p = new URLSearchParams(hash.replace(/^#/,''));
  return {pkg:p.get('crate'), tab:p.get('tab') || 'overview', global:hash.replace(/^#/,'').split('&')[0] || 'overview', section:p.get('section')};
}
function filterDeclarations(items, query='', kind='', root=false) {
  const q = query.trim().toLowerCase();
  return items.filter(d => (!root || d.root_export) && (!kind || d.kind===kind) && (!q || [d.name,d.signature,d.module,d.file,...d.guards].join(' ').toLowerCase().includes(q)));
}
function sidebar(query='') {
  const route = parseRoute(location.hash);
  const q = query.toLowerCase().trim();
  const matches = crates.filter(c => !q || (c.package+' '+notes[c.package].role+' '+c.declarations.map(d=>d.name).join(' ')).toLowerCase().includes(q));
  document.getElementById('crate-list').innerHTML = groups.map(g => {
    const list=matches.filter(c=>notes[c.package].group===g);
    const entries = list.map(c=>'<a class="crate-link" '+(route.pkg===c.package || (c.package === 'xgpui-shader' && shaderGuideRoutes.includes(route.global)) || (c.package === 'xgpui-gamut' && route.global === 'high-gamut-windows') ? 'aria-current="page" ' : '')+'href="'+crateUrl(c.package)+'">'+escapeHtml(short(c.package))+'</a>').join('');
    return entries ? '<p class="group-label">'+escapeHtml(g)+'</p>'+entries : '';
  }).join('') || '<p>No crates match.</p>';

}
function header(eyebrow,title,lead) {return '<div class="eyebrow">'+escapeHtml(eyebrow)+'</div><h1>'+escapeHtml(title)+'</h1><p class="lead">'+escapeHtml(lead)+'</p>';}
function overview() {
  const count = crates.reduce((n,c)=>n+c.declarations.length,0);
  const files = new Set(crates.flatMap(c=>c.files)).size;
  return header('Captured source / '+data.provenance.ref,'Know the code. Shape the contract.','A crate-by-crate roadmap for understanding GPUI before changing its API. Start with responsibilities, follow the source, then decide which boundaries deserve a smaller contract.')+
    '<div class="stats"><div class="stat"><strong>'+crates.length+'</strong><span>Captured crates</span></div><div class="stat"><strong>'+count.toLocaleString()+'</strong><span>Source declarations & impls</span></div><div class="stat"><strong>'+files+'</strong><span>Rust source files</span></div></div>'+
    '<div class="notice">This is a source inventory, not a compiler-resolved public API report. Private-module declarations, conditional code and trait implementations are included for investigation. <a href="#method">Read the limits</a>.</div>'+
    '<div class="cards"><a class="card" href="#architecture"><h3>Architecture overview →</h3><p>Ownership, layers, contracts and rendering extension boundaries.</p></a><a class="card" href="#how-gpui-works"><h3>How GPUI works →</h3><p>Follow launch, input, notification, layout and the next displayed frame.</p></a></div><div class="route"><h3><a href="#build">Build GPUI and your app →</a></h3><p>Native targets, renderer and feature choices, application setup, single-threaded WASM and threaded browser deployment.</p></div><div class="route"><h3><a href="#pipeline">Explore the rendering pipeline →</a></h3><p>An animated SVG walkthrough: move geometry, clip primitives, group layers and inspect the resulting batches.</p></div><div class="route"><h3><a href="#extensions">Extensions to GPUI →</a></h3><p>Opt-in downstream capabilities: the xgpui-shader crate, integration/API contracts and portable shader adapters. These additions are distinct from upstream GPUI.</p></div><h2>A reading route for rendering changes</h2><div class="route">'+crateLink('gpui')+' → '+crateLink('gpui-platform')+' → platform implementation → '+crateLink('gpui-apple')+' / '+crateLink('gpui-wgpu')+'<p>Trace scene primitives and platform window contracts first. Then inspect surface formats, shaders and color handling in the selected renderer. A wider-gamut window requires an end-to-end color contract.</p></div>'+
    groups.map(g=>'<h2>'+escapeHtml(g)+'</h2><div class="cards">'+crates.filter(c=>notes[c.package].group===g).map(c=>'<a class="card" href="'+crateUrl(c.package)+'"><h3>'+escapeHtml(short(c.package))+'</h3><p>'+escapeHtml(notes[c.package].role)+'</p>'+tag(c.declarations.length+' declarations')+tag(c.dependencies.length+' internal manifest edges')+'</a>').join('')+'</div>').join('');
}
function method() {
  const warnings=crates.filter(c=>c.warnings.length);
  return header('Evidence & limits','How this map is made','The Rust generator reads upstream.json, the local extension package map, each Cargo manifest and the modules reachable from its library entrypoint. Curated notes explain responsibilities and suggest review questions.')+
    '<h2>What the counts mean</h2><p>'+escapeHtml(data.methodology)+'</p><ul><li><strong>Root declaration/export:</strong> written at the crate root, or an exported macro. Glob re-exports remain an export statement rather than a resolved list of names.</li><li><strong>Public module path:</strong> all enclosing modules have public visibility. This does not prove that an owner type or a trait implementation is externally reachable.</li><li><strong>Private module path:</strong> may still reach consumers through re-exports. Never use this label as a removal decision.</li><li><strong>Conditional declarations:</strong> cfg attributes are shown without evaluating a target or feature set. Plain cfg(test) items/modules are excluded; other test-support code remains visible.</li><li>Macro-generated styles, derives, includes and external re-exports need compiler/rustdoc inspection. Associated items and generic bounds need their full source context.</li><li>Dependency edges cover captured packages only, with normal, dev, build, optional and target metadata. They describe manifests, not a resolved build graph or actual symbol use.</li><li>The generator inventories library module trees. Separate binary entrypoints, examples and benches are not an exhaustive part of this index.</li></ul>'+
    '<h2>Snapshot provenance</h2><p>Captured release: <code>'+escapeHtml(data.provenance.ref)+'</code><br>Upstream commit: <code>'+escapeHtml(data.provenance.commit)+'</code></p><p>Generated from the local captured files. Downstream patches may differ from the upstream commit. Rebuild after a sync; review curated notes when responsibilities change.</p>'+
    '<h2>Unresolved source items</h2>'+(warnings.length?warnings.map(c=>'<details><summary>'+escapeHtml(short(c.package))+' · '+c.warnings.length+'</summary><ul>'+c.warnings.map(w=>'<li><code>'+escapeHtml(w)+'</code></li>').join('')+'</ul></details>').join(''):'<p>No unresolved file modules or include! calls were recorded. This does not mean macros were expanded.</p>')+
    '<h2>Refresh</h2><p><code>cargo run --manifest-path tools/crate-roadmap/Cargo.toml</code></p><p>The generated site uses local assets and source pages. Open its index.html directly; no server, Python or network runtime is required.</p>';
}
function review() {
  return header('Review hypotheses / no API changes','Make the contract deliberate.','Use this roadmap to identify boundaries worth testing before editing public APIs. These are investigation candidates, not approved removals.')+
    '<div class="notice">A package dependency is not proof that a symbol is used. An unused-looking declaration is not proof that downstream applications can lose it.</div>'+
    '<h2>Suggested order of work</h2><ol><li><strong>Establish the baseline.</strong> Compile representative consumers and record the supported targets and feature combinations. Use rustdoc or a compiler-resolved API tool to supplement this syntactic index.</li><li><strong>Define the rendering contract.</strong> Document scene-to-renderer ownership, texture formats, shader inputs and surface color behavior. Begin with Apple/Metal and wgpu source, then compare native window contracts.</li><li><strong>Build focused compatibility probes.</strong> Each probe should exercise a named contract: window creation, entity updates, text layout, image upload or a renderer extension. Design these after choosing the contract.</li><li><strong>Review exposure by subsystem.</strong> Separate supported application APIs, backend contracts, macro support and optional tooling. Trace re-exports and signature types before changing visibility.</li><li><strong>Change one boundary at a time.</strong> Compare the refreshed inventory, compile consumers and review the incoming upstream diff before making a stability promise.</li></ol>'+
    groups.map(g=>'<h2>'+escapeHtml(g)+'</h2>'+crates.filter(c=>notes[c.package].group===g).map(c=>'<article class="card"><h3>'+crateLink(c.package)+'</h3><p>'+escapeHtml(notes[c.package].review)+'</p>'+sourceLink(c.entry,1,'Read crate entrypoint')+'</article>').join('')).join('');
}
function tabs(c,active) {return '<nav class="tabs" aria-label="Crate sections">'+[['overview',notes[c.package].group === 'Extensions' ? 'Use' : 'Role & sources'],...(notes[c.package].group === 'Extensions' ? [['sources','Role & sources']] : []),['surface','API inventory'],['dependencies','Dependencies'],['review','Trim review']].map(([id,label])=>'<a '+(active===id?'aria-current="page" ':'')+'href="'+crateUrl(c.package,id)+'">'+label+'</a>').join('')+'</nav>';}
function cratePage(c,tab) {
  const n=notes[c.package];
  let body='';
  if(tab==='surface') {
    const kinds=[...new Set(c.declarations.map(d=>d.kind))].sort();
    body='<div class="notice">These are source declarations and implementation candidates. Root exports are distinguished from declarations behind private modules. Macro expansions and glob export reachability are unresolved.</div><div class="filters"><label>Search declarations<input id="api-search" type="search" placeholder="Symbol, signature, module or cfg…"></label><label>Kind<select id="api-kind"><option value="">All kinds</option>'+kinds.map(k=>'<option>'+escapeHtml(k)+'</option>').join('')+'</select></label><label class="check"><input id="api-root" type="checkbox"> Root declarations / exports only</label></div><div id="api-results" aria-live="polite"></div>';
  } else if(tab==='dependencies') {
    const incoming=crates.flatMap(other=>other.dependencies.filter(d=>d.package===c.package).map(d=>({...d,from:other.package})));
    const edge=(d,inbound)=>'<div class="dependency">'+crateLink(inbound?d.from:d.package)+' '+tag(d.kind)+(d.optional?tag('optional'):'')+(d.target?tag(d.target):'')+'<small> · manifest alias: <code>'+escapeHtml(d.alias)+'</code></small></div>';
    body='<p>Captured-package manifest edges, including conditional, optional, development and build dependencies. External packages and resolved feature activation are outside this graph.</p><h2>Depends on · '+c.dependencies.length+'</h2>'+(c.dependencies.map(d=>edge(d,false)).join('')||'<p>No captured-package dependencies.</p>')+'<h2>Referenced by · '+incoming.length+'</h2>'+(incoming.map(d=>edge(d,true)).join('')||'<p>No captured-package incoming edges.</p>')+'<h2>Declared features</h2><pre><code>'+escapeHtml(JSON.stringify(c.features,null,2))+'</code></pre>';
  } else if(tab==='review') {
    body='<h2>Boundary to investigate</h2><div class="notice">Review hypothesis. No visibility changes or removal decisions have been made.</div><p class="lead">'+escapeHtml(n.review)+'</p><p>Follow '+sourceLink(c.entry,1,'the entrypoint')+' and inspect incoming dependencies. Then check expanded macros, re-exports and actual downstream consumers before narrowing the API.</p><p><a href="#review">Read the overall review sequence →</a></p>';
  } else if(notes[c.package].group === 'Extensions' && tab === 'overview') {
    body=c.package === 'xgpui-shader' ? shaderCrateGuide() : gamutCrateGuide();
  } else {
    body='<h2>Public surface at a glance</h2><p>'+escapeHtml(n.surface)+'</p><p>'+sourceLink(c.entry,1,'Open library entrypoint →')+'</p><div class="stats"><div class="stat"><strong>'+c.declarations.length+'</strong><span>Declaration / impl candidates</span></div><div class="stat"><strong>'+c.declarations.filter(d=>d.root_export).length+'</strong><span>Root declarations / exports</span></div><div class="stat"><strong>'+c.files.length+'</strong><span>Source files</span></div></div><h2>Source map</h2><div class="source-list">'+c.files.map(file=>sourceLink(file,1,file.replace(c.entry.slice(0,c.entry.indexOf('/src/')+1),''))).join('')+'</div>'+(c.warnings.length?'<div class="notice">'+c.warnings.length+' unresolved source items. <a href="#method">Inspect inventory limits</a>.</div>':'');
  }
  return header(n.group,short(c.package),n.role)+'<p class="muted"><code>'+escapeHtml(c.package)+'</code> · '+escapeHtml(c.version)+(c.procedural?' · procedural macro crate':'')+'</p>'+tabs(c,tab)+body;
}
function renderInventory(c) {
  const results=filterDeclarations(c.declarations,document.getElementById('api-search').value,document.getElementById('api-kind').value,document.getElementById('api-root').checked);
  const size=60;
  page=Math.min(page,Math.max(0,Math.ceil(results.length/size)-1));
  document.getElementById('api-results').innerHTML='<p class="muted">'+results.length.toLocaleString()+' matching declarations · page '+(page+1)+' of '+Math.max(1,Math.ceil(results.length/size))+'</p>'+results.slice(page*size,(page+1)*size).map(d=>'<article class="declaration"><h3>'+escapeHtml(d.name)+'</h3>'+tag(d.kind)+(d.root_export?tag('root declaration / export'):tag(d.module_public?'public module path':'private module path'))+'<pre>'+escapeHtml(d.signature)+'</pre><small>'+escapeHtml(d.module)+' · '+sourceLink(d.file,d.line,d.file.split('/').slice(-2).join('/')+':'+d.line)+'</small>'+(d.guards.length?'<small>'+escapeHtml(d.guards.join(' · '))+'</small>':'')+'</article>').join('')+(results.length?'':'<p class="empty">No declarations match these filters.</p>')+'<div class="paging"><button id="previous" '+(page===0?'disabled':'')+'>Previous</button><button id="next" '+((page+1)*size>=results.length?'disabled':'')+'>Next</button></div>';
  document.getElementById('previous').onclick=()=>{page--;renderInventory(c);};
  document.getElementById('next').onclick=()=>{page++;renderInventory(c);};
}
function render() {
  stopPipeline();
  const route=parseRoute(location.hash);
  const c=crates.find(c=>c.package===route.pkg);
  page=0;
  const content=document.getElementById('content');
  content.innerHTML=c?cratePage(c,route.tab):route.pkg?header('Not found','Unknown crate','Choose a captured crate from the navigation.'):isExtensionRoute(route.global)?extensionPage(route.global):route.global==='pipeline'?pipelinePage():route.global==='build'?buildGuidePage():route.global==='architecture'?architecturePage():route.global==='how-gpui-works'?howGpuiWorksPage():route.global==='method'?method():route.global==='review'?review():overview();
  document.title=(c?short(c.package):isExtensionRoute(route.global)?extensionTitle(route.global):route.global==='pipeline'?'Rendering pipeline explorer':route.global==='build'?'Build GPUI and your app':route.global==='architecture'?'Architecture overview':route.global==='how-gpui-works'?'How GPUI works':route.global==='review'?'API review roadmap':route.global==='method'?'Inventory methodology':'Code roadmap')+' · xgpui';
  sidebar(document.getElementById('crate-search').value);
  if(route.section && isExtensionRoute(route.global)) document.getElementById(route.section)?.scrollIntoView?.();
  if(route.global==='pipeline' && !c && !route.pkg) bindPipeline();
  if(c && route.tab==='surface') {
    for(const id of ['api-search','api-kind','api-root']) document.getElementById(id).addEventListener('input',()=>{page=0;renderInventory(c);});
    renderInventory(c);
  }
}
if(typeof document!=='undefined') {
  window.addEventListener('hashchange',()=>{render();window.scrollTo(0,0);});
  document.getElementById('crate-search').addEventListener('input',event=>sidebar(event.target.value));
  render();
}
