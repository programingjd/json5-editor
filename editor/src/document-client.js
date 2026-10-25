import { yieldToBrowser } from './cooperative.js';

export class DocumentClient {
  constructor(wasm, download, reportError, options = {}) {
    this.wasm = wasm;
    this.download = download;
    this.reportError = reportError;
    this.onLoading = options.onLoading ?? (() => {});
    this.onParsing = options.onParsing ?? (() => {});
    this.onSaved = options.onSaved ?? (() => {});
    this.onSaveError = options.onSaveError ?? this.reportError;
    this.onConverted = options.onConverted ?? (() => {});
    this.yieldToBrowser = options.yieldToBrowser ?? yieldToBrowser;
    this.newWorker = options.createWorker ?? (() =>
      new Worker(new URL('./document-worker.js', import.meta.url), { type: 'module' }));
    this.worker = null;
    this.pendingLoad = null;
    this.originalDocument = null;
    this.active = 0;
    this.nextId = 0;
    this.revision = 0;
    this.parsedRevision = 0;
    this.sentRevision = 0;
    this.busy = false;
    this.parsing = false;
    this.conversionPending = false;
    this.queue = [];
    this.timer = null;
  }

  createWorker() {
    const worker = this.newWorker();
    worker.onmessage = ({ data }) => this.receive(data, worker);
    worker.onerror = (event) => {
      if (worker === this.pendingLoad?.worker) this.abortOpen();
      else if (worker !== this.worker) return;
      else {
        this.setParsing(false);
        this.finishConversion(this.sentRevision, '', `Document worker failed: ${event.message}`);
      }
      this.reportError(new Error(`Document worker failed: ${event.message}`));
    };
    return worker;
  }

  openFile(file) { this.open({ name: file.name, file }); }
  openText(name, contents) { this.open({ name, contents }); }

  revert() {
    if (this.originalDocument) this.open(this.originalDocument);
  }

  openStream(stream) {
    if (this.active) throw new Error('A response stream can only be the startup document');
    this.abortOpen();
    if (!this.wasm.begin_streaming_document(stream.name)) throw new Error('The editor is not ready for the response');
    const id = ++this.nextId;
    const worker = this.createWorker();
    this.pendingLoad = { id, worker, name: stream.name, prepared: null, stream, started: performance.now(),
      preview: [], previewLength: 0, lastPreview: 0, previewTimer: null, previewElapsed: 0, bytes: 0,
      totalBytes: stream.totalBytes, acknowledge: null };
    this.loading('receiving');
    try {
      const port = stream.takePort?.();
      worker.postMessage({ type: 'stream-start', id, name: stream.name, port }, port ? [port] : []);
    } catch (error) { this.abortOpen(); throw error; }
  }

  async pumpStream(load) {
    try {
      while (this.pendingLoad === load) {
        const next = await load.stream.read();
        if (this.pendingLoad !== load) return;
        if (next.error) throw new Error(next.error);
        if (next.offset !== undefined && next.offset !== load.bytes) {
          throw new Error(`Response stream byte gap: expected ${load.bytes}, received ${next.offset}`);
        }
        if (next.done) {
          this.previewStream(load);
          this.loading('processing');
          console.info(`[JSON5 stream] response complete: ${load.bytes} bytes; starting final analysis`);
          load.worker.postMessage({ type: 'stream-end', id: load.id });
          return;
        }
        if (typeof next.chunk !== 'string' || next.chunk.length > 64 * 1024) throw new Error('Invalid response chunk');
        const accepted = await new Promise((resolve, reject) => {
          load.acknowledge = { resolve, reject };
          load.worker.postMessage({ type: 'stream-chunk', id: load.id, chunk: next.chunk, bytes: next.bytes });
        });
        if (this.pendingLoad !== load) return;
        load.preview.push(accepted.preview);
        load.previewLength += accepted.preview.length;
        // Bounded UI batches. The transport pulls again only after the worker
        // has accepted the previous chunk; no full-buffer replacements occur.
        if (performance.now() - load.lastPreview >= 100 || load.previewLength >= 1024 * 1024) {
          this.previewStream(load);
          await new Promise(resolve => setTimeout(resolve, 0));
        } else if (!load.previewTimer) {
          load.previewTimer = setTimeout(() => {
            if (this.pendingLoad !== load) return;
            try { this.previewStream(load); }
            catch (error) { this.abortOpen(); this.reportError(error); }
          }, Math.max(0, 100 - (performance.now() - load.lastPreview)));
        }
      }
    } catch (error) {
      if (this.pendingLoad !== load) return;
      this.abortOpen();
      this.reportError(error);
    }
  }

  previewStream(load) {
    clearTimeout(load.previewTimer);
    load.previewTimer = null;
    if (!load.previewLength) return;
    const started = performance.now();
    if (!this.wasm.append_streaming_document(load.preview.join(''))) throw new Error('The response preview is unavailable');
    load.previewElapsed += performance.now() - started;
    load.preview = [];
    load.previewLength = 0;
    load.lastPreview = performance.now();
    this.loading('receiving');
  }

  open(document) {
    this.abortOpen();
    const id = ++this.nextId;
    const worker = this.createWorker();
    this.pendingLoad = { id, worker, name: document.name, prepared: null, bytes: 0,
      totalBytes: document.file?.size };
    this.loading('starting');
    // Loading gets its own worker: Abort can terminate synchronous WASM work
    // without destroying the current document's source projection or edits.
    worker.postMessage({ type: 'open', id, ...document });
    this.flush();
  }

  loading(stage, abortable = true) {
    const load = this.pendingLoad;
    if (!load) return;
    const layout = stage === 'installing' ? load.layoutProgress : null;
    const receivedBytes = load.bytes ?? 0;
    const totalBytes = load.prepared ? receivedBytes : load.totalBytes;
    const layoutProgress = layout
      ? (layout[1] === 0 ? 1 : Math.max(0, Math.min(1, layout[0] / layout[1]))) : null;
    const bytes = layout ? Math.floor(receivedBytes * layoutProgress) : receivedBytes;
    // Reading/receiving is only one pass. Parsing cannot report incremental
    // progress. Map measured layout progress onto the original source size;
    // generated indentation must not change the size displayed by the overlay.
    const progress = layout ? layoutProgress
      : ['reading', 'receiving'].includes(stage) && Number.isSafeInteger(totalBytes) && bytes < totalBytes
        ? bytes / totalBytes : null;
    this.onLoading({ name: load.name, stage, abortable,
      streaming: !!load.stream, bytes, totalBytes, progress });
  }

  abortOpen({ download = false } = {}) {
    if (!this.pendingLoad) return false;
    const load = this.pendingLoad;
    this.pendingLoad = null;
    load.resumeInstallation?.();
    clearTimeout(load.previewTimer);
    load.stream?.cancel({ download });
    load.worker.terminate();
    this.wasm.cancel_json5_file_preparation(load.id);
    load.acknowledge?.reject(new DOMException('Response loading was aborted', 'AbortError'));
    if (load.stream) this.wasm.cancel_streaming_document();
    this.onLoading(null);
    return true;
  }

  setParsing(parsing) {
    if (this.parsing === parsing) return;
    this.parsing = parsing;
    this.onParsing(parsing);
  }

  edit({ revision, edit }) {
    if (!this.active) return;
    this.revision = revision;
    this.setParsing(true);
    this.queue.push({ type: 'edit', revision, edit });
    clearTimeout(this.timer);
    this.timer = setTimeout(() => this.flush(), 20);
  }

  save(name, formatting, convert_to_json = false) {
    if (!this.active) {
      // Saving originates inside a WASM callback; return before calling it back.
      queueMicrotask(() => this.onSaveError('The document is not ready to save'));
      return;
    }
    // Save is a barrier in the same ordered stream as edits. Later keystrokes
    // cannot slip into the saved file while an earlier parse is finishing.
    this.queue.push({ type: 'save', name, formatting, convert_to_json });
    this.flush();
  }

  convertToJson() {
    if (this.conversionPending) return;
    this.conversionPending = true;
    if (!this.active || this.pendingLoad) {
      queueMicrotask(() => this.finishConversion(this.revision, '', 'The document is not ready to convert'));
      return;
    }
    try {
      this.queue.push({ type: 'convert' });
      this.flush();
    } catch (error) {
      this.busy = false;
      this.queue = this.queue.filter(operation => operation.type !== 'convert');
      queueMicrotask(() => this.finishConversion(this.sentRevision, '', String(error)));
    }
  }

  finishConversion(revision, content, error = '') {
    if (!this.conversionPending) return;
    this.conversionPending = false;
    this.onConverted(revision, content, error);
  }

  flush() {
    if (this.busy || !this.queue.length || !this.worker) return;
    clearTimeout(this.timer);
    this.busy = true;
    const first = this.queue.shift();
    if (first.type === 'save' || first.type === 'convert') {
      if (first.type === 'convert') this.setParsing(true);
      this.worker.postMessage({ ...first, id: this.active, revision: this.sentRevision });
      return;
    }
    const edits = [first.edit];
    let revision = first.revision;
    while (this.queue[0]?.type === 'edit') {
      const next = this.queue.shift();
      edits.push(next.edit);
      revision = next.revision;
    }
    this.worker.postMessage({ type: 'edit', id: this.active,
      fromRevision: this.sentRevision, revision, edits });
    this.sentRevision = revision;
  }

  completeOpen() {
    const load = this.pendingLoad;
    if (!load?.prepared) return;
    if (load.installing) {
      if (!this.busy && !this.queue.length) {
        const resume = load.resumeInstallation;
        load.resumeInstallation = null;
        resume?.();
      }
      return load.installation;
    }
    // Finish edits and save barriers on the previous document before retiring
    // its worker. Aborting while this finishes still preserves that document.
    if (this.busy || this.queue.length) {
      this.loading('waiting');
      this.flush();
      return;
    }
    load.installing = true;
    load.installation = this.installOpen(load).catch(error => {
      if (this.pendingLoad !== load) return;
      this.abortOpen();
      this.reportError(error);
    });
    return load.installation;
  }

  async installOpen(load) {
    const message = load.prepared;
    this.loading('installing');
    const start = performance.now();
    // Preparation only touches a detached layout. The current document and
    // undo history stay intact until the final synchronous commit.
    await this.yieldToBrowser();
    if (this.pendingLoad !== load) return;
    if (!this.wasm.prepare_json5_file(load.id, message.view)) {
      throw new Error('The editor could not prepare the document');
    }
    const updateProgress = () => {
      const measured = this.wasm.json5_file_preparation_progress(load.id);
      load.layoutProgress = measured.length === 2 ? measured : null;
      this.loading('installing');
    };
    updateProgress();
    let batches = 0;
    let wrapping = 0;
    const advance = () => {
      const batchStart = performance.now();
      const progress = this.wasm.advance_json5_file_preparation(load.id);
      wrapping += performance.now() - batchStart;
      batches++;
      if (progress < 0) throw new Error('Document preparation is unavailable');
      updateProgress();
      return progress;
    };
    while (this.pendingLoad === load) {
      // An earlier clipboard callback or save may finish while preparation is
      // yielding. Preserve the old document's ordered-operation barrier.
      if (this.busy || this.queue.length) {
        this.loading('waiting');
        await new Promise(resolve => {
          load.resumeInstallation = resolve;
          this.flush();
          if (!this.busy && !this.queue.length) {
            load.resumeInstallation = null;
            resolve();
          }
        });
        if (this.pendingLoad === load) this.loading('installing');
        continue;
      }
      if (advance() === 1) {
        // Give Abort a final turn, then confirm the font/size still match.
        await this.yieldToBrowser();
        if (this.pendingLoad !== load) return;
        if (!this.busy && !this.queue.length && advance() === 1) break;
      }
      await this.yieldToBrowser();
    }
    if (this.pendingLoad !== load) return;
    const commitStart = performance.now();
    this.loading('installing', false);
    if (!this.wasm.open_prepared_json5_file(message.name, message.view,
      message.highlights, message.folds, message.brackets, message.diagnostics)) {
      throw new Error('The editor could not install the prepared document');
    }
    console.info(`[JSON5 load] layout: ${wrapping.toFixed(1)} ms in ${batches} batches; ` +
      `commit: ${(performance.now() - commitStart).toFixed(1)} ms`);
    this.worker?.terminate();
    load.stream?.close?.();
    this.worker = load.worker;
    this.active = load.id;
    this.originalDocument = { name: message.name, file: message.original };
    this.pendingLoad = null;
    this.revision = this.parsedRevision = this.sentRevision = 0;
    clearTimeout(this.timer);
    this.setParsing(false);
    this.worker.postMessage({ type: 'commit', id: load.id });
    this.onLoading(null);
    if (load.stream) {
      console.info(`[JSON5 stream] ${load.bytes} bytes; receipt ${message.receivedElapsed.toFixed(1)} ms; ` +
        `preview appends ${load.previewElapsed.toFixed(1)} ms; analysis ${message.analysisElapsed.toFixed(1)} ms`);
      if (message.formattingElapsed !== undefined) {
        console.info(`[JSON5 stream] worker preview formatting: ${message.formattingElapsed.toFixed(1)} ms`);
      }
      console.info(`[JSON5 stream] total through editor installation: ${(performance.now() - load.started).toFixed(1)} ms`);
    }
    console.info(`[JSON5 load] worker: ${message.elapsed.toFixed(1)} ms; editor: ${(performance.now() - start).toFixed(1)} ms`);
  }

  receive(message, worker = this.pendingLoad?.id === message.id ? this.pendingLoad.worker : this.worker) {
    try {
      const fromLoad = this.pendingLoad && worker === this.pendingLoad.worker && message.id === this.pendingLoad.id;
      const fromActive = worker === this.worker && message.id === this.active;
      if (!fromLoad && !fromActive) return;
      if (fromLoad && message.type === 'stream-ready') {
        void this.pumpStream(this.pendingLoad);
        return;
      }
      if (fromLoad && message.type === 'stream-chunk-received') {
        this.pendingLoad.bytes = message.bytes;
        this.pendingLoad.acknowledge?.resolve(message);
        this.pendingLoad.acknowledge = null;
        this.loading('receiving');
        return;
      }
      if (fromLoad && message.type === 'stream-preview') {
        const load = this.pendingLoad;
        load.bytes = message.bytes;
        load.preview.push(message.preview);
        load.previewLength += message.preview.length;
        this.previewStream(load);
        worker.postMessage({ type: 'stream-preview-accepted', id: load.id });
        return;
      }
      if (message.type === 'error') {
        if (fromActive && message.operation === 'convert') {
          this.busy = false;
          this.finishConversion(this.sentRevision, '', message.message);
          this.setParsing(this.parsedRevision < this.revision);
          this.flush();
          this.completeOpen();
          return;
        }
        if (fromActive && message.operation === 'save') {
          this.busy = false;
          this.onSaveError(message.message);
          this.flush();
          this.completeOpen();
          return;
        }
        if (fromLoad) this.abortOpen();
        else {
          this.setParsing(false);
          this.finishConversion(this.sentRevision, '', message.message);
        }
        this.reportError(new Error(message.message));
        return;
      }
      if (message.type === 'progress' && fromLoad) {
        if (message.bytes !== undefined) this.pendingLoad.bytes = message.bytes;
        if (message.totalBytes !== undefined) this.pendingLoad.totalBytes = message.totalBytes;
        if (this.pendingLoad.stream?.takePort && message.stage === 'processing') {
          console.info(`[JSON5 stream] response complete: ${this.pendingLoad.bytes} bytes; starting final analysis`);
        }
        this.loading(message.stage);
        return;
      }
      if (message.type === 'opened' && fromLoad) {
        this.pendingLoad.prepared = message;
        return this.completeOpen();
      }
      if (!fromActive) return;
      if (message.type === 'converted') {
        this.finishConversion(message.revision, message.content);
        this.setParsing(this.parsedRevision < this.revision);
      }
      if (message.type === 'saved') {
        try {
          this.download(message.name, message.content);
          this.onSaved(message.name);
        } catch (error) { this.onSaveError(error.message ?? String(error)); }
      }
      if (message.type === 'syntax') {
        this.parsedRevision = message.revision;
        if (message.revision === this.revision) {
          this.wasm.apply_syntax_result(message.revision, message.highlights,
            message.folds, message.brackets, message.diagnostics);
        }
        this.setParsing(this.parsedRevision < this.revision);
      }
      this.busy = false;
      this.flush();
      this.completeOpen();
    } catch (error) {
      if (worker === this.pendingLoad?.worker) this.abortOpen();
      else if (worker === this.worker) this.finishConversion(this.sentRevision, '', String(error));
      this.reportError(error);
    }
  }
}
