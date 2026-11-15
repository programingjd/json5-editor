import { createEditorConfig } from '../editor/vite.config.mjs';

export default async function httpClientConfig() {
  const config = await createEditorConfig(new URL('./', import.meta.url), {
    wasmModule: 'gpui_http_client_web',
  });
  config.plugins.push({
    name: 'http-client-title',
    transformIndexHtml: html => html.replace('<title>JSON5 Editor</title>', '<title>HTTP Client</title>'),
  });
  config.server.port = 3011;
  config.preview = { port: 3011 };
  return config;
}
