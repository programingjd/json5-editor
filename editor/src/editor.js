import { DocumentClient } from './document-client.js';
import { createProcessingIndicators } from './processing-indicators.js';
import { loadFonts } from './font-loader.js';
import sbomUrl from '../../assets/sbom.json?url';
import './fonts.css';
import './processing-indicators.css';
import { mountEditorShell } from './editor-shell.js';
import './editor-shell.css';
import { readColorScheme, toggleColorSchemeOverride, observeColorScheme } from './color-scheme.js';

export function reportEditorError(error) {
  console.error('JSON5 editor:', error);
  const output = document.getElementById('error');
  output.textContent = `JSON5 editor\n${error?.stack ?? error}`;
  output.style.display = 'grid';
}

function download(name, content) {
  const url = URL.createObjectURL(new Blob([content], { type: 'application/json;charset=utf-8' }));
  const link = document.createElement('a');
  link.href = url;
  link.download = name;
  link.click();
  URL.revokeObjectURL(url);
}

// Both launchers pass their startup document explicitly; the editor owns the
// worker, GPUI lifecycle and file actions after startup.
export async function startEditor({ initialDocument = null } = {}) {
  mountEditorShell();
  const startup = performance.now();
  const fileInput = document.createElement('input');
  fileInput.type = 'file';
  fileInput.accept = '.json,.json5,application/json,text/json';
  fileInput.hidden = true;
  document.body.append(fileInput);

  const wasmReady = import('./wasm/gpui_json5_editor_web.js').then(async (wasm) => {
    await wasm.default();
    return wasm;
  });
  const [wasm, fonts, hostedDocument] = await Promise.all([
    wasmReady,
    loadFonts(),
    initialDocument,
  ]);
  const assetsReady = performance.now();
  let startupDocument = hostedDocument;
  const indicators = createProcessingIndicators(() => documents.abortOpen({ download: true }));
  const documents = new DocumentClient(wasm, download, reportEditorError, {
    onLoading: indicators.loading,
    onParsing: indicators.parsing,
    onSaved: (name) => wasm.finish_save(name, ''),
    onSaveError: (error) => wasm.finish_save('', String(error)),
    onConverted: (revision, content, error = '') => {
      try { wasm.finish_json_conversion(revision, content, error); }
      finally { indicators.converting(false); }
    },
  });

  window.addEventListener('gpui-json5-layout-busy', (event) => indicators.layout(event.detail));
  window.addEventListener('gpui-json5-filename', ({ detail: name }) => {
    document.title = `${name} — JSON5 Editor`;
  });
  window.addEventListener('gpui-json5-open', () => fileInput.click());
  window.addEventListener('gpui-json5-revert', () => documents.revert());
  window.addEventListener('gpui-json5-toggle-color-scheme', toggleColorSchemeOverride);
  fileInput.addEventListener('change', () => {
    const file = fileInput.files?.[0];
    fileInput.value = '';
    if (file) documents.openFile(file);
  });

  window.addEventListener('gpui-json5-open-text', ({ detail }) => {
    // Graphics initialization can finish after run() returns. Read CSS again
    // when the window is ready in case the preference changed during startup.
    const { dark, overridden } = readColorScheme();
    wasm.set_color_scheme(dark, overridden);
    // The host's response replaces the startup example, using the normal worker
    // loader and its abort/progress UI. Later Open actions keep their behavior.
    const file = startupDocument;
    startupDocument = null;
    if (file?.kind === 'stream') {
      console.info(`[JSON5 startup] assets/host: ${(assetsReady - startup).toFixed(1)} ms; ` +
        `GPUI window: ${(performance.now() - assetsReady).toFixed(1)} ms`);
    }
    if (file?.kind === 'stream') documents.openStream(file);
    else if (file) documents.openFile(file);
    else documents.openText(detail.name, detail.contents);
  });
  window.addEventListener('gpui-json5-edit', ({ detail }) => documents.edit(detail));
  window.addEventListener('gpui-json5-save', (event) => {
    documents.save(event.detail.name, event.detail.formatting, event.detail.convert_to_json);
  });
  window.addEventListener('gpui-json5-convert', () => {
    indicators.converting(true);
    documents.convertToJson();
  });

  window.addEventListener('dragover', (event) => {
    if (event.dataTransfer?.types.includes('Files')) event.preventDefault();
  });
  window.addEventListener('drop', (event) => {
    const file = event.dataTransfer?.files?.[0];
    if (!file) return;
    event.preventDefault();
    documents.openFile(file);
  });

  const { dark, overridden } = readColorScheme();
  await wasm.run(dark, overridden, fonts.inter, fonts.mono, fonts.interItalic,
    new URL(sbomUrl, window.location.href).href,
    new URL('./NOTICE', window.location.href).href);
  const stopObserving = observeColorScheme(({ dark, overridden }) => wasm.set_color_scheme(dark, overridden));
  window.addEventListener('pagehide', event => { if (!event.persisted) stopObserving(); });
}

