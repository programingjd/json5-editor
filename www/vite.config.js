import { createEditorConfig } from '../editor/vite.config.mjs';

export default createEditorConfig(new URL('.', import.meta.url));
