export function browserInputFocus() {
  return {
    name: 'browser-input-focus',
    enforce: 'pre',
    transform(code, id) {
      if (!id.replaceAll('\\', '/').endsWith('/wasm/gpui_json5_editor_web.js')) return;
      const focus = 'arg0.focus(arg1);';
      if (code.split(focus).length !== 2) {
        throw new Error('The WASM focus binding changed; update browser-input-focus.js');
      }
      return {
        code: `import { focusBrowserInput } from '../browser-input-focus.js';\n`
          + code.replace(focus, 'focusBrowserInput(arg0, arg1);'),
        map: null,
      };
    },
  };
}
