// Node adapter for the existing browser worker test; not imported by the app.
import { parentPort } from 'node:worker_threads';
import { readFile } from 'node:fs/promises';

const browserFetch = globalThis.fetch;
globalThis.fetch = async (input, options) => {
  const url = input instanceof Request ? input.url : String(input);
  if (url.startsWith('file:')) {
    return new Response(await readFile(new URL(url)), { headers: { 'Content-Type': 'application/wasm' } });
  }
  return browserFetch(input, options);
};
globalThis.self = globalThis;
globalThis.postMessage = (message, transfer) => parentPort.postMessage(message, transfer);
await import('./document-worker.js');
parentPort.on('message', data => globalThis.onmessage({ data }));
