mod language_name;
pub use gpui_base::input::{
    Diagnostic, DiagnosticEntry, DiagnosticRelatedInformation, DiagnosticSet, DiagnosticSeverity,
    DiagnosticSummary, DiagnosticTag, RelatedInformation,
};
pub(crate) use language_name::language_name;

// JSON5 parsing and incremental analysis live in the document worker.
pub(crate) fn input_highlighter_factory() -> gpui_base::input::InputHighlighterFactory {
    std::rc::Rc::new(|_| None)
}

// Theme and highlight data consumed by the worker-backed input presentation.
mod wasm_stub;
pub use wasm_stub::*;
