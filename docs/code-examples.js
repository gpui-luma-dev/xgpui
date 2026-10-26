'use strict';

// Local Prism Rust highlighting, matching gpui-luma.dev. No code-block UI.
window.enhanceCodeExamples = function(root) {
  if (!root?.querySelectorAll) return;
  for (const pre of root.querySelectorAll('pre')) {
    let code = pre.querySelector('code');
    if (!code && pre.closest('.declaration')) {
      code = document.createElement('code');
      code.textContent = pre.textContent;
      code.className = 'language-rust';
      pre.replaceChildren(code);
    }
    if (!code || code.dataset.syntaxHighlighted) continue;
    const declared = [...code.classList].find(name => name.startsWith('language-'));
    if (declared?.slice(9).split(',')[0] !== 'rust') continue;
    // Rust documentation fence flags do not change the highlighting language.
    code.classList.remove(declared);
    code.classList.add('language-rust');
    if (window.Prism?.languages.rust) {
      window.Prism.highlightElement(code);
      code.dataset.syntaxHighlighted = 'true';
    }
  }
};
