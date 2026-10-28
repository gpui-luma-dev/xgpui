import assert from 'node:assert/strict';
import { webgpuPreflight, recoveryUrls, monitorGraphics } from './graphics.mjs';
assert.match(webgpuPreflight({isSecureContext:false,navigator:{gpu:{}}}), /HTTPS or localhost/);
assert.match(webgpuPreflight({isSecureContext:true,navigator:{}}), /navigator.gpu/);
assert.equal(webgpuPreflight({isSecureContext:true,navigator:{gpu:{}}}), null);
for (const root of ['https://xgpui.dev/', 'http://localhost:8000/', 'https://example.test/xgpui/']) {
  for (const route of ['examples/extensions/?demo=layers', 'examples/shaders/?example=minimal']) {
    const original = new URL(route + '&backend=webgpu&reload=123#sample', root);
    const urls = recoveryUrls(original.href);
    for (const [mode, href] of Object.entries(urls)) {
      const recovered = new URL(href);
      assert.equal(recovered.origin, original.origin);
      assert.equal(recovered.pathname, original.pathname);
      assert.equal(recovered.hash, original.hash);
      for (const key of ['demo', 'example']) assert.equal(recovered.searchParams.get(key), original.searchParams.get(key));
      assert.equal(recovered.searchParams.get('backend'), mode === 'webgl' ? 'webgl' : null);
    }
  }
}
let calls = 0;
class Canvas { getContext(kind) { calls++; if(kind === 'webgpu') return null; return {kind}; } }
const state = monitorGraphics({isSecureContext:true,navigator:{gpu:{},userAgent:'test'},HTMLCanvasElement:Canvas});
assert.equal(calls,0,'monitor must not lock a canvas by creating a probe context');
const canvas = new Canvas();
assert.equal(canvas.getContext('webgpu'),null);
assert.equal(state.context,'webgpu: returned null');
assert.equal(canvas.getContext('webgl2').kind,'webgl2');
assert.equal(state.context,'webgl2: created');
assert.equal(calls,2);
console.log('Verified WebGPU preflight, fresh-page recovery links and non-invasive context diagnostics.');

// Recovery text uses textContent, and links preserve the current example.
const { showGraphicsFailure } = await import('./graphics.mjs');
const makeNode = tag => ({ tag, style: {}, children: [], setAttribute(name,value) {this[name]=value;},
  append(node) {this.children.push(node);}, replaceChildren(...nodes) {this.children=nodes;} });
globalThis.document = {createElement: makeNode};
const panel = makeNode('div');
showGraphicsFailure(panel, new Error('<unavailable>'), {secure:true,available:false,context:'not requested',browser:'test'},
  'https://example.test/?example=minimal&backend=webgpu');
assert.equal(panel.hidden,false);
assert.equal(panel.role,'alert');
assert.match(panel.children[0].textContent, /<unavailable>/);
assert.match(panel.children[1].textContent, /WebGPU API exposed: false/);
assert.equal(new URL(panel.children[3].children[1].href).searchParams.get('example'),'minimal');
assert.equal(new URL(panel.children[3].children[1].href).searchParams.get('backend'),'webgl');
console.log('Verified visible error details and recovery controls.');

const { canonicalLocalDemoUrl } = await import('./graphics.mjs');
assert.equal(canonicalLocalDemoUrl('http://[::]:8000/examples/shaders/?example=composition&backend=webgpu#sample'),
  'http://localhost:8000/examples/shaders/?example=composition&backend=webgpu#sample');
assert.equal(canonicalLocalDemoUrl('http://0.0.0.0:8081/?demo=layers&backend=webgpu'),
  'http://localhost:8081/?demo=layers&backend=webgpu');
for (const href of ['http://localhost:8000/', 'http://127.0.0.1:8000/',
  'http://[::1]:8000/', 'https://example.test/', 'http://192.168.1.2:8000/']) {
  assert.equal(canonicalLocalDemoUrl(href),null,href);
}
console.log('Verified wildcard bind-address correction preserves forced WebGPU, paths, ports, queries and fragments.');
