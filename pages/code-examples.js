'use strict';

// Local Prism Rust highlighting, matching gpui-luma.dev. No code-block UI.
// Fence flags such as `rust,ignore` do not change the highlighting language.
for (const code of document.querySelectorAll('pre > code')) {
  const declared = [...code.classList].find(name => name.startsWith('language-'));
  if (declared?.slice(9).split(',')[0] !== 'rust' || !window.Prism?.languages.rust) continue;
  code.classList.remove(declared);
  code.classList.add('language-rust');
  window.Prism.highlightElement(code);
}
