// One running render and at most one latest queued render. Never render on the UI
// thread as a fallback. Each worker owns its Wasm memory and parsed document.
export function createSvgWorkerClient(factory = () =>
  new Worker(new URL('./svg-worker.mjs', import.meta.url), { type: 'module' })) {
  let worker, active, queued, serial = 0, timeout;
  const reject = (job, message) => job?.reject(new Error(message));
  function stop(message) {
    clearTimeout(timeout);
    worker?.terminate();
    worker = undefined;
    reject(active, message);
    reject(queued, message);
    active = queued = undefined;
  }
  function send(job) {
    if (!worker) {
      worker = factory();
      const source = worker;
      worker.onmessage = ({ data }) => {
        if (worker !== source || !active || data.id !== active.data.id) return;
        clearTimeout(timeout);
        const completed = active;
        active = undefined;
        if (completed.obsolete) reject(completed, 'Superseded SVG request');
        else if (data.error) reject(completed, data.error);
        else completed.resolve({ bytes: data.bytes, ms: data.ms });
        // Even an obsolete render's trap can leave its Wasm instance unusable.
        if (data.error) { source.terminate(); worker = undefined; }
        if (queued) {
          const next = queued;
          queued = undefined;
          try { send(next); } catch (error) { next.reject(error); stop(String(error)); }
        }
      };
      worker.onerror = event => {
        if (worker === source) stop(event.message || 'SVG worker failed');
      };
    }
    active = job;
    timeout = setTimeout(() => stop('SVG worker timed out after 30 seconds'), 30000);
    try { worker.postMessage(job.data, [job.data.svg.buffer]); }
    catch (error) { stop(String(error)); }
  }
  return {
    request(svg, width, height, x, y, viewWidth, viewHeight) {
      return new Promise((resolve, rejectJob) => {
        const job = { resolve, reject: rejectJob, data: {
          id: ++serial, svg: new Uint8Array(svg), width, height, x, y, viewWidth, viewHeight,
        }};
        // A newly settled camera must not wait behind obsolete synchronous work.
        // Recreate only in that case; normally the worker/document stay warm.
        if (active?.obsolete) stop('Superseded SVG request');
        if (active) {
          reject(queued, 'Superseded SVG request');
          queued = job;
        } else {
          try { send(job); } catch (error) { rejectJob(error); stop(String(error)); }
        }
      });
    },
    // A running render may finish during movement, but cannot publish its result.
    // If it is still running at the next settled request, terminate and restart.
    invalidate() {
      if (active) active.obsolete = true;
      reject(queued, 'Superseded SVG request');
      queued = undefined;
    },
    dispose() { stop('SVG worker stopped'); },
  };
}
globalThis.xgpuiSvgWorker = createSvgWorkerClient();
