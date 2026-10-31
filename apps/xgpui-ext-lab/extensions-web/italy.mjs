// Rust/Wasm supplies explicit sRGB previews; the browser presents original ICC JPEGs.
export async function mountItaly(bindings) {
  document.body.classList.add('italy-running');
  const root = document.createElement('main');
  root.id = 'italy-demo';
  root.innerHTML = `
    <h1>Italy — photographic gamut comparison</h1>
    <p>Select a thumbnail, then switch between <strong>Original / browser color</strong> and <strong>sRGB preview</strong>. Look at the orange/red umbrellas. The same selected photo is used in both modes.</p>
    <fieldset><legend>Presentation</legend>
      <label><input type="radio" name="italy-mode" value="original" checked> Original / browser color</label>
      <label><input type="radio" name="italy-mode" value="srgb"> sRGB preview</label>
    </fieldset>
    <p class="italy-limit">Original mode lets your browser interpret WebKit’s original JPEG profiles. sRGB preview uses Rust/Wasm to clip the prepared image to sRGB. This does not switch your monitor or a GPUI window’s color space. Differences depend on your display and browser; on an sRGB display they may be small.</p>
    <p id="italy-capability"></p>
    <div id="italy-thumbnails" aria-label="Choose image version"></div>
    <p id="italy-selection" role="status"></p>
    <img id="italy-main" width="600" height="400" alt="">
    <p>The “Out of sRGB areas” version is WebKit’s supplied highlighted photograph. Its diagnostic markings remain visible in either presentation mode. The sRGB and Display P3 source files are separate versions, not guaranteed identical after clipping.</p>
    <p><a href="https://webkit.org/blog-files/color-gamut/" target="_blank" rel="noopener">Italy images courtesy of WebKit’s Wide Gamut demo ↗</a> · <a href="italy/README.md">Asset provenance and preparation</a></p>
    <p>These browser image elements use the browser’s color management, independently of the sRGB GPUI WebGPU/WebGL2 canvas used by other demos. This example does not establish native GPUI wide-gamut output or calibrated monitor accuracy.</p>`;
  document.body.insertBefore(root, document.getElementById('back'));
  const files = ['Italy-sRGB.jpg', 'Italy-P3.jpg', 'Italy-oog.jpg'];
  const labels = ['sRGB', 'Display P3', 'Out of sRGB areas'];
  const originals = files.map(file => new URL(`italy/${file}`, location.href).href);
  // Yield between bounded 600×400 conversions so loading can paint.
  const previews = [];
  for (let i = 0; i < files.length; i++) {
    await new Promise(resolve => requestAnimationFrame(resolve));
    previews.push(URL.createObjectURL(new Blob([bindings.italy_srgb_preview(i)], {type: 'image/png'})));
  }
  let selection = 1;
  let standard = false;
  const main = root.querySelector('#italy-main');
  const thumbnails = root.querySelector('#italy-thumbnails');
  const buttons = labels.map((label, i) => {
    const button = document.createElement('button');
    button.type = 'button';
    button.innerHTML = `<img width="180" height="120" alt=""><span></span>`;
    button.querySelector('span').textContent = label;
    button.setAttribute('aria-label', `Show ${label}`);
    button.addEventListener('click', () => { selection = i; update(); });
    thumbnails.append(button);
    return button;
  });
  const update = () => {
    const sources = standard ? previews : originals;
    main.src = sources[selection];
    main.alt = `Italy umbrellas — ${labels[selection]} — ${standard ? 'sRGB preview' : 'original browser color'}`;
    root.querySelector('#italy-selection').textContent = `Currently showing: ${labels[selection]} · ${standard ? 'sRGB preview' : 'Original / browser color'}`;
    buttons.forEach((button, i) => {
      button.setAttribute('aria-pressed', String(i === selection));
      button.querySelector('img').src = sources[i];
    });
  };
  root.querySelectorAll('input[name="italy-mode"]').forEach(input => {
    input.addEventListener('change', () => { standard = input.value === 'srgb'; update(); });
  });
  const gamut = matchMedia('(color-gamut: p3)');
  const capability = () => {
    root.querySelector('#italy-capability').textContent = `Browser reports display gamut at least P3: ${gamut.matches ? 'yes' : 'no'}.`;
  };
  gamut.addEventListener('change', capability);
  window.addEventListener('pagehide', () => previews.forEach(url => URL.revokeObjectURL(url)), {once: true});
  main.addEventListener('error', () => {
    root.querySelector('#italy-selection').textContent = 'Unable to load the selected Italy image. Reload or check that the demo assets were built.';
  });
  capability();
  update();
  await main.decode();
}
