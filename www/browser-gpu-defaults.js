// The pinned GPUI renderer requests HighPerformance without an application
// setting. Apply the editor's browser policy at its generated WASM binding:
// leave GPU selection to the browser and preserve the other adapter options.
export function browserGpuDefaults() {
  return {
    name: 'browser-gpu-defaults',
    enforce: 'pre',
    transform(code, id) {
      if (!id.replaceAll('\\', '/').endsWith('/wasm/gpui_json5_editor_web.js')) return;
      const request = 'const ret = arg0.requestAdapter(arg1);';
      if (code.split(request).length !== 2) {
        throw new Error('The WASM adapter binding changed; update browser-gpu-defaults.js');
      }
      return {
        code: code.replace(request, `const options = { ...arg1 };
            delete options.powerPreference;
            const ret = arg0.requestAdapter(options);`),
        map: null,
      };
    },
  };
}
