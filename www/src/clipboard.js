// Native clipboard access and the fallback both run after Rust event borrows
// unwind. Firefox can omit navigator.clipboard in an HTTP-embedded viewer.
export async function writeClipboard(text, clipboard) {
  await Promise.resolve();
  if (typeof clipboard?.writeText === 'function') {
    try {
      await clipboard.writeText(text);
      return;
    } catch (error) {
      if (!['NotAllowedError', 'SecurityError'].includes(error.name)) throw error;
    }
  }

  const previousFocus = document.activeElement;
  const selection = window.getSelection();
  const ranges = [];
  for (let index = 0; index < (selection?.rangeCount ?? 0); index++) {
    ranges.push(selection.getRangeAt(index).cloneRange());
  }
  const field = document.createElement('textarea');
  field.value = text;
  field.readOnly = true;
  field.tabIndex = -1;
  field.setAttribute('aria-label', 'Copy text');
  Object.assign(field.style, { position: 'fixed', left: '-10000px', top: '0', opacity: '0' });
  document.body.append(field);
  try {
    field.focus({ preventScroll: true });
    field.select();
    if (!document.execCommand('copy')) throw new Error('The browser refused to copy editor text');
  } finally {
    field.remove();
    if (selection) {
      selection.removeAllRanges();
      for (const range of ranges) selection.addRange(range);
    }
    if (previousFocus?.isConnected) previousFocus.focus({ preventScroll: true });
  }
}
