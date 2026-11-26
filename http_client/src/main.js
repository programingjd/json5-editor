import { loadFonts } from '../../editor/src/font-loader.js';
import { readColorScheme, observeColorScheme } from '../../editor/src/color-scheme.js';
import { preserveHistoryMouseButtons, preserveNativeContextMenu } from '../../editor/src/browser-shortcuts.js';
import { DocumentClient } from '../../editor/src/document-client.js';
import '../../editor/src/editor-shell.css';
import { createPreviewTransport, extractBinaryFile, prepareRequestBody } from './transport.js';
import { preview } from './preview.js';
import { createProxyTransport } from '../proxy/client.js';
import init, {
  advance_body_preparation, apply_syntax, begin_response, binary_drop_tab, binary_paste_tab,
  body_preparation_progress, cancel_body_preparation, consume_custom_context_menu, finish_archive,
  finish_request_error, install_body, install_non_json_response, prepare_body, run,
  selected_response_text, set_binary_drag_over, set_binary_file, set_color_scheme,
  set_request_loading,
} from './wasm/gpui_http_client_web.js';

function downloadName({ headers, url }) {
  const disposition = headers.find(([name]) => name.toLowerCase() === 'content-disposition')?.[1] ?? '';
  const encoded = /(?:^|;)\s*filename\*\s*=\s*UTF-8'[^']*'([^;]+)/i.exec(disposition)?.[1];
  const name = /(?:^|;)\s*filename\s*=\s*(?:"([^"]+)"|([^;]+))/i.exec(disposition);
  try { if (encoded) return decodeURIComponent(encoded.trim()); } catch { /* Try the plain name. */ }
  if (name) return (name[1] ?? name[2]).trim();
  const path = new URL(url).pathname.split('/').pop();
  try { return decodeURIComponent(path) || 'response.bin'; } catch { return path || 'response.bin'; }
}

function createTabDocuments(tab, transport) {
  let disposed = false;
  let responseDownload;
  const bodyEditor = (response, metadata) => ({
    prepare_json5_file: (id, view) => prepare_body(tab, response, id, view),
    advance_json5_file_preparation: id => advance_body_preparation(tab, response, id),
    json5_file_preparation_progress: id => body_preparation_progress(tab, response, id),
    cancel_json5_file_preparation: id => cancel_body_preparation(tab, response, id),
    open_prepared_json5_file: (_name, _view, highlights, folds, brackets, diagnostics) => {
      const installed = install_body(tab, response, metadata(), highlights, folds, brackets, diagnostics);
      if (installed && response) responseDownload = undefined;
      return installed;
    },
    apply_syntax_result: (revision, highlights, folds, brackets, diagnostics) =>
      apply_syntax(tab, revision, highlights, folds, brackets, diagnostics),
  });
  let pendingBody;
  const rejectRequestBody = error => {
    if (!pendingBody) {
      console.error('HTTP client request body:', error);
      return;
    }
    pendingBody.reject(error);
    pendingBody = undefined;
  };
  const requestDocuments = new DocumentClient(bodyEditor(false, () => ''), (_name, body) => {
    pendingBody?.resolve(body);
    pendingBody = undefined;
  }, rejectRequestBody, {
    onLoading: load => set_request_loading(tab, !!load),
    onSaveError: rejectRequestBody,
  });
  const loadRequest = body => {
    try { requestDocuments.openText('HTTP request', body); }
    catch (error) {
      requestDocuments.abortOpen();
      set_request_loading(tab, false);
      rejectRequestBody(error);
    }
  };
  const queueEdit = payload => requestDocuments.edit(JSON.parse(payload));
  let responseMetadata;
  const reportResponseError = error => {
    if (disposed) return;
    responseMetadata = undefined;
    finish_request_error(tab, error?.message ?? String(error), JSON.stringify(error?.timeline ?? []));
  };
  const responseDocuments = new DocumentClient(
    bodyEditor(true, () => responseMetadata), undefined, reportResponseError);
  const openResponse = ({ body, ...metadata }) => {
    if (disposed) return;
    const empty = metadata.bytes === 0;
    if (empty || metadata.format === 'binary' || metadata.format === 'text') {
      const download = !empty && metadata.format === 'binary' ? { body, name: downloadName(metadata) } : undefined;
      if (!install_non_json_response(tab, JSON.stringify(metadata), empty || download ? '' : body)) {
        throw new Error('Could not install the response');
      }
      responseDownload = download;
      return;
    }
    begin_response(tab);
    responseMetadata = JSON.stringify(metadata);
    try {
      responseDocuments.openText('HTTP response', body);
    } catch (error) {
      responseDocuments.abortOpen();
      throw error;
    }
  };
  const downloadResponse = () => {
    if (!responseDownload) return;
    const url = URL.createObjectURL(responseDownload.body);
    const link = document.createElement('a');
    link.href = url;
    link.download = responseDownload.name;
    link.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  };
  let binaryFile;
  const setBinaryFile = file => {
    binaryFile = file;
    set_binary_file(tab, file.name, file.size);
  };
  const clearBinaryFile = () => {
    binaryFile = undefined;
  };
  const send = (payload, text) => {
    // Return to GPUI after its click handler releases the view.
    void Promise.resolve().then(async () => {
      if (disposed) return;
      const request = JSON.parse(payload);
      const body = request.body_format === 'json' ? await new Promise((resolve, reject) => {
        pendingBody = { resolve, reject };
        requestDocuments.save('HTTP request', 'keep');
      }) : prepareRequestBody(request.body_format, text, binaryFile);
      if (disposed) return;
      return transport.send({ ...request, body });
    })
      .then(response => { if (response && !disposed) openResponse(response); })
      .catch(reportResponseError);
  };
  const archive = async (payload, text) => {
    const request = JSON.parse(payload);
    let body = text;
    if (request.body_format === 'json') {
      body = await new Promise((resolve, reject) => {
        pendingBody = { resolve, reject };
        requestDocuments.save('HTTP request', 'keep');
      });
    } else if (request.body_format === 'binary' && binaryFile) {
      body = {
        name: binaryFile.name,
        type: binaryFile.type,
        last_modified: binaryFile.lastModified,
        data: await new Promise((resolve, reject) => {
          const reader = new FileReader();
          reader.onload = () => resolve(reader.result);
          reader.onerror = () => reject(reader.error);
          reader.readAsDataURL(binaryFile);
        }),
      };
    }
    const key = 'json5-http-client.archived-requests';
    const archived = JSON.parse(localStorage.getItem(key) ?? '[]');
    if (!Array.isArray(archived)) throw new Error('Invalid archived requests storage');
    archived.push({ id: crypto.randomUUID(), archived_at: new Date().toISOString(), request: { ...request, body } });
    localStorage.setItem(key, JSON.stringify(archived));
  };
  const dispose = () => {
    disposed = true;
    pendingBody?.reject(new Error('Request tab closed'));
    pendingBody = undefined;
    for (const documents of [requestDocuments, responseDocuments]) {
      documents.abortOpen();
      documents.worker?.terminate();
    }
  };
  return { loadRequest, queueEdit, openResponse, reportResponseError, send, archive, setBinaryFile, clearBinaryFile, downloadResponse, dispose };
}

async function start(transport, initial) {
  const stopHistory = preserveHistoryMouseButtons();
  const [{ inter, mono, interItalic }] = await Promise.all([loadFonts(), init()]);
  const tabs = new Map();
  const closedTabs = new Set();
  const documentsFor = tab => {
    if (!tabs.has(tab)) tabs.set(tab, createTabDocuments(tab, transport));
    return tabs.get(tab);
  };
  const loadRequest = (tab, body) => queueMicrotask(() => {
    if (!closedTabs.has(tab)) documentsFor(tab).loadRequest(body);
  });
  const queueEdit = (tab, payload) => documentsFor(tab).queueEdit(payload);
  const send = (tab, payload, text) => documentsFor(tab).send(payload, text);
  const downloadResponse = tab => documentsFor(tab).downloadResponse();
  const closeTab = tab => {
    closedTabs.add(tab);
    queueMicrotask(() => {
      tabs.get(tab)?.dispose();
      tabs.delete(tab);
    });
  };
  const archiveTab = (tab, payload, text) => {
    void Promise.resolve().then(() => documentsFor(tab).archive(payload, text)).then(
      () => finish_archive(tab, ''),
      error => finish_archive(tab, error?.message ?? String(error)),
    );
  };
  const fileInput = document.createElement('input');
  fileInput.type = 'file';
  fileInput.hidden = true;
  document.body.append(fileInput);
  let fileTab;
  const pickBinary = tab => {
    fileTab = tab;
    fileInput.click();
  };
  const clearBinary = tab => {
    tabs.get(tab)?.clearBinaryFile();
  };
  fileInput.addEventListener('change', () => {
    const file = fileInput.files?.[0];
    fileInput.value = '';
    if (file && !closedTabs.has(fileTab)) documentsFor(fileTab).setBinaryFile(file);
  });
  const { response, ...initialRequest } = initial;
  const initialMetadata = response
    ? JSON.stringify({ ...initialRequest, response: { ...response, body: undefined } })
    : JSON.stringify(initialRequest);
  const ready = () => queueMicrotask(() => {
    const documents = documentsFor(0);
    documents.loadRequest(initial.request);
    if (response) {
      try { documents.openResponse(response); }
      catch (error) { documents.reportResponseError(error); }
    }
  });
  const requestDefaultsKey = 'json5-http-client.request-defaults';
  let requestDefaults = '';
  try {
    requestDefaults = localStorage.getItem(requestDefaultsKey) ?? '';
  } catch {
    // Storage can be unavailable. Start with the built-in defaults.
  }
  const persistRequestDefaults = value => localStorage.setItem(requestDefaultsKey, value);
  const stopContextMenu = preserveNativeContextMenu(() => consume_custom_context_menu());
  try {
    run(readColorScheme().dark, inter, mono, interItalic, send, queueEdit, loadRequest, pickBinary, clearBinary, downloadResponse, closeTab, archiveTab, ready,
      initialMetadata, requestDefaults, persistRequestDefaults);
  } catch (error) {
    stopContextMenu();
    throw error;
  }
  // Browser menu commands can send a copy event without a keydown.
  const copyResponseSelection = event => {
    if (!event.clipboardData) return;
    const text = selected_response_text();
    if (!text) return;
    event.clipboardData.setData('text/plain', text);
    event.preventDefault();
  };
  document.addEventListener('copy', copyResponseSelection);
  const hasBinaryDropPayload = dataTransfer => {
    const types = Array.from(dataTransfer?.types ?? []);
    return types.includes('Files') || types.includes('text/plain');
  };
  const binaryDropTarget = event => {
    const canvas = document.querySelector('canvas');
    if (!canvas) return -1;
    const rect = canvas.getBoundingClientRect();
    return binary_drop_tab(event.clientX - rect.left, event.clientY - rect.top);
  };
  let dragOverTab = -1;
  const setDragOverTab = tab => {
    if (tab < 0 && dragOverTab < 0) return;
    dragOverTab = tab;
    set_binary_drag_over(tab);
  };
  const onDragOver = event => {
    const tab = hasBinaryDropPayload(event.dataTransfer) ? binaryDropTarget(event) : -1;
    if (tab < 0) {
      setDragOverTab(-1);
      return;
    }
    event.preventDefault();
    event.dataTransfer.dropEffect = 'copy';
    setDragOverTab(tab);
  };
  const onDragLeave = event => {
    if (event.relatedTarget === null) setDragOverTab(-1);
  };
  let canvasHovered = false;
  const resetCanvasHover = () => {
    canvasHovered = false;
  };
  const onDragEnd = () => {
    resetCanvasHover();
    setDragOverTab(-1);
  };
  const onDrop = event => {
    resetCanvasHover();
    setDragOverTab(-1);
    const tab = binaryDropTarget(event);
    if (tab < 0 || closedTabs.has(tab)) return;
    const file = extractBinaryFile(event.dataTransfer, 'dropped');
    if (!file) return;
    event.preventDefault();
    documentsFor(tab).setBinaryFile(file);
  };
  const onPaste = event => {
    const tab = binary_paste_tab();
    if (tab < 0 || closedTabs.has(tab)) return;
    const file = extractBinaryFile(event.clipboardData, 'pasted');
    if (!file) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    documentsFor(tab).setBinaryFile(file);
  };
  const onPointerEnter = event => {
    if (event.target?.tagName === 'CANVAS') canvasHovered = true;
  };
  const onPointerLeave = event => {
    if (event.target?.tagName === 'CANVAS') canvasHovered = false;
  };
  const onPointerMove = event => {
    if (canvasHovered || event.pointerType === 'touch') return;
    const canvas = document.querySelector('canvas');
    if (!canvas || event.target !== canvas) return;
    const rect = canvas.getBoundingClientRect();
    if (event.clientX < rect.left || event.clientX > rect.right || event.clientY < rect.top || event.clientY > rect.bottom) return;
    canvas.dispatchEvent(new PointerEvent('pointerenter', {
      pointerId: event.pointerId,
      pointerType: event.pointerType,
      clientX: event.clientX,
      clientY: event.clientY,
    }));
  };
  window.addEventListener('blur', resetCanvasHover);
  window.addEventListener('pointerenter', onPointerEnter, { capture: true });
  window.addEventListener('pointerleave', onPointerLeave, { capture: true });
  window.addEventListener('pointermove', onPointerMove, { capture: true });
  window.addEventListener('dragover', onDragOver);
  window.addEventListener('dragleave', onDragLeave);
  window.addEventListener('dragend', onDragEnd);
  window.addEventListener('drop', onDrop);
  window.addEventListener('paste', onPaste, { capture: true });
  let dark = readColorScheme().dark;
  const stopTheme = observeColorScheme(state => {
    if (dark === state.dark) return;
    dark = state.dark;
    set_color_scheme(dark);
  });
  window.addEventListener('pagehide', event => {
    if (event.persisted) return;
    for (const documents of tabs.values()) documents.dispose();
    stopTheme();
    stopHistory();
    stopContextMenu();
    document.removeEventListener('copy', copyResponseSelection);
    window.removeEventListener('blur', resetCanvasHover);
    window.removeEventListener('pointerenter', onPointerEnter, { capture: true });
    window.removeEventListener('pointerleave', onPointerLeave, { capture: true });
    window.removeEventListener('pointermove', onPointerMove, { capture: true });
    window.removeEventListener('dragover', onDragOver);
    window.removeEventListener('dragleave', onDragLeave);
    window.removeEventListener('dragend', onDragEnd);
    window.removeEventListener('drop', onDrop);
    window.removeEventListener('paste', onPaste, { capture: true });
  });
}

const demo = new URLSearchParams(location.search).has('preview');
const initial = demo ? preview : { url: '', request: '{}', headers: '' };
const transport = demo ? createPreviewTransport(preview.response) : createProxyTransport();
void start(transport, initial).catch(error => {
  console.error('HTTP client:', error);
  const output = document.createElement('div');
  output.id = 'error';
  output.setAttribute('role', 'alert');
  output.textContent = error?.message ?? String(error);
  output.style.display = 'grid';
  document.body.append(output);
});
