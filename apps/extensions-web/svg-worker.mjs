// A separate Wasm instance, with no shared memory or main-thread SVG rendering.
import init, { rasterize_svg_region } from './extensions_web.js';
const ready = init();
self.onmessage = async ({ data }) => {
  try {
    await ready;
    const start = performance.now();
    const bytes = rasterize_svg_region(data.svg, data.width, data.height,
      data.x, data.y, data.viewWidth, data.viewHeight);
    const ms = performance.now() - start;
    self.postMessage({ id: data.id, bytes, ms }, [bytes.buffer]);
  } catch (error) {
    self.postMessage({ id: data.id, error: String(error?.message ?? error) });
  }
};
