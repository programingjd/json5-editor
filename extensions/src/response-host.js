async function downloadResponse(blob, name, onFinished) {
  const extension = globalThis.browser ?? globalThis.chrome;
  const url = URL.createObjectURL(blob);
  let id;
  const finished = new Set();
  const cleanup = () => {
    extension.downloads.onChanged.removeListener(onChanged);
    URL.revokeObjectURL(url);
  };
  const onChanged = change => {
    if (!['complete', 'interrupted'].includes(change.state?.current)) return;
    if (id === undefined) finished.add(change.id);
    else if (change.id === id) {
      cleanup();
      onFinished();
    }
  };
  extension.downloads.onChanged.addListener(onChanged);
  try {
    id = await extension.downloads.download({ url, filename: name });
    if (finished.has(id)) {
      cleanup();
      onFinished();
    }
  } catch (error) {
    cleanup();
    throw error;
  }
}

function downloadError(error) {
  console.error('JSON5 Editor response download:', error);
  const output = document.getElementById('error');
  output.textContent = `Unable to download the response\n${error.message ?? String(error)}`;
  output.style.display = 'grid';
}

export function createInitialDocument() {
  const token = new URLSearchParams(location.hash.slice(1)).get('embedded');
  const embedded = window !== window.top && /^[a-f\d]{32}$/.test(token ?? '');
  return embedded ? new Promise(resolve => {

    function receive(event) {
      if ((event.source !== null && event.source !== window.parent) || !/^https?:\/\//.test(event.origin)
          || event.data?.type !== 'json5-editor-stream' || event.data.token !== token
          || event.ports.length !== 2 || typeof event.data.name !== 'string') return;
      window.removeEventListener('message', receive);
      const port = event.ports[0];
      const control = event.ports[1];
      // Keep title updates alive after the completed stream's ports close.
      window.addEventListener('gpui-json5-filename', ({ detail: name }) => {
        window.parent.postMessage({ type: 'json5-editor-filename', token, name }, event.origin);
      });
      control.onmessage = ({ data }) => {
        if (data?.type === 'download' && data.blob instanceof Blob) {
          void downloadResponse(data.blob, event.data.name, () => {
            control.postMessage({ type: 'download-finished' });
            control.close();
          }).catch(error => {
            control.postMessage({ type: 'download-failed' });
            control.close();
            downloadError(error);
          });
        } else if (data?.type === 'download-error') {
          control.close();
          downloadError(new Error(data.error));
        }
      };
      let pending = null;
      let ended = false;
      let transferred = false;
      port.onmessage = ({ data }) => {
        if (!pending) return;
        const { resolve, reject } = pending;
        pending = null;
        if (data.error) { ended = true; port.close(); control.close(); reject(new Error(data.error)); }
        else { ended = !!data.done; if (ended) port.close(); resolve(data); }
      };
      document.title = `${event.data.name} — JSON5 Editor`;
      control.postMessage({ type: 'ready' });
      resolve({
        kind: 'stream', name: event.data.name,
        totalBytes: Number.isSafeInteger(event.data.totalBytes) && event.data.totalBytes >= 0
          ? event.data.totalBytes : undefined,
        takePort() {
          if (transferred || pending || ended) throw new Error('The response stream is already in use');
          transferred = true;
          port.onmessage = null;
          return port;
        },
        read() {
          if (transferred) return Promise.reject(new Error('The response stream belongs to the document worker'));
          if (ended) return Promise.resolve({ done: true });
          if (pending) return Promise.reject(new Error('A response read is already pending'));
          return new Promise((resolve, reject) => { pending = { resolve, reject }; port.postMessage({ type: 'read' }); });
        },
        cancel({ download = false } = {}) {
          ended = true;
          control.postMessage({ type: download ? 'download' : 'cancel' });
          if (!download) control.close();
          if (!transferred) port.close();
          pending?.reject(new DOMException('Response loading was aborted', 'AbortError'));
          pending = null;
        },
        close() { control.postMessage({ type: 'close' }); control.close(); },
      });
    }
    window.addEventListener('message', receive);
  }) : Promise.resolve(null);
}
