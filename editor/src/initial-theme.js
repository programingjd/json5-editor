// Restore the explicit choice before first paint. CSS chooses the system default.
try {
  const value = localStorage.getItem('gpui-json5-editor.theme-override');
  if (value === 'light' || value === 'dark') {
    document.documentElement.setAttribute('data-color-scheme-override', value);
  }
} catch { /* CSS still supplies the system scheme when storage is unavailable. */ }
