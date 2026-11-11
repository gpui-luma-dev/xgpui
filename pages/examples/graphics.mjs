// A server bind address is not a browser loopback origin. Preserve the requested
// example/backend while moving wildcard HTTP URLs to the trustworthy local host.
export function canonicalLocalDemoUrl(href) {
  const url = new URL(href);
  if (url.protocol !== 'http:' || !['[::]', '0.0.0.0'].includes(url.hostname)) return null;
  url.hostname = 'localhost';
  return url.href;
}
// Shared startup diagnostics. Observe the renderer's context request; never probe
// its canvas, since creating another context can make WebGPU initialization fail.
export function webgpuPreflight(environment) {
  if (!environment.isSecureContext) {
    return 'WebGPU requires a secure context. Open this example over HTTPS or localhost.';
  }
  if (!environment.navigator?.gpu) {
    return 'This browser does not expose navigator.gpu. WebGPU is unavailable in this browser configuration; try Automatic or WebGL2.';
  }
  return null;
}
export function recoveryUrls(href) {
  const automatic = new URL(href);
  automatic.searchParams.delete('backend');
  const webgl = new URL(automatic);
  webgl.searchParams.set('backend', 'webgl');
  return { automatic: automatic.href, webgl: webgl.href };
}
export function monitorGraphics(environment = window) {
  const state = { context: 'not requested', available: !!environment.navigator?.gpu,
    secure: environment.isSecureContext, browser: environment.navigator?.userAgent ?? 'unknown' };
  const prototype = environment.HTMLCanvasElement.prototype;
  const original = prototype.getContext;
  prototype.getContext = function (kind, ...args) {
    const graphics = ['webgpu', 'webgl2', 'webgl'].includes(kind);
    try {
      const context = Reflect.apply(original, this, [kind, ...args]);
      if (graphics) state.context = `${kind}: ${context ? 'created' : 'returned null'}`;
      return context;
    } catch (error) {
      if (graphics) state.context = `${kind}: ${error?.message ?? error}`;
      throw error;
    }
  };
  return state;
}
export function showGraphicsFailure(container, error, diagnostics, href = location.href) {
  container.hidden = false;
  container.setAttribute('role', 'alert');
  const requested = new URL(href).searchParams.get('backend') ?? 'automatic';
  const message = document.createElement('p');
  message.textContent = `Unable to run example: ${error?.message ?? error}`;
  const detail = document.createElement('pre');
  detail.style.whiteSpace = 'pre-wrap';
  detail.textContent = `Requested backend: ${requested}\nSecure context: ${diagnostics.secure}\nWebGPU API exposed: ${diagnostics.available}\nCanvas context: ${diagnostics.context}\nBrowser: ${diagnostics.browser}`;
  const help = document.createElement('p');
  help.textContent = requested === 'webgpu'
    ? 'WebGPU was explicitly requested, so WebGL2 was not tried. Automatic tries WebGPU and then WebGL2 on a fresh canvas.'
    : 'Automatic tries WebGPU and then WebGL2 on a fresh canvas. The full startup error is also in the console.';
  const links = document.createElement('nav');
  const urls = recoveryUrls(href);
  for (const [text, url] of [['Try Automatic', urls.automatic], ['Try WebGL2', urls.webgl]]) {
    const link = document.createElement('a');
    link.textContent = text;
    link.href = url;
    link.style.marginRight = '1rem';
    links.append(link);
  }
  container.replaceChildren(message, detail, help, links);
}
