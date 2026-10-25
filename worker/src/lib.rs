use serde::Deserialize;
use tree_sitter::{InputEdit, Point};
use wasm_bindgen::prelude::*;

#[path = "../../src/json5_format.rs"]
mod json5_format;
mod json_conversion;
mod parser;
mod stream_preview;
#[path = "../../src/syntax_data.rs"]
mod syntax_data;

#[derive(Deserialize)]
struct Edit {
    start: usize,
    old_end: usize,
    replacement: String,
    start_row: usize,
    start_column: usize,
    old_end_row: usize,
    old_end_column: usize,
    new_end_row: usize,
    new_end_column: usize,
}

/// Lives exclusively in the dedicated worker, with no GPUI/DOM dependencies.
#[wasm_bindgen]
pub struct DocumentProcessor {
    source: json5_format::SourceProjection,
    parser: parser::DocumentParser,
    incoming: String,
    preview: stream_preview::StreamPreview,
}

#[wasm_bindgen]
impl DocumentProcessor {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<DocumentProcessor, JsValue> {
        Ok(Self {
            source: json5_format::SourceProjection::new("", 2),
            parser: parser::DocumentParser::new().map_err(|error| JsValue::from_str(&error))?,
            incoming: String::new(),
            preview: stream_preview::StreamPreview::default(),
        })
    }

    pub fn open(&mut self, contents: &str) -> SyntaxResult {
        self.source = json5_format::SourceProjection::new(contents, 2);
        self.parser.reset();
        self.parser.analyze(self.source.view())
    }

    pub fn view(&self) -> String {
        self.source.view().to_owned()
    }

    /// Accumulate original UTF-8 once, without parsing an incomplete response.
    pub fn push_stream(&mut self, chunk: &str) -> String {
        self.incoming.push_str(chunk);
        self.preview.push(chunk)
    }

    pub fn stream_bytes(&self) -> usize {
        self.incoming.len()
    }

    pub fn finish_stream(&mut self) -> SyntaxResult {
        let source = std::mem::take(&mut self.incoming);
        self.open(&source)
    }

    pub fn apply_edits(&mut self, edits: &str) -> Result<SyntaxResult, JsValue> {
        let edits: Vec<Edit> =
            serde_json::from_str(edits).map_err(|error| JsValue::from_str(&error.to_string()))?;
        for edit in edits {
            let view = self.source.view();
            if edit.start > edit.old_end
                || edit.old_end > view.len()
                || !view.is_char_boundary(edit.start)
                || !view.is_char_boundary(edit.old_end)
            {
                return Err(JsValue::from_str(
                    "Document worker received an invalid edit range",
                ));
            }
            self.parser.edit(&InputEdit {
                start_byte: edit.start,
                old_end_byte: edit.old_end,
                new_end_byte: edit.start + edit.replacement.len(),
                start_position: Point::new(edit.start_row, edit.start_column),
                old_end_position: Point::new(edit.old_end_row, edit.old_end_column),
                new_end_position: Point::new(edit.new_end_row, edit.new_end_column),
            });
            self.source
                .apply_edit(edit.start..edit.old_end, &edit.replacement);
        }
        Ok(self.parser.analyze(self.source.view()))
    }

    pub fn save(&self, remove_indentation: bool) -> String {
        json5_format::format_for_save(self.source.source(), remove_indentation)
    }

    pub fn save_formatted(&self, formatting: &str) -> Result<String, JsValue> {
        match formatting {
            "keep" => Ok(self.save(false)),
            "minimize" => Ok(self.save(true)),
            "indent" => Ok(json5_format::auto_indent(self.source.source(), 2)),
            _ => Err(JsValue::from_str("Unknown save formatting option")),
        }
    }

    pub fn convert_to_json(&mut self, compact: bool) -> Result<String, JsValue> {
        self.parser
            .convert_to_json(self.source.source(), self.source.view(), compact)
            .map_err(|error| JsValue::from_str(&error))
    }
}

#[wasm_bindgen]
pub struct SyntaxResult {
    pub(crate) highlights: Vec<u32>,
    pub(crate) folds: Vec<u32>,
    pub(crate) brackets: Vec<u32>,
    pub(crate) diagnostics: String,
}

#[wasm_bindgen]
impl SyntaxResult {
    pub fn highlights(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.highlights)
    }
    pub fn folds(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.folds)
    }
    pub fn brackets(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.brackets)
    }
    pub fn diagnostics(&mut self) -> String {
        std::mem::take(&mut self.diagnostics)
    }
}
