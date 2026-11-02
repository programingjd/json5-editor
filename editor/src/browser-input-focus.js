// GPUI paints its controls on a canvas, but receives keyboard and composition
// events through a hidden textarea. Firefox can move DOM focus back to the
// canvas after a pointer gesture, independently of GPUI's internal focus.
const documents = new WeakMap();

export function focusBrowserInput(element, options) {
  if (element.tagName === 'TEXTAREA') {
    const document = element.ownerDocument;
    let state = documents.get(document);
    if (!state) {
      state = { input: element };
      documents.set(document, state);
      document.addEventListener('focusin', ({ target }) => {
        if (target.tagName !== 'CANVAS') return;
        // Finish the native focus event and any Rust callback before focusing
        // again: synchronous activation can re-enter a borrowed GPUI window.
        queueMicrotask(() => {
          const input = state.input;
          if (document.activeElement !== target || !document.hasFocus()
              || !input.isConnected || target.closest('[inert]')
              || input.closest('[inert]')) return;
          input.focus({ preventScroll: true });
        });
      }, { capture: true });
    }
    state.input = element;
  }
  element.focus(options);
}
