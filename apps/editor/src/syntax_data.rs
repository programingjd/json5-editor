use serde::{Deserialize, Serialize};

pub(crate) const SEMANTIC_NAMES: [&str; 11] = [
    "",
    "property",
    "string",
    "number",
    "boolean",
    "constant",
    "punctuation.bracket",
    "punctuation.delimiter",
    "comment",
    "string.escape",
    "punctuation",
];

#[derive(Serialize, Deserialize)]
pub(crate) struct DiagnosticData {
    pub start: usize,
    pub end: usize,
    pub message: String,
    pub code: String,
}
