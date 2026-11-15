import { loadFonts } from '../../editor/src/font-loader.js';
import { readColorScheme, observeColorScheme } from '../../editor/src/color-scheme.js';
import { preserveHistoryMouseButtons } from '../../editor/src/browser-shortcuts.js';
import { DocumentClient } from '../../editor/src/document-client.js';
import '../../editor/src/editor-shell.css';
import { createPreviewTransport } from './transport.js';
import { preview } from './preview.js';
import init, {
  advance_body_preparation, apply_syntax, begin_response, body_preparation_progress,
  cancel_body_preparation, finish_request_error, install_body, prepare_body,
  run, set_color_scheme, set_request_loading,
} from './wasm/gpui_http_client_web.js';

async function start(transport) {
  const stopHistory = preserveHistoryMouseButtons();
  const [{ inter, mono, interItalic }] = await Promise.all([loadFonts(), init()]);
  const bodyEditor = (response, metadata) => ({
    prepare_json5_file: (id, view) => prepare_body(response, id, view),
    advance_json5_file_preparation: id => advance_body_preparation(response, id),
    json5_file_preparation_progress: id => body_preparation_progress(response, id),
    cancel_json5_file_preparation: id => cancel_body_preparation(response, id),
    open_prepared_json5_file: (_name, _view, highlights, folds, brackets, diagnostics) =>
      install_body(response, metadata(), highlights, folds, brackets, diagnostics),
    apply_syntax_result: apply_syntax,
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
    onLoading: load => set_request_loading(!!load),
    onSaveError: rejectRequestBody,
  });
  const loadRequest = body => queueMicrotask(() => {
    try { requestDocuments.openText('HTTP request', body); }
    catch (error) {
      requestDocuments.abortOpen();
      set_request_loading(false);
      rejectRequestBody(error);
    }
  });
  const queueEdit = payload => requestDocuments.edit(JSON.parse(payload));
  let responseMetadata;
  const reportResponseError = error => {
    responseMetadata = undefined;
    finish_request_error(error?.message ?? String(error));
  };
  const responseDocuments = new DocumentClient(
    bodyEditor(true, () => responseMetadata), undefined, reportResponseError);
  const openResponse = ({ body, ...metadata }) => {
    begin_response();
    responseMetadata = JSON.stringify(metadata);
    try {
      responseDocuments.openText('HTTP response', body);
    } catch (error) {
      responseDocuments.abortOpen();
      throw error;
    }
  };
  const send = payload => {
    // Return to GPUI after its click handler releases the view.
    void Promise.resolve().then(() => new Promise((resolve, reject) => {
      pendingBody = { resolve, reject };
      requestDocuments.save('HTTP request', 'keep');
    }))
      .then(body => transport.send({ ...JSON.parse(payload), body }))
      .then(openResponse)
      .catch(reportResponseError);
  };
  const { body, ...initialMetadata } = preview.response;
  const ready = () => queueMicrotask(() => {
    loadRequest(preview.request);
    try { openResponse(preview.response); }
    catch (error) { reportResponseError(error); }
  });
  run(readColorScheme().dark, inter, mono, interItalic, send, queueEdit, loadRequest, ready,
    JSON.stringify({ ...preview, response: initialMetadata }));
  let dark = readColorScheme().dark;
  const stopTheme = observeColorScheme(state => {
    if (dark === state.dark) return;
    dark = state.dark;
    set_color_scheme(dark);
  });
  window.addEventListener('pagehide', event => {
    if (event.persisted) return;
    requestDocuments.abortOpen();
    requestDocuments.worker?.terminate();
    responseDocuments.abortOpen();
    responseDocuments.worker?.terminate();
    stopTheme();
    stopHistory();
  });
}

void start(createPreviewTransport(preview.response)).catch(error => {
  console.error('HTTP client:', error);
  const output = document.createElement('div');
  output.id = 'error';
  output.setAttribute('role', 'alert');
  output.textContent = error?.message ?? String(error);
  output.style.display = 'grid';
  document.body.append(output);
});
