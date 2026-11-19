import { startEditor, reportEditorError } from './editor.js';

void startEditor().catch(reportEditorError);
