// Runs in the document worker. The response port never passes through GPUI's
// event loop. One preview may be installing while this worker reads/formats the
// next bounded batch; only a full backlog applies backpressure to the reader.
export async function receiveResponse(port, processor, publish) {
  let pendingRead = null;
  let preview = [];
  let length = 0;
  let installing = null;
  let lastPreview = -Infinity;
  let formattingElapsed = 0;
  let timer = null;
  let receiving = true;
  let bytes = 0;
  port.onmessage = ({ data }) => {
    const pending = pendingRead;
    pendingRead = null;
    if (!pending) return;
    if (data.error) pending.reject(new Error(data.error));
    else pending.resolve(data);
  };
  port.onmessageerror = () => {
    pendingRead?.reject(new Error('The response chunk could not be received'));
    pendingRead = null;
  };
  function flush() {
    clearTimeout(timer);
    timer = null;
    if (!length) return;
    installing = publish(preview.join(''), bytes);
    preview = [];
    length = 0;
    lastPreview = performance.now();
    const current = installing;
    current.then(() => {
      if (installing === current) installing = null;
      schedule();
    });
  }
  function schedule() {
    if (!receiving || installing || !length) return;
    if (length >= 128 * 1024 || performance.now() - lastPreview >= 100) flush();
    else if (!timer) timer = setTimeout(flush, 100 - (performance.now() - lastPreview));
  }
  try {
    while (true) {
      const next = await new Promise((resolve, reject) => {
        pendingRead = { resolve, reject };
        port.postMessage({ type: 'read' });
      });
      if (next.offset !== undefined && next.offset !== bytes) {
        throw new Error(`Response stream byte gap: expected ${bytes}, received ${next.offset}`);
      }
      if (next.done) break;
      if (typeof next.chunk !== 'string' || next.chunk.length > 64 * 1024) {
        throw new Error('Invalid response stream chunk');
      }
      const start = performance.now();
      const formatted = processor.push_stream(next.chunk);
      bytes = Number.isSafeInteger(next.bytes) && next.bytes >= 0
        ? bytes + next.bytes : processor.stream_bytes();
      formattingElapsed += performance.now() - start;
      preview.push(formatted);
      length += formatted.length;
      // Keep at most one published batch and about 1 Mi UTF-16 units buffered
      // here (plus one formatted chunk). A slow canvas cannot queue the file.
      if (length >= 1024 * 1024) {
        const current = installing;
        await current;
        if (installing === current) installing = null;
      }
      schedule();
    }
    receiving = false;
    clearTimeout(timer);
    timer = null;
    if (length) {
      await installing;
      flush();
    }
    // The final batch and analysis result are ordered on the same worker port.
    // Start analysis immediately; it need not wait for the final preview paint.
    return { bytes, formattingElapsed };
  } finally {
    receiving = false;
    clearTimeout(timer);
    port.close();
  }
}
