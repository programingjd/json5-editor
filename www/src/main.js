import { startEditor, reportEditorError } from '../../editor/src/editor.js';

void startEditor().catch(reportEditorError);
