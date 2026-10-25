const ATTRIBUTE = 'data-color-scheme-override';
const STORAGE_KEY = 'gpui-json5-editor.theme-override';
const isScheme = value => value === 'light' || value === 'dark';

function cssSchemes() {
  const style = getComputedStyle(document.documentElement);
  const scheme = style.getPropertyValue('--color-scheme').trim();
  const system = style.getPropertyValue('--system-color-scheme').trim();
  if (!isScheme(scheme) || !isScheme(system)) throw new Error('The editor color-scheme stylesheet is missing');
  return { scheme, system };
}

export function readColorScheme() {
  const { scheme, system } = cssSchemes();
  const override = document.documentElement.getAttribute(ATTRIBUTE);
  return { dark: scheme === 'dark', overridden: isScheme(override) && override !== system };
}

function setOverride(value, persist = true) {
  const root = document.documentElement;
  if (isScheme(value)) root.setAttribute(ATTRIBUTE, value);
  else root.removeAttribute(ATTRIBUTE);
  if (persist) {
    try {
      if (isScheme(value)) localStorage.setItem(STORAGE_KEY, value);
      else localStorage.removeItem(STORAGE_KEY);
    } catch { /* The HTML attribute retains this session's choice without storage. */ }
  }
}

export function toggleColorSchemeOverride() {
  const { scheme, system } = cssSchemes();
  const next = scheme === 'dark' ? 'light' : 'dark';
  setOverride(next === system ? '' : next);
}

export function observeColorScheme(onChange) {
  const root = document.documentElement;
  const signal = document.createElement('span');
  signal.id = 'color-scheme-observer';
  signal.setAttribute('aria-hidden', 'true');
  document.body.append(signal);
  let previous = '';
  const synchronize = () => {
    const override = root.getAttribute(ATTRIBUTE);
    // Returning to the system scheme clears both the attribute and saved choice.
    if (override !== null && (!isScheme(override) || override === cssSchemes().system)) setOverride('');
    const state = readColorScheme();
    const key = `${state.dark}:${state.overridden}`;
    if (key !== previous) {
      previous = key;
      onChange(state);
    }
  };
  const resize = new ResizeObserver(synchronize);
  resize.observe(signal);
  const attributes = new MutationObserver(synchronize);
  attributes.observe(root, { attributes: true, attributeFilter: [ATTRIBUTE] });
  const storage = event => {
    if (event.key === STORAGE_KEY || event.key === null) setOverride(event.newValue, false);
  };
  window.addEventListener('storage', storage);
  window.addEventListener('pageshow', synchronize);
  queueMicrotask(synchronize);
  return () => {
    resize.disconnect();
    attributes.disconnect();
    window.removeEventListener('storage', storage);
    window.removeEventListener('pageshow', synchronize);
    signal.remove();
  };
}
