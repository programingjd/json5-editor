function formatSize(bytes) {
  if (bytes < 1024) return `${bytes} B`;
  const units = ['KiB', 'MiB', 'GiB', 'TiB'];
  const unit = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)) - 1, units.length - 1);
  return `${(bytes / 1024 ** (unit + 1)).toFixed(1)} ${units[unit]}`;
}

export function createProcessingIndicators(abort) {
  const overlay = document.getElementById('document-loading');
  const title = document.getElementById('loading-title');
  const progress = document.getElementById('loading-progress');
  const size = document.getElementById('loading-size');
  const bar = document.getElementById('loading-progress-bar');
  const fill = bar.firstElementChild;
  const spinner = overlay.querySelector('.processing-spinner');
  const abortButton = document.getElementById('abort-loading');
  const actions = abortButton.closest('.loading-actions');
  const parsing = document.getElementById('parser-busy');
  let previousFocus = null;
  let parsingBusy = false;
  let layoutBusy = false;
  let conversionBusy = false;
  let loadingState = null;
  const updateParsing = () => {
    parsing.hidden = !parsingBusy || !overlay.hidden;
  };
  const canvasStates = new Map();
  const scrollKeys = new Set(['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight', 'PageUp', 'PageDown', 'Home', 'End', ' ']);
  const focusOverlay = () => overlay.focus({ preventScroll: true });
  const focusControl = () => (abortButton.disabled ? overlay : abortButton).focus({ preventScroll: true });

  abortButton.addEventListener('click', abort);
  window.addEventListener('keydown', (event) => {
    if (overlay.hidden) return;
    // Registered before GPUI starts: no key may reach the covered editor.
    event.stopImmediatePropagation();
    if (event.key === 'Escape') {
      event.preventDefault();
      if (!abortButton.disabled) abort();
    } else if (event.target === abortButton && (event.key === 'Enter' || event.key === ' ')) {
      event.preventDefault();
      if (!abortButton.disabled && !event.repeat) abortButton.click();
    } else if (event.key === 'Tab') {
      event.preventDefault();
      focusControl();
    } else if (scrollKeys.has(event.key) && !(event.key === ' ' && event.target === abortButton && !abortButton.disabled)) {
      // Let the loading page scroll natively, while the canvas stays inert.
      if (!overlay.contains(event.target)) event.preventDefault();
    }
  }, { capture: true });
  window.addEventListener('keyup', (event) => {
    if (!overlay.hidden) {
      event.stopImmediatePropagation();
      if (event.target === abortButton && (event.key === 'Enter' || event.key === ' ')) event.preventDefault();
    }
  }, { capture: true });
  window.addEventListener('focusin', (event) => {
    if (!overlay.hidden && !overlay.contains(event.target)) {
      event.stopImmediatePropagation();
      focusOverlay();
    }
  }, { capture: true });
  window.addEventListener('wheel', (event) => {
    if (overlay.hidden) return;
    if (!overlay.contains(event.target)) event.preventDefault();
    event.stopImmediatePropagation();
  }, { capture: true, passive: false });
  // Overlay controls must not also activate the canvas application's handlers.
  for (const name of ['keydown', 'keyup', 'pointerdown', 'pointerup', 'mousedown',
    'mouseup', 'click', 'dblclick', 'contextmenu']) {
    overlay.addEventListener(name, (event) => event.stopPropagation());
  }

  // Blocking operations share one surface, focus trap and canvas lock.
  // Loading takes priority; focus returns after every blocking operation finishes.
  const updateOverlay = () => {
    if (!loadingState && !layoutBusy && !conversionBusy) {
      const restoreFocus = overlay.contains(document.activeElement);
      overlay.hidden = true;
      for (const [canvas, inert] of canvasStates) canvas.inert = inert;
      canvasStates.clear();
      if (restoreFocus) {
        const target = previousFocus?.isConnected && previousFocus !== document.body
          ? previousFocus : document.querySelector('canvas');
        target?.focus({ preventScroll: true });
      }
      previousFocus = null;
      updateParsing();
      return;
    }
    overlay.dataset.mode = loadingState ? 'loading' : conversionBusy ? 'converting' : 'layout';
    progress.hidden = !loadingState;
    spinner.hidden = !!loadingState;
    actions.hidden = !loadingState;
    abortButton.disabled = !loadingState?.abortable;
    if (loadingState) {
      title.textContent = `Loading ${loadingState.name}`;
      const bytes = Math.max(0, loadingState.bytes ?? 0);
      const total = loadingState.totalBytes;
      const known = Number.isSafeInteger(total) && total >= bytes;
      size.textContent = known ? `${formatSize(bytes)} / ${formatSize(total)}` : formatSize(bytes);
      const measured = Number.isFinite(loadingState.progress);
      bar.classList.toggle('indeterminate', !measured);
      bar.setAttribute('aria-valuetext', size.textContent);
      if (measured) {
        const percent = Math.max(0, Math.min(100, loadingState.progress * 100));
        bar.setAttribute('aria-valuemax', '100');
        bar.setAttribute('aria-valuenow', String(percent));
        fill.style.width = `${percent}%`;
      } else {
        bar.removeAttribute('aria-valuemax');
        bar.removeAttribute('aria-valuenow');
        fill.style.removeProperty('width');
      }
    } else {
      title.textContent = conversionBusy ? 'Converting to JSON' : 'Updating layout';
    }
    if (overlay.hidden) {
      previousFocus = document.activeElement;
      overlay.hidden = false;
    }
    for (const canvas of document.querySelectorAll('canvas')) {
      if (!canvasStates.has(canvas)) canvasStates.set(canvas, canvas.inert);
      canvas.inert = true;
    }
    if (!overlay.contains(document.activeElement)
      || (abortButton.disabled && document.activeElement === abortButton)) focusOverlay();
    updateParsing();
  };

  return {
    loading(state) {
      loadingState = state;
      updateOverlay();
    },
    layout(busy) {
      layoutBusy = busy;
      updateOverlay();
    },
    converting(busy) {
      conversionBusy = busy;
      updateOverlay();
    },
    parsing(busy) {
      parsingBusy = busy;
      updateParsing();
    },
  };
}
