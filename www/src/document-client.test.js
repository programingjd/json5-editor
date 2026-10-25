import { afterEach, mock, test } from 'node:test';
import assert from 'node:assert/strict';
import { DocumentClient } from './document-client.js';

const clients = [];
afterEach(() => { for (const client of clients.splice(0)) clearTimeout(client.timer); });

function harness() {
  const sent = [];
  const workers = [];
  const wasm = { open_prepared_json5_file: mock.fn(() => true), apply_syntax_result: mock.fn(),
    prepare_json5_file: mock.fn(() => true), advance_json5_file_preparation: mock.fn(() => 1),
    json5_file_preparation_progress: mock.fn(() => new Uint32Array([2, 2])),
    cancel_json5_file_preparation: mock.fn() };
  const onLoading = mock.fn();
  const onParsing = mock.fn();
  const client = new DocumentClient(wasm, mock.fn(), mock.fn(), {
    onLoading,
    onParsing,
    yieldToBrowser: () => Promise.resolve(),
    createWorker() {
      const worker = { postMessage(message) { sent.push(message); }, terminate: mock.fn() };
      workers.push(worker);
      return worker;
    },
  });
  clients.push(client);
  return { client, wasm, sent, workers, onLoading, onParsing };
}

function opened(id) {
  return { type: 'opened', id, name: 'example.json5', view: '{}',
    highlights: new Uint32Array(), folds: new Uint32Array(), brackets: new Uint32Array(),
    diagnostics: '[]', elapsed: 0 };
}

test('old file results cannot replace a newer file request', async () => {
  const { client, wasm, workers } = harness();
  client.openText('old.json', '{}');
  client.openText('new.json', '[]');
  await client.receive(opened(1), workers[0]);
  assert.equal(wasm.open_prepared_json5_file.mock.callCount(), 0);
  assert.equal(workers[0].terminate.mock.callCount(), 1);
  await client.receive(opened(2));
  assert.equal(client.active, 2);
  assert.equal(wasm.open_prepared_json5_file.mock.callCount(), 1);
});

test('typing during a parse is batched and stale syntax is discarded', async () => {
  const { client, wasm, sent, onParsing } = harness();
  client.openText('example.json', '{}');
  await client.receive(opened(1));
  client.edit({ revision: 1, edit: { replacement: 'a' } });
  client.flush();
  client.edit({ revision: 2, edit: { replacement: 'b' } });
  client.edit({ revision: 3, edit: { replacement: 'c' } });
  client.flush();
  assert.equal(sent.filter((message) => message.type === 'edit').length, 1);
  client.receive({ type: 'syntax', id: 1, revision: 1 });
  assert.deepEqual(onParsing.mock.calls.map(call => call.arguments), [[true]]);
  assert.equal(wasm.apply_syntax_result.mock.callCount(), 0);
  assert.partialDeepStrictEqual(sent.at(-1), { type: 'edit', fromRevision: 1, revision: 3,
    edits: [{ replacement: 'b' }, { replacement: 'c' }] });
  client.receive({ type: 'syntax', id: 1, revision: 3 });
  assert.equal(wasm.apply_syntax_result.mock.callCount(), 1);
  assert.deepEqual(onParsing.mock.calls.map(call => call.arguments), [[true], [false]]);
});

test('save stays between edits made before and after the save action', async () => {
  const { client, sent } = harness();
  client.openText('example.json', '{}');
  await client.receive(opened(1));
  client.edit({ revision: 1, edit: { replacement: 'before' } });
  client.save('saved.json5', 'keep');
  client.edit({ revision: 2, edit: { replacement: 'after' } });
  client.receive({ type: 'syntax', id: 1, revision: 1 });
  assert.partialDeepStrictEqual(sent.at(-1), { type: 'save', revision: 1, name: 'saved.json5' });
  client.receive({ type: 'saved', id: 1, name: 'saved.json5', content: 'before' });
  assert.deepEqual(client.download.mock.calls[0].arguments, ['saved.json5', 'before']);
  assert.partialDeepStrictEqual(sent.at(-1), { type: 'edit', fromRevision: 1, revision: 2 });
});
