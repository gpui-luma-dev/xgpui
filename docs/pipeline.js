'use strict';

// Teaching model for a small subset of Scene; not a GPUI/GPU execution trace.
const pipelineKinds = ['Shadow','Quad','Path','Underline','MonochromeSprite','SubpixelSprite','PolychromeSprite','Surface'];
const pipelineStages = [
  ['Views','Persistent entities produce an element tree. One element may paint several primitives; elements are not draw calls.'],
  ['Layout / prepaint','Layout supplies bounds. Prepaint establishes hitboxes and other frame state. This example uses fixed logical geometry and scale factor 1.'],
  ['Paint → scene','Intersect bounds with the content mask; reject an empty intersection. Assign an order, append to a type array and record a paint operation.'],
  ['Finish','Sort each type array by order. Sprite arrays also sort by tile_id. This does not flatten the scene into one global array.'],
  ['Batch','Merge the heads of the type arrays by (order, primitive kind). Sprite batches also stop at a texture change.'],
  ['GPU work','The backend consumes batches and binds the appropriate pipelines/resources. Paths can rasterize into an intermediate texture before compositing.'],
  ['Present','The platform presents the rendered result. The picture here is an SVG explanation, not GPUI pixels or a measured GPU trace.'],
  ['Cached replay','A new scene can replay previous paint operations through insertion, recalculating order. Here the entire toy scene is replayed unchanged; real views replay selected ranges.'],
];
let pipelineState = {stage:0,badgeX:190,mask:true,layer:false,atlas:false,backend:'metal',selected:'photo'};
let pipelineTimer = null;
let pipelinePrevious = null;

function pipelineIntersection(a,b) {
  const x=Math.max(a.x,b.x), y=Math.max(a.y,b.y);
  return {x,y,w:Math.max(0,Math.min(a.x+a.w,b.x+b.w)-x),h:Math.max(0,Math.min(a.y+a.h,b.y+b.h)-y)};
}
function pipelineOverlap(a,b) {const r=pipelineIntersection(a,b);return r.w>0 && r.h>0;}
function pipelineCompare(a,b) {return a.order-b.order || pipelineKinds.indexOf(a.kind)-pipelineKinds.indexOf(b.kind);}
function pipelineBatch(arrays) {
  const queues=Object.fromEntries(pipelineKinds.map(k=>[k,[...(arrays[k]||[])]]));
  const batches=[];
  while(true) {
    const heads=Object.values(queues).filter(a=>a.length).map(a=>a[0]).sort(pipelineCompare);
    if(!heads.length) return batches;
    const head=heads[0], next=heads[1], items=[queues[head.kind].shift()];
    const queue=queues[head.kind];
    while(queue.length && (!next || pipelineCompare(queue[0],next)<0) && (!head.kind.endsWith('Sprite') || queue[0].texture===head.texture)) items.push(queue.shift());
    batches.push({kind:head.kind,texture:head.texture,items});
  }
}
function replayPipeline(operations) {
  const bounds=[], stack=[], primitives=[];
  const insert=rect=>{const order=1+Math.max(0,...bounds.filter(n=>pipelineOverlap(n.bounds,rect)).map(n=>n.order));bounds.push({bounds:rect,order});return order;};
  for(const op of operations) {
    if(op.op==='StartLayer') stack.push(insert(op.bounds));
    else if(op.op==='EndLayer') stack.pop();
    else {
      const p=op.primitive, clipped=pipelineIntersection(p.bounds,p.mask);
      if(clipped.w && clipped.h) primitives.push({...p,clipped,order:stack.at(-1)??insert(clipped)});
    }
  }
  return primitives;
}
function composePipeline(state) {
  const full={x:0,y:0,w:360,h:255};
  const photoMask=state.mask?{x:36,y:70,w:170,h:112}:full;
  const badge={x:Number(state.badgeX),y:148,w:104,h:58};
  const make=(id,kind,bounds,extra={})=>({id,kind,bounds,mask:full,...extra});
  const inputs=[
    make('panel','Quad',{x:20,y:24,w:320,h:205}),
    make('title-a','MonochromeSprite',{x:36,y:39,w:110,h:16},{texture:0,tile:1}),
    make('title-b','MonochromeSprite',{x:154,y:39,w:52,h:16},{texture:state.atlas?1:0,tile:2}),
    make('photo','PolychromeSprite',{x:36,y:70,w:205,h:136},{mask:photoMask,texture:2,tile:8}),
    make('badge','Quad',badge,{layer:state.layer}),
    make('path','Path',{x:badge.x+10,y:badge.y+15,w:18,h:24},{layer:state.layer}),
    make('label','MonochromeSprite',{x:badge.x+34,y:badge.y+21,w:60,h:14},{layer:state.layer,texture:0,tile:3}),
    make('clipped-out','Quad',{x:285,y:78,w:28,h:22},{mask:photoMask}),
  ];
  const tree=[], primitives=[], rejected=[], operations=[];
  const orderFor=bounds=>{const order=1+Math.max(0,...tree.filter(n=>pipelineOverlap(n.bounds,bounds)).map(n=>n.order));tree.push({bounds,order});return order;};
  let layerOrder=null;
  for(const input of inputs) {
    if(input.id==='badge' && state.layer) {layerOrder=orderFor(badge);operations.push({op:'StartLayer',bounds:badge,order:layerOrder});}
    const clipped=pipelineIntersection(input.bounds,input.mask);
    if(!clipped.w || !clipped.h) rejected.push({...input,clipped});
    else {
      const primitive={...input,clipped,order:input.layer?layerOrder:orderFor(clipped)};
      primitives.push(primitive);operations.push({op:'Primitive',primitive});
    }
    if(input.id==='label' && state.layer) {operations.push({op:'EndLayer'});layerOrder=null;}
  }
  const arrays=Object.fromEntries(pipelineKinds.map(kind=>[kind,primitives.filter(p=>p.kind===kind).sort((a,b)=>a.order-b.order || (kind.endsWith('Sprite')?a.tile-b.tile:0))]));
  return {inputs,primitives,rejected,operations,arrays,batches:pipelineBatch(arrays),photoMask,replayed:replayPipeline(operations)};
}
function pipelineSvg(model,state,previous,reducedMotion=false) {
  const phase=state.stage, ordered=phase>=4?model.batches.flatMap(b=>b.items):model.primitives;
  const old=new Map((previous?.primitives||[]).map(p=>[p.id,p]));
  const defs='<defs><linearGradient id="pipe-photo" x2="0" y2="1"><stop stop-color="#b0b0b0"/><stop offset="1" stop-color="#dcdcdc"/></linearGradient>'+ordered.map(p=>'<clipPath id="pipe-clip-'+p.id+'"><rect x="'+p.clipped.x+'" y="'+p.clipped.y+'" width="'+p.clipped.w+'" height="'+p.clipped.h+'"/></clipPath>').join('')+'</defs>';
  const marks=ordered.map(p=>{
    const b=p.bounds, before=old.get(p.id)?.bounds;
    const motion=!reducedMotion && before && (before.x!==b.x || before.y!==b.y)?'<animateTransform attributeName="transform" type="translate" from="'+before.x+' '+before.y+'" to="'+b.x+' '+b.y+'" dur="0.35s" fill="freeze"/>':'';
    let shape;
    if(phase<2) shape='<rect width="'+b.w+'" height="'+b.h+'" fill="none" stroke="#636363" stroke-dasharray="'+(phase===0?'4 4':'0')+'"/>';
    else if(p.id==='panel') shape='<rect width="320" height="205" rx="9" fill="#eaeaea"/>';
    else if(p.id==='photo') shape='<rect width="205" height="136" fill="url(#pipe-photo)"/><circle cx="151" cy="29" r="16" fill="#dfdfdf"/><path d="M0 136 L68 40 L125 111 L160 69 L205 136Z" fill="#626262"/><path d="M48 68 L68 40 L88 66 L68 59Z" fill="#ececec"/>';
    else if(p.id==='badge') shape='<rect width="104" height="58" rx="8" fill="#434343"/>';
    else if(p.id==='path') shape='<path d="M9 0 L18 15 L12 15 L12 24 L6 24 L6 15 L0 15Z" fill="#bfbfbf"/>';
    else if(p.id==='label') shape='<text x="0" y="12" fill="#f3f3f3" font-size="13">Adjust</text>';
    else if(p.id==='title-a') shape='<text x="0" y="13" fill="#353535" font-size="14" font-weight="600">Photo canvas</text>';
    else if(p.id==='title-b') shape='<text x="0" y="13" fill="#555555" font-size="12">100%</text>';
    else shape='<rect width="28" height="22" fill="#929292"/>';
    return '<g clip-path="url(#pipe-clip-'+p.id+')"><g transform="translate('+b.x+' '+b.y+')">'+motion+shape+'</g></g>';
  }).join('');
  const selected=model.inputs.find(p=>p.id===state.selected), b=selected?.bounds;
  const selection=b?'<rect x="'+b.x+'" y="'+b.y+'" width="'+b.w+'" height="'+b.h+'" fill="none" stroke="#646464" stroke-width="2" stroke-dasharray="5 3"/>':'';
  const mask=state.mask?'<rect x="36" y="70" width="170" height="112" fill="none" stroke="#5d5d5d" stroke-dasharray="3 3"/>':'';
  return '<svg class="pipeline-preview" viewBox="0 0 360 255" role="img" aria-labelledby="pipeline-svg-title pipeline-svg-desc"><title id="pipeline-svg-title">'+escapeHtml(pipelineStages[phase][0])+' · scene composition</title><desc id="pipeline-svg-desc">Illustrative panel, two glyph runs, photo sprite, adjustment badge and path. Fine dashed bounds show the photo content mask. Heavy dashed outlines mark the selected primitive.</desc>'+defs+marks+mask+selection+'<text x="20" y="248" font-size="11" fill="#515151">SVG teaching preview · scale factor 1</text></svg>';
}
function pipelineRail(stage,oldStage,reducedMotion=false) {
  const x=20+stage*44, old=20+(oldStage??stage)*44;
  return '<svg class="pipeline-rail" viewBox="0 0 360 30" role="img" aria-label="Pipeline stage '+(stage+1)+' of 8"><path d="M20 15H328" stroke="#c1c1c1" stroke-width="3"/>'+pipelineStages.map((_,i)=>'<circle cx="'+(20+i*44)+'" cy="15" r="4" fill="#747474"/>').join('')+'<circle cx="'+x+'" cy="15" r="8" fill="#585858">'+(!reducedMotion && old!==x?'<animate attributeName="cx" from="'+old+'" to="'+x+'" dur="0.35s" fill="freeze"/>':'')+'</circle></svg>';
}
function pipelinePage() {
  return header('Interactive walkthrough / rendering & scene composition','From elements to pixels','Move a badge, change clipping and group a layer. Follow the same small scene from view geometry to ordered primitives, batches and backend work.')+
    '<div class="notice">This is a deterministic teaching model grounded in the captured Rust source. It is not executing GPUI or measuring GPU performance. It models selected primitive types; text and the photo are SVG illustrations, not GPUI glyph rasterization or a color-accurate photo pipeline.</div>'+
    '<div class="pipeline-toolbar"><button id="pipeline-prev" type="button">Previous</button><button id="pipeline-play" type="button">Play once</button><button id="pipeline-next" type="button">Next</button><button id="pipeline-reset" type="button">Reset</button></div>'+
    '<div class="pipeline-steps" aria-label="Pipeline stages">'+pipelineStages.map(([name],i)=>'<button type="button" id="pipeline-stage-'+i+'">'+(i+1)+'. '+name+'</button>').join('')+'</div><div id="pipeline-rail"></div>'+
    '<div class="pipeline-settings"><label>Badge horizontal position<input id="pipeline-overlap" type="range" min="60" max="232" step="1" value="190"></label><label><input id="pipeline-mask" type="checkbox" checked> Clip photo / token to content mask</label><label><input id="pipeline-layer" type="checkbox"> Explicit layer for badge + icon + label</label><label><input id="pipeline-atlas" type="checkbox"> Second title run uses another texture</label><label>Backend explanation<select id="pipeline-backend"><option value="metal">Apple / Metal</option><option value="wgpu">wgpu (including browser)</option></select></label></div>'+
    '<section id="pipeline-caption" aria-live="polite"></section><div class="pipeline-grid"><div><h2>Composition preview</h2><div id="pipeline-preview"></div><label>Inspect a primitive<select id="pipeline-select"></select></label><div id="pipeline-inspector"></div></div><div><h2 id="pipeline-data-heading">Scene data</h2><div id="pipeline-data"></div></div></div>'+
    '<h2>What to try</h2><ol><li>Select <strong>Batch</strong>. Toggle the second texture: one compatible glyph batch splits, even though the visible text does not change.</li><li>Move the badge horizontally. Watch its order change as it enters or leaves the photo bounds. Non-overlapping work may share an order.</li><li>Toggle the explicit layer. The badge quad, icon path and label now share the layer order. This is ordering/grouping, not automatically an offscreen framebuffer.</li><li>Disable clipping. The photo grows to its original bounds and the previously clipped-out token appears. Clipping affects order assignment as well as visible output.</li><li>Select <strong>Cached replay</strong>. Compare the operation stream with the type arrays: replay re-inserts operations, rather than copying a finished image.</li></ol>'+
    '<h2>Where the model meets real GPUI</h2><p>Window painting applies scale, pixel snapping, opacity and the current content mask before scene insertion. This demo fixes the scale at 1 and omits border splitting, complex paths, shadows, subpixel text, surfaces and nested layers. It replaces the BoundsTree spatial index with a linear overlap scan for these few rectangles, using the same maximum-overlap ordering rule.</p><p>Scene::finish sorts per-type arrays. BatchIterator merges their heads by order and PrimitiveKind, limiting sprite batches by texture. A batch is not necessarily one render pass or one GPU draw call. Both Metal and wgpu have intermediate path-rasterization work. The backend selector changes the explanation only; the SVG output is the same.</p><p>For a custom shader, decide whether you need an existing custom element paint path, a new shared scene primitive, or backend-specific rendering work. For wide-gamut photo output, image interpretation, shader math, intermediate formats, blending and native surface/display configuration all need a defined contract. This illustration deliberately makes no color-management claim.</p>'+
    '<p class="evidence">Source trail: '+guideSource('scene.rs',89,'Insertion and clipping')+' · '+guideSource('bounds_tree.rs',126,'Maximum-overlap order')+' · '+guideSource('scene.rs',151,'Finish sorting')+' · '+guideSource('scene.rs',290,'Batch merge')+' · '+guideSource('scene.rs',137,'Replay')+' · '+guideSource('window.rs',4533,'Explicit paint layers')+' · '+guideSource('metal_renderer.rs',712,'Metal batch consumption','gpui-apple')+' · '+guideSource('wgpu_renderer.rs',1518,'wgpu batch consumption','gpui-wgpu')+'</p>';
}
function pipelineRectText(b) {return '('+b.x+', '+b.y+') '+b.w+' × '+b.h;}
function paintPipeline() {
  const state=pipelineState, model=composePipeline(state), el=id=>document.getElementById(id);
  const reduced=window.matchMedia?.('(prefers-reduced-motion: reduce)')?.matches || false;
  el('pipeline-preview').innerHTML=pipelineSvg(model,state,pipelinePrevious?.model,reduced);
  el('pipeline-rail').innerHTML=pipelineRail(state.stage,pipelinePrevious?.stage,reduced);
  el('pipeline-caption').innerHTML='<h2>'+escapeHtml(pipelineStages[state.stage][0])+'</h2><p>'+escapeHtml(pipelineStages[state.stage][1])+'</p>';
  for(let i=0;i<pipelineStages.length;i++) {el('pipeline-stage-'+i).setAttribute('aria-pressed',String(i===state.stage));}
  el('pipeline-prev').disabled=state.stage===0;el('pipeline-next').disabled=state.stage===7;
  el('pipeline-select').innerHTML=model.inputs.map(p=>'<option value="'+p.id+'" '+(p.id===state.selected?'selected':'')+'>'+p.id+' · '+p.kind+'</option>').join('');
  el('pipeline-overlap').value=state.badgeX;el('pipeline-mask').checked=state.mask;el('pipeline-layer').checked=state.layer;el('pipeline-atlas').checked=state.atlas;el('pipeline-backend').value=state.backend;
  const selected=model.primitives.find(p=>p.id===state.selected), rejected=model.rejected.find(p=>p.id===state.selected), item=selected||rejected;
  el('pipeline-inspector').innerHTML='<p><strong>'+escapeHtml(item.id)+'</strong> · '+escapeHtml(item.kind)+'</p><dl><dt>Original bounds</dt><dd>'+pipelineRectText(item.bounds)+'</dd><dt>Mask intersection</dt><dd>'+pipelineRectText(item.clipped)+'</dd><dt>Order</dt><dd>'+(selected?selected.order+(selected.layer?' (explicit layer)':''):'Rejected: empty intersection')+'</dd>'+(item.texture!==undefined?'<dt>Texture / tile</dt><dd>'+item.texture+' / '+item.tile+'</dd>':'')+'</dl><p class="muted">Labels here describe the model even before the paint stage.</p>';
  const esc=escapeHtml;
  let heading='Element responsibilities', body;
  if(state.stage<2) body='<ul><li>Panel → background Quad</li><li>Title → two representative glyph-run sprites</li><li>Photo → PolychromeSprite + mask</li><li>Adjustment badge → Quad + Path + glyph sprite</li></ul><p>These mappings are illustrative. In real text, glyph count and atlas use depend on shaping and rasterization. Element count is not primitive count.</p><p>Layout fixes geometry; prepaint fixes hit-test state. Primitive insertion starts in the next stage.</p>';
  else if(state.stage===2 || state.stage===7) {heading=state.stage===7?'Replayable paint operations':'Insertion / paint operations';body='<ol class="pipeline-log">'+model.operations.map(op=>'<li>'+(op.op==='Primitive'?'<code>'+esc(op.primitive.id)+'</code> → '+esc(op.primitive.kind)+' · order '+op.primitive.order:esc(op.op)+(op.order?' · order '+op.order:''))+'</li>').join('')+'</ol><p>'+model.inputs.length+' attempted primitives → '+model.primitives.length+' accepted · '+model.rejected.length+' rejected. Scene::len would count '+model.operations.length+' paint operations, including layer markers.</p>'+(state.stage===7?'<p>This unchanged example replays the full operation stream into a fresh scene. In GPUI, a cached view can replay a selected range and receive new ordering relative to its surroundings. The view invalidation decision is outside this model.</p>':'');}
  else if(state.stage===3) {heading='Sorted, type-specific arrays';body=pipelineKinds.filter(k=>model.arrays[k].length).map(k=>'<h3>'+k+'</h3><p>'+model.arrays[k].map(p=>'<code>'+p.id+' [order '+p.order+(p.tile!==undefined?', tile '+p.tile:'')+']</code>').join(' → ')+'</p>').join('');}
  else if(state.stage===4) {heading='Batch iterator output';body='<ol class="pipeline-log">'+model.batches.map(b=>'<li><strong>'+b.kind+'</strong>'+(b.texture!==undefined?' · texture '+b.texture:'')+'<br><code>'+b.items.map(p=>p.id+' @'+p.order).join(', ')+'</code></li>').join('')+'</ol><p>'+model.batches.length+' illustrative batches. Count is not GPU draw-call count.</p>';}
  else {heading=state.stage===5?'Backend work: '+(state.backend==='metal'?'Metal':'wgpu'):'Presented composition';body='<ol class="pipeline-log"><li>Prepare/upload primitive instance data and referenced resources.</li><li>Consume scene.batches() in order.</li><li>Quads and sprites use their matching pipelines and texture bindings.</li><li>For Paths: rasterize into an intermediate, then composite into the destination. '+(state.backend==='metal'?'Metal ends and resumes command encoding around the path work.':'wgpu drops and resumes its main render pass around the path work.')+'</li><li>Finish command submission and platform presentation.</li></ol><p>The rendered-frame scene crosses PlatformWindow::draw. This SVG illustrates the resulting composition; no native renderer or GPU is running here.</p>';}
  el('pipeline-data-heading').textContent=heading;el('pipeline-data').innerHTML=body;
  pipelinePrevious={model,stage:state.stage};
}
function stopPipeline() {if(pipelineTimer!==null){clearInterval(pipelineTimer);pipelineTimer=null;}const button=document.getElementById('pipeline-play');if(button)button.textContent='Play once';}
function bindPipeline() {
  pipelinePrevious=null;
  const el=id=>document.getElementById(id);
  for(let i=0;i<pipelineStages.length;i++) el('pipeline-stage-'+i).onclick=()=>{stopPipeline();pipelineState.stage=i;paintPipeline();};
  el('pipeline-prev').onclick=()=>{stopPipeline();pipelineState.stage=Math.max(0,pipelineState.stage-1);paintPipeline();};
  el('pipeline-next').onclick=()=>{stopPipeline();pipelineState.stage=Math.min(7,pipelineState.stage+1);paintPipeline();};
  el('pipeline-play').onclick=()=>{if(pipelineTimer!==null){stopPipeline();return;}pipelineState.stage=0;paintPipeline();el('pipeline-play').textContent='Pause';pipelineTimer=setInterval(()=>{pipelineState.stage++;paintPipeline();if(pipelineState.stage===7)stopPipeline();},1400);};
  el('pipeline-reset').onclick=()=>{stopPipeline();pipelineState={stage:0,badgeX:190,mask:true,layer:false,atlas:false,backend:'metal',selected:'photo'};paintPipeline();};
  for(const [id,key] of [['pipeline-mask','mask'],['pipeline-layer','layer'],['pipeline-atlas','atlas']]) el(id).onchange=()=>{stopPipeline();pipelineState[key]=el(id).checked;paintPipeline();};
  el('pipeline-overlap').oninput=()=>{stopPipeline();pipelineState.badgeX=Number(el('pipeline-overlap').value);paintPipeline();};
  el('pipeline-select').onchange=()=>{pipelineState.selected=el('pipeline-select').value;paintPipeline();};
  el('pipeline-backend').onchange=()=>{pipelineState.backend=el('pipeline-backend').value;paintPipeline();};
  paintPipeline();
}
