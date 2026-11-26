'use strict';

// Local Prism Rust highlighting, matching xgpui-luma.dev. No code-block UI.
// Fence flags such as `rust,ignore` do not change the highlighting language.
for (const code of document.querySelectorAll('pre > code')) {
  const declared = [...code.classList].find(name => name.startsWith('language-'));
  if (declared?.slice(9).split(',')[0] !== 'rust' || !window.Prism?.languages.rust) continue;
  code.classList.remove(declared);
  code.classList.add('language-rust');
  window.Prism.highlightElement(code);
}

// Line marks: <pre data-highlight="3,10-14"> marks those 1-based lines, after
// syntax highlighting. A line whose markup doesn't close on the same line (a
// token spanning lines) is left unmarked rather than broken.
for (const pre of document.querySelectorAll('pre[data-highlight]')) {
  const code = pre.querySelector(':scope > code');
  if (!code) continue;
  const marked = new Set();
  for (const part of pre.dataset.highlight.split(',')) {
    const [start, end = start] = part.trim().split('-').map(Number);
    for (let line = start; line <= end; line++) marked.add(line);
  }
  const opens = line => (line.match(/<span\b/g) || []).length;
  const closes = line => (line.match(/<\/span>/g) || []).length;
  // A marked line is a block, so it ends the line itself: no newline follows.
  const lines = code.innerHTML.split('\n');
  code.innerHTML = lines
    .map((line, index) => {
      const last = index === lines.length - 1;
      if (marked.has(index + 1) && opens(line) === closes(line)) {
        return `<span class="code-highlight">${line || ' '}</span>`;
      }
      return last ? line : `${line}\n`;
    })
    .join('');
}
