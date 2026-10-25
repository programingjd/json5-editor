import { startEditor, reportEditorError } from '../../editor/src/editor.js';
import { createInitialDocument } from './response-host.js';

void startEditor({ initialDocument: createInitialDocument() }).catch(reportEditorError);
