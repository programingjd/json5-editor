export function open_file_picker() {
  window.dispatchEvent(new Event('gpui-json5-open'));
}

export function update_document_name(name) {
  queueMicrotask(() => window.dispatchEvent(
    new CustomEvent('gpui-json5-filename', { detail: name }),
  ));
}

export function is_mac_platform() {
  const platform = navigator.userAgentData?.platform || navigator.platform || navigator.userAgent || '';
  return /mac/i.test(platform);
}

export function update_overlay_colors(window_background, panel_background, heading, foreground, border, hover, active) {
  const style = document.documentElement.style;
  style.setProperty('--processing-window', window_background);
  style.setProperty('--processing-panel', panel_background);
  style.setProperty('--processing-heading', heading);
  for (const [name, value] of Object.entries({foreground, border, hover, active})) style.setProperty('--processing-' + name, value);
}

export function request_open_document(name, contents) {
  // Launch can run during run_embedded. Yield until its ApplicationHandle has
  // been stored before a worker can return an installation result to WASM.
  queueMicrotask(() => window.dispatchEvent(
    new CustomEvent('gpui-json5-open-text', { detail: { name, contents } }),
  ));
}

export function request_save_file(name, formatting, convert_to_json) {
  window.dispatchEvent(
    new CustomEvent('gpui-json5-save', { detail: { name, formatting, convert_to_json } }),
  );
}

const SAVE_OPTION_PREFIX = 'gpui-json5-editor.save.';

export function load_save_formatting() {
  try {
    const formatting = localStorage.getItem(SAVE_OPTION_PREFIX + 'formatting');
    if (['keep', 'minimize', 'indent'].includes(formatting)) return formatting;
    // Migrate the old checkbox preference, including its default of on.
    return load_save_option('remove-indentation') ? 'minimize' : 'keep';
  } catch {
    return 'minimize';
  }
}

export function store_save_formatting(formatting) {
  try {
    localStorage.setItem(SAVE_OPTION_PREFIX + 'formatting', formatting);
  } catch {
    // The overlay retains the choice for this session without local storage.
  }
}

export function request_convert_to_json() {
  // Return from the WASM action before moving DOM focus to the overlay.
  queueMicrotask(() => window.dispatchEvent(new Event('gpui-json5-convert')));
}

export function request_revert_document() {
  window.dispatchEvent(new Event('gpui-json5-revert'));
}

export function load_save_option(name) {
  try {
    return localStorage.getItem(SAVE_OPTION_PREFIX + name) !== 'false';
  } catch {
    return true;
  }
}

export function store_save_option(name, enabled) {
  try {
    localStorage.setItem(SAVE_OPTION_PREFIX + name, String(enabled));
  } catch {
    // The overlay still retains the choice for this session when storage is unavailable.
  }
}

let editorBounds = '';
export function update_editor_bounds(left, top, width, height) {
  const bounds = `${left},${top},${width},${height}`;
  if (bounds === editorBounds) return;
  editorBounds = bounds;
  const style = document.documentElement.style;
  for (const [key, value] of Object.entries({ left, top, width, height })) {
    style.setProperty(`--editor-${key}`, `${value}px`);
  }
}

export function queue_syntax_edit(revision, start, old_end, replacement,
  start_row, start_column, old_end_row, old_end_column, new_end_row, new_end_column) {
  window.dispatchEvent(new CustomEvent('gpui-json5-edit', { detail: {
    revision, edit: { start, old_end, replacement, start_row, start_column,
      old_end_row, old_end_column, new_end_row, new_end_column },
  } }));
}

export function request_toggle_color_scheme() {
  // Let the Rust click handler unwind before CSS changes are reported to GPUI.
  queueMicrotask(() => window.dispatchEvent(new Event('gpui-json5-toggle-color-scheme')));
}

export function set_layout_busy(busy) {
  window.dispatchEvent(new CustomEvent('gpui-json5-layout-busy', { detail: busy }));
}
