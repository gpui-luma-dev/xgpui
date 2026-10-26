import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';

const makeCode = language => {
  const classes = new Set([language]);
  return {dataset:{}, classList:{
    [Symbol.iterator]:()=>classes.values(),
    remove:value=>classes.delete(value), add:value=>classes.add(value)
  }};
};
const rust = makeCode('language-rust,ignore');
const toml = makeCode('language-toml');
const highlighted = [];
const context = vm.createContext({window:{Prism:{languages:{rust:{}},
  highlightElement:code=>highlighted.push(code)}}});
vm.runInContext(await readFile(new URL('code-examples.js', import.meta.url), 'utf8'), context);
const root = {querySelectorAll:()=>[rust,toml].map(code=>({querySelector:()=>code}))};
context.window.enhanceCodeExamples(root);
context.window.enhanceCodeExamples(root);
assert.deepEqual([...rust.classList], ['language-rust']);
assert.deepEqual([...toml.classList], ['language-toml']);
assert.deepEqual(highlighted, [rust]);
console.log('Verified Rust-only highlighting and repeated page updates.');
