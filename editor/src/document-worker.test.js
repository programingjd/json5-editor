import { test } from 'node:test';
import assert from 'node:assert/strict';
import { Worker } from 'node:worker_threads';

test('large-file processing runs in the worker while the caller remains responsive', { timeout: 20000 }, async () => {
  const worker = new Worker(new URL('./document-worker.node.js', import.meta.url));
  const response = (request) => new Promise((resolve, reject) => {
    const onMessage = data => {
      if (data.type === 'progress') return;
      worker.off('message', onMessage);
      worker.off('error', onError);
      if (data.type === 'error') reject(new Error(data.message));
      else resolve(data);
    };
    const onError = error => { worker.off('message', onMessage); reject(error); };
    worker.on('message', onMessage);
    worker.once('error', onError);
    worker.postMessage(request);
  });
  const source = JSON.stringify({ entries: Array.from({ length: 20000 }, (_, index) => ({
    id: index, name: `entry-${index}`, enabled: true, value: [index, index + 1, index + 2],
  })) });
  let ticks = 0;
  const timer = setInterval(() => { ticks += 1; }, 1);
  try {
    const opened = await response({ type: 'open', id: 1, name: 'large.json', file: new File([source], 'large.json') });
    assert.equal(opened.type, 'opened');
    assert.ok(opened.highlights instanceof Uint32Array);
    assert.ok(opened.highlights.length > 100000);
    assert.ok(opened.folds.length > 40000);
    assert.ok(ticks > 5);
    const saved = await response({ type: 'save', id: 1, revision: 0, name: 'large.json', formatting: 'keep' });
    assert.equal(saved.content, source);
  } finally {
    clearInterval(timer);
    await worker.terminate();
  }
});
