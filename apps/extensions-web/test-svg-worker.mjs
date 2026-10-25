import assert from 'node:assert/strict';
import test from 'node:test';
import { createSvgWorkerClient } from './svg-worker-client.mjs';

function harness() {
  const workers = [];
  const client = createSvgWorkerClient(() => {
    const worker = {
      sent: [], stopped: false,
      postMessage(data, transfers) { this.sent.push({ data, transfers }); },
      terminate() { this.stopped = true; },
      finish(bytes = new Uint8Array([1, 2, 3, 4])) {
        this.onmessage({ data: { id: this.sent.at(-1).data.id, bytes, ms: 12 } });
      },
    };
    workers.push(worker);
    return worker;
  });
  const request = () => client.request(new Uint8Array([60, 115, 118, 103]), 1, 1, 0, 0, 1, 1);
  return { client, workers, request };
}

test('single flight transfers pixel results and only sends the latest queued request', async () => {
  const { client, workers, request } = harness();
  const first = request();
  const second = request();
  const superseded = assert.rejects(second, /Superseded/);
  const third = request();
  assert.equal(workers.length, 1);
  assert.equal(workers[0].sent.length, 1);
  assert.equal(workers[0].sent[0].transfers.length, 1);
  workers[0].finish();
  assert.deepEqual((await first).bytes, new Uint8Array([1, 2, 3, 4]));
  assert.equal(workers[0].sent.length, 2);
  assert.equal(workers[0].sent[1].data.id, 3);
  workers[0].finish();
  assert.equal((await third).ms, 12);
  await superseded;
  client.dispose();
});

test('gesture invalidation cancels queued and stale results but reuses a finished worker', async () => {
  const { client, workers, request } = harness();
  const first = assert.rejects(request(), /Superseded/);
  const second = assert.rejects(request(), /Superseded/);
  client.invalidate();
  workers[0].finish();
  await Promise.all([first, second]);
  assert.equal(workers[0].sent.length, 1);
  const latest = request();
  assert.equal(workers.length, 1);
  workers[0].finish();
  await latest;
  client.dispose();
});

test('a settled request restarts an obsolete busy worker instead of waiting behind it', async () => {
  const { client, workers, request } = harness();
  const first = assert.rejects(request(), /Superseded/);
  client.invalidate();
  const latest = request();
  assert.equal(workers[0].stopped, true);
  assert.equal(workers.length, 2);
  workers[0].finish(); // Late delivery from the previous worker is ignored.
  workers[1].finish(new Uint8Array([9, 8, 7, 6]));
  assert.deepEqual((await latest).bytes, new Uint8Array([9, 8, 7, 6]));
  await first;
  client.dispose();
});

test('worker failures reject all work and the next request starts a fresh worker', async () => {
  const { client, workers, request } = harness();
  const first = assert.rejects(request(), /broken/);
  const second = assert.rejects(request(), /broken/);
  workers[0].onerror({ message: 'broken' });
  await Promise.all([first, second]);
  assert.equal(workers[0].stopped, true);
  const retry = request();
  assert.equal(workers.length, 2);
  workers[0].onerror({ message: 'old worker event' });
  workers[1].finish();
  await retry;
  client.dispose();
});

test('a render error replaces the worker before dispatching queued work', async () => {
  const { client, workers, request } = harness();
  const failed = assert.rejects(request(), /bad SVG/);
  const latest = request();
  workers[0].onmessage({ data: { id: workers[0].sent[0].data.id, error: 'bad SVG' } });
  await failed;
  assert.equal(workers[0].stopped, true);
  assert.equal(workers.length, 2);
  workers[1].finish();
  await latest;
  client.dispose();
});

test('dispose stops active and queued jobs, and stale result IDs cannot resolve a job', async () => {
  const { client, workers, request } = harness();
  const first = assert.rejects(request(), /stopped/);
  const second = assert.rejects(request(), /stopped/);
  workers[0].onmessage({ data: { id: 999, bytes: new Uint8Array(4), ms: 0 } });
  assert.equal(workers[0].sent.length, 1);
  client.dispose();
  await Promise.all([first, second]);
  assert.equal(workers[0].stopped, true);
});
