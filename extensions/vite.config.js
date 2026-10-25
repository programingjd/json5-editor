import { fileURLToPath } from 'node:url';
import { createEditorConfig } from '../editor/vite.config.mjs';

export default createEditorConfig(new URL('.', import.meta.url), {
  outDir: fileURLToPath(new URL('../target/extensions-web', import.meta.url)),
});
