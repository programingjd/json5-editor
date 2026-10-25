// Browser clipboard access may synchronously move focus. Let GPUI finish the
// current Rust event/RefCell borrow before invoking the native clipboard API.
export function browserClipboard() {
  return {
    name: 'browser-clipboard',
    enforce: 'pre',
    transform(code, id) {
      if (!id.replaceAll('\\', '/').endsWith('/wasm/gpui_json5_editor_web.js')) return;
      const write = 'const ret = arg0.writeText(getStringFromWasm0(arg1, arg2));';
      if (code.split(write).length !== 2) {
        throw new Error('The WASM clipboard binding changed; update browser-clipboard.js');
      }
      return {
        code: `import { writeClipboard } from '../clipboard.js';\n` + code.replace(write, `const text = getStringFromWasm0(arg1, arg2);
            const ret = writeClipboard(text, arg0).catch(error => {
                console.error('Unable to copy editor text:', error);
            });`),
        map: null,
      };
    },
  };
}
