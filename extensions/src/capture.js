(() => {
  if (window !== window.top || !/^(?:text\/(?:plain|html)|(?:application|text)\/(?:[^;]+\+)?json5?)(?:;|$)/i.test(document.contentType)) return;
  const extension = globalThis.browser ?? globalThis.chrome;

  function waitForBody() {
    if (document.body) return Promise.resolve();
    return new Promise(resolve => {
      const observer = new MutationObserver(() => {
        if (document.body) { observer.disconnect(); resolve(); }
      });
      observer.observe(document, { childList: true, subtree: true });
    });
  }

  function domReader() {
    const offsets = new WeakMap();
    const queued = new WeakSet();
    let nodes = [];
    let index = 0;
    let source = null;
    let pending = null;
    let finished = document.readyState !== 'loading';
    let cancelled = false;

    function enqueue(node) {
      if (node.nodeType === Node.TEXT_NODE) {
        if (!queued.has(node)) { queued.add(node); nodes.push(node); }
      } else {
        const walker = document.createTreeWalker(node, NodeFilter.SHOW_TEXT);
        for (let text; (text = walker.nextNode());) enqueue(text);
      }
    }
    function collect(records) {
      if (!source) {
        source = document.querySelector('pre') ?? (finished ? document.body : null);
        if (!source) return;
        // Chromium still appends the original response to this node. Keep it
        // observable, but don't shape/wrap megabytes behind the editor iframe.
        if (source !== document.body) source.style.setProperty('display', 'none', 'important');
        observer.disconnect();
        observer.observe(source, { childList: true, characterData: true, subtree: true });
        enqueue(source);
      } else {
        for (const record of records) {
          if (record.type === 'characterData') enqueue(record.target);
          else for (const node of record.addedNodes) enqueue(node);
        }
      }
      flush();
    }
    function flush() {
      if (!pending) return;
      let chunk = '';
      while (index < nodes.length && chunk.length < 64 * 1024) {
        const node = nodes[index];
        const start = offsets.get(node) ?? 0;
        let end = Math.min(node.length, start + 64 * 1024 - chunk.length);
        if (end > start && (end < node.length || chunk.length + end - start === 64 * 1024
            || (!finished && index + 1 === nodes.length))) {
          const last = node.substringData(end - 1, 1).charCodeAt(0);
          if (last >= 0xd800 && last <= 0xdbff) end--;
        }
        if (end > start) chunk += node.substringData(start, end - start);
        offsets.set(node, end);
        if (end === node.length) { queued.delete(node); index++; }
        else break;
      }
      if (index > 1024) { nodes = nodes.slice(index); index = 0; }
      if (chunk || finished || cancelled) {
        const resolve = pending;
        pending = null;
        resolve(chunk ? { chunk, done: false } : { done: true });
      }
    }
    const observer = new MutationObserver(collect);
    observer.observe(document, { childList: true, subtree: true });
    collect([]);
    function finish() {
      finished = true;
      collect(observer.takeRecords());
    }
    document.addEventListener('DOMContentLoaded', finish, { once: true });
    return {
      read() {
        if (pending) throw new Error('A response read is already pending');
        return new Promise(resolve => { pending = resolve; flush(); });
      },
      cancel() { cancelled = true; this.close(); flush(); },
      close() { observer.disconnect(); document.removeEventListener('DOMContentLoaded', finish); },
      async download() {
        // The browser still owns the original response. Let it finish without
        // sending more text to the editor, including for POST/single-use URLs.
        if (document.readyState === 'loading') {
          await new Promise(resolve => document.addEventListener('DOMContentLoaded', resolve, { once: true }));
        }
        finish();
        if (!source) throw new Error('The original response is unavailable');
        return new Blob([source.textContent], { type: 'application/octet-stream' });
      },
    };
  }

  async function capture() {
    // A prerendered top-level page can have a nonzero frame ID. Wait until it
    // becomes the active document before requesting its response metadata.
    if (document.prerendering) {
      await new Promise(resolve =>
        document.addEventListener('prerenderingchange', resolve, { once: true }));
    }
    const result = await extension.runtime.sendMessage({
      type: 'json-response-inspect', contentType: document.contentType,
    });
    if (!result?.ok) throw new Error(result?.error ?? 'The extension could not inspect the response');
    const response = result.response;
    if (!response) return;
    await waitForBody();
    const reader = domReader();
    const viewerUrl = new URL(extension.runtime.getURL('index.html'));
    const viewerOrigin = extension.runtime.getURL('').replace(/\/$/, '');
    const token = Array.from(crypto.getRandomValues(new Uint8Array(16)), byte =>
      byte.toString(16).padStart(2, '0')).join('');
    viewerUrl.hash = `embedded=${token}`;
    const frame = document.createElement('iframe');
    frame.title = 'JSON5 Editor';
    frame.src = viewerUrl.href;
    if (!extension.runtime.getManifest().browser_specific_settings?.gecko) {
      frame.allow = 'clipboard-read; clipboard-write';
    }
    frame.referrerPolicy = 'no-referrer';
    Object.assign(frame.style, {
      position: 'fixed', inset: '0', width: '100%', height: '100%',
      border: '0', display: 'block', zIndex: '2147483647',
      colorScheme: 'normal',
    });

    window.addEventListener('message', event => {
      if (event.source !== frame.contentWindow || event.origin !== viewerOrigin
          || event.data?.type !== 'json5-editor-filename' || event.data.token !== token
          || typeof event.data.name !== 'string') return;
      response.name = event.data.name;
      document.title = `${response.name} — JSON5 Editor`;
    });

    frame.addEventListener('load', () => {
      const channel = new MessageChannel();
      const control = new MessageChannel();
      let busy = false;
      let closed = false;
      let downloading = false;
      function close() {
        closed = true;
        channel.port1.close();
        control.port1.close();
      }
      function cleanup() {
        reader.close();
        // Removing/reinserting the iframe would reload the editor.
        for (const child of [...document.body.childNodes]) {
          if (child !== frame) child.remove();
        }
        document.documentElement.style.overflow = 'hidden';
        document.body.style.margin = '0';
        document.title = downloading ? '' : `${response.name} — JSON5 Editor`;
      }
      control.port1.onmessage = ({ data }) => {
        if (downloading && data?.type === 'download-finished') {
          frame.remove();
          close();
          return;
        }
        if (downloading && data?.type === 'download-failed') {
          frame.style.visibility = 'visible';
          frame.inert = false;
          frame.focus();
          close();
          return;
        }
        if (closed) return;
        if (data?.type === 'cancel') {
          close();
          reader.cancel();
          window.stop();
          cleanup();
        } else if (data?.type === 'download') {
          // Stop parser delivery, but keep the response and control channel
          // alive until its unmodified source is ready for the browser download.
          closed = true;
          downloading = true;
          // Keep the source DOM until receipt finishes, and keep the hidden
          // viewer alive while its Blob URL is owned by the browser download.
          frame.style.visibility = 'hidden';
          frame.inert = true;
          document.title = '';
          channel.port1.close();
          void reader.download().then(blob => {
            cleanup();
            control.port1.postMessage({ type: 'download', blob });
          }, error => {
            cleanup();
            frame.style.visibility = 'visible';
            frame.inert = false;
            frame.focus();
            control.port1.postMessage({ type: 'download-error', error: error.message ?? String(error) });
            close();
          });
        } else if (data?.type === 'close') {
          cleanup();
          close();
        } else if (data?.type === 'ready') {
          document.title = `${response.name} — JSON5 Editor`;
          frame.focus();
        }
      };
      channel.port1.onmessage = async ({ data }) => {
        if (closed) return;
        if (data?.type === 'read' && !busy) {
          busy = true;
          try {
            const result = await reader.read();
            if (closed) return;
            channel.port1.postMessage(result);
            if (result.done || result.error) {
              // Commit or Abort may follow after final analysis. Keep the
              // original source and control port until that choice is made.
              channel.port1.close();
            }
          } catch (error) {
            if (closed) return;
            channel.port1.postMessage({ error: error.message ?? String(error) });
            reader.cancel();
            close();
          } finally { busy = false; }
        }
      };
      frame.contentWindow.postMessage({ type: 'json5-editor-stream', token,
        name: response.name, contentType: response.contentType, totalBytes: response.totalBytes },
        viewerOrigin, [channel.port2, control.port2]);
    }, { once: true });
    document.body.append(frame);
  }

  void capture().catch(error => console.error('JSON5 Editor response capture:', error));
})();
