import init, { DocumentProcessor } from './worker-wasm/gpui_json5_processor.js';
import { receiveResponse } from './response-stream.js';
import { StreamPreviewBudget } from './stream-preview-budget.js';

const ready = init({ module_or_path: new URL('./worker-wasm/gpui_json5_processor_bg.wasm', import.meta.url) });
const documents = new Map();

function sendSyntax(type, id, revision, result, extra = {}) {
  const highlights = result.highlights();
  const folds = result.folds();
  const brackets = result.brackets();
  const diagnostics = result.diagnostics();
  result.free();
  self.postMessage({ type, id, revision, highlights, folds, brackets, diagnostics, ...extra },
    [highlights.buffer, folds.buffer, brackets.buffer]);
}

function release(id) {
  documents.get(id)?.processor.free();
  documents.delete(id);
}

async function readDocument(message) {
  const { id, file } = message;
  if (!file) {
    const bytes = new TextEncoder().encode(message.contents).byteLength;
    self.postMessage({ type: 'progress', id, stage: 'processing', bytes, totalBytes: bytes });
    return message.contents;
  }
  const reader = file.stream().getReader();
  const decoder = new TextDecoder();
  const parts = [];
  let bytes = 0;
  let lastProgress = performance.now();
  self.postMessage({ type: 'progress', id, stage: 'reading', bytes, totalBytes: file.size });
  try {
    while (true) {
      const next = await reader.read();
      if (next.done) break;
      bytes += next.value.byteLength;
      parts.push(decoder.decode(next.value, { stream: true }));
      if (performance.now() - lastProgress >= 100) {
        self.postMessage({ type: 'progress', id, stage: 'reading', bytes, totalBytes: file.size });
        lastProgress = performance.now();
      }
    }
    parts.push(decoder.decode());
  } finally {
    reader.releaseLock();
  }
  self.postMessage({ type: 'progress', id, stage: 'processing', bytes, totalBytes: file.size });
  return parts.join('');
}

async function process(message) {
  await ready;
  const { type, id } = message;
  if (type === 'stream-start') {
    const document = { processor: new DocumentProcessor(), revision: 0, streaming: true,
      started: performance.now(), name: message.name, bytes: 0, previewBudget: new StreamPreviewBudget() };
    documents.set(id, document);
    if (message.port) {
      const received = await receiveResponse(message.port, document.processor, (preview, bytes) => {
        const visible = document.previewBudget.take(preview);
        if (!visible) {
          // Receipt must not wait for GPUI after the initial preview is full.
          self.postMessage({ type: 'progress', id, stage: 'receiving', bytes });
          return Promise.resolve();
        }
        return new Promise(resolve => {
          document.previewAccepted = resolve;
          self.postMessage({ type: 'stream-preview', id, preview: visible, bytes });
        });
      });
      document.bytes = received.bytes;
      document.formattingElapsed = received.formattingElapsed;
      finishStream(id, document);
      return;
    }
    self.postMessage({ type: 'stream-ready', id });
    return;
  }
  if (type === 'open') {
    const start = performance.now();
    const contents = await readDocument(message);
    const processor = new DocumentProcessor();
    documents.set(id, { processor, revision: 0 });
    const result = processor.open(contents);
    sendSyntax('opened', id, 0, result, { name: message.name, view: processor.view(),
      original: new Blob([contents], { type: 'text/plain;charset=utf-8' }),
      elapsed: performance.now() - start });
    return;
  }
  if (type === 'release') { release(id); return; }
  if (type === 'commit') {
    for (const other of documents.keys()) if (other !== id) release(other);
    return;
  }
  const document = documents.get(id);
  if (!document) return;
  if (type === 'stream-chunk') {
    if (!document.streaming || typeof message.chunk !== 'string' || message.chunk.length > 64 * 1024) {
      throw new Error('Invalid response stream chunk');
    }
    const preview = document.previewBudget.take(document.processor.push_stream(message.chunk));
    document.bytes = Number.isSafeInteger(message.bytes) && message.bytes >= 0
      ? document.bytes + message.bytes : document.processor.stream_bytes();
    self.postMessage({ type: 'stream-chunk-received', id, bytes: document.bytes, preview });
  } else if (type === 'stream-end') {
    finishStream(id, document);
  } else if (type === 'edit') {
    if (message.fromRevision !== document.revision) throw new Error('Document worker edit revision mismatch');
    const result = document.processor.apply_edits(JSON.stringify(message.edits));
    document.revision = message.revision;
    sendSyntax('syntax', id, document.revision, result);
  } else if (type === 'save') {
    if (message.revision !== document.revision) throw new Error('Document worker save revision mismatch');
    const formatting = message.formatting;
    let content = document.processor.save_formatted(message.convert_to_json ? 'keep' : formatting);
    if (message.convert_to_json) {
      // Already-valid JSON needs no syntax conversion. Preserve its exact
      // formatting when requested instead of serializing it again.
      let keep = false;
      if (formatting === 'keep') {
        try { JSON.parse(content); keep = true; } catch { /* JSON5 needs conversion. */ }
      }
      if (!keep) content = document.processor.convert_to_json(formatting === 'minimize');
    }
    self.postMessage({ type: 'saved', id, name: message.name,
      content });
  } else if (type === 'convert') {
    if (message.revision !== document.revision) throw new Error('Document worker conversion revision mismatch');
    const content = document.processor.convert_to_json(false);
    self.postMessage({ type: 'converted', id, revision: document.revision, content });
  }
}

function finishStream(id, document) {
  if (!document.streaming) throw new Error('The response stream is already complete');
  document.streaming = false;
  self.postMessage({ type: 'progress', id, stage: 'processing', bytes: document.bytes });
  const analysisStarted = performance.now();
  const result = document.processor.finish_stream();
  sendSyntax('opened', id, 0, result, { name: document.name, view: document.processor.view(),
    original: new Blob([document.processor.save(false)], { type: 'text/plain;charset=utf-8' }),
    elapsed: performance.now() - document.started, receivedElapsed: analysisStarted - document.started,
    analysisElapsed: performance.now() - analysisStarted, formattingElapsed: document.formattingElapsed });
}

// Serialize asynchronous file reads and edit/save barriers. At most one parse
// is in flight from the UI; edits typed during it are combined into the next batch.
let messages = Promise.resolve();
self.onmessage = ({ data }) => {
  // Preview credits must bypass the ordered document-operation queue: the
  // receiving operation may be awaiting this credit while keeping its parser.
  if (data.type === 'stream-preview-accepted') {
    const document = documents.get(data.id);
    document?.previewAccepted?.();
    if (document) document.previewAccepted = null;
    return;
  }
  messages = messages.then(() => process(data)).catch((error) => {
    self.postMessage({ type: 'error', id: data.id, operation: data.type, message: error?.message ?? String(error) });
  });
};
