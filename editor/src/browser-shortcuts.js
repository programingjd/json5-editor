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

export function preserveHistoryMouseButtons() {
  const leaveToBrowser = event => {
    if ((event.button === 3 || event.button === 4) && event.target instanceof HTMLCanvasElement) {
      // GPUI cancels pointer events on the canvas. Keep the browser's default
      // history action and leave these presses out of the editor's mouse state.
      event.stopPropagation();
    }
  };
  const options = { capture: true, passive: true };
  window.addEventListener('pointerdown', leaveToBrowser, options);
  window.addEventListener('pointerup', leaveToBrowser, options);
  return () => {
    window.removeEventListener('pointerdown', leaveToBrowser, options);
    window.removeEventListener('pointerup', leaveToBrowser, options);
  };
}

export function preserveNativeContextMenu(consumeCustomContextMenu) {
  let touchContextMenu = false;
  const onPointerDown = event => {
    touchContextMenu = event.target instanceof HTMLCanvasElement && event.pointerType === 'touch';
  };
  const onKeyDown = () => {
    touchContextMenu = false;
  };
  const onContextMenu = event => {
    if (!(event.target instanceof HTMLCanvasElement)) return;
    if (event.pointerType === 'touch'
        || (event.pointerType === undefined && event.button !== 2 && touchContextMenu)) {
      return;
    }
    if (event.button === 2 && consumeCustomContextMenu()) return;
    // GPUI cancels contextmenu on the canvas unconditionally. Stop the event
    // before it reaches the canvas listener so the browser's native menu opens.
    event.stopPropagation();
  };
  const capture = { capture: true };
  const passiveCapture = { capture: true, passive: true };
  window.addEventListener('pointerdown', onPointerDown, passiveCapture);
  window.addEventListener('keydown', onKeyDown, passiveCapture);
  window.addEventListener('contextmenu', onContextMenu, capture);
  return () => {
    window.removeEventListener('pointerdown', onPointerDown, passiveCapture);
    window.removeEventListener('keydown', onKeyDown, passiveCapture);
    window.removeEventListener('contextmenu', onContextMenu, capture);
  };
}
