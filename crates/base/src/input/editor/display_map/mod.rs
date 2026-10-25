mod display_map;
mod inline_line;
pub(crate) use inline_line::{InlineFragment, InputLine};
mod fold_map;
mod folding;
mod text_wrapper;
mod wrap_map;

// Re-export public API
pub use self::display_map::{DisplayMap, WrappingIndent};
pub(crate) use self::text_wrapper::{LineLayout, RewrapJob};

// Re-export FoldRange and extract_fold_ranges
pub use folding::FoldRange;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BufferPoint {
    pub line: usize,
    pub col: usize,
}

impl BufferPoint {
    pub fn new(line: usize, col: usize) -> Self {
        Self { line, col }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct WrapPoint {
    pub row: usize,
    pub col: usize,
}

impl WrapPoint {
    pub(super) fn new(row: usize, col: usize) -> Self {
        Self { row, col }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DisplayPoint {
    pub row: usize,
    pub col: usize,
}

impl DisplayPoint {
    pub fn new(row: usize, col: usize) -> Self {
        Self { row, col }
    }
}
