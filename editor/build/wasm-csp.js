// wasm_thread uses js_sys::eval("self") to find its global scope. Resolve that
// fixed lookup statically; MV3 permits WASM but forbids JavaScript evaluation.
// Fail closed if the dependency ever requests a different expression.
export function wasmCsp() {
  return {
    name: 'wasm-bindgen-csp',
    enforce: 'pre',
    transform(code, id) {
      if (!id.replaceAll('\\', '/').endsWith('/wasm/gpui_json5_editor_web.js')) return;
      const lookup = 'eval(getStringFromWasm0(arg0, arg1))';
      const replacement = `(function(source) {
        if (source !== 'self') throw new Error('Unsupported dynamic WASM global lookup');
        return globalThis;
      })(getStringFromWasm0(arg0, arg1))`;
      const output = code.replaceAll(lookup, replacement);
      if (/\beval\s*\(/.test(output)) {
        throw new Error('The WASM binding has an unsupported JavaScript evaluator; update wasm-csp.js');
      }
      return { code: output, map: null };
    },
  };
}
