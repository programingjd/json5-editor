// Browser-owned Find must be canceled during keydown, even when DOM focus is
// on the canvas or another control instead of GPUI's hidden keyboard receiver.
export function installSearchShortcut(openSearch) {
  const keydown = event => {
    if (!(event.ctrlKey || event.metaKey) || event.altKey || event.shiftKey
        || event.isComposing || event.key.toLowerCase() !== 'f') return;
    event.preventDefault();
    event.stopImmediatePropagation();
    if (!event.repeat) queueMicrotask(openSearch);
  };
  const requested = () => queueMicrotask(openSearch);
  window.addEventListener('keydown', keydown, { capture: true });
  window.addEventListener('gpui-json5-search', requested);
  return () => {
    window.removeEventListener('keydown', keydown, { capture: true });
    window.removeEventListener('gpui-json5-search', requested);
  };
}
