#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FoldRange {
    pub start_line: usize,
    pub end_line: usize,

    pub start_column: Option<usize>,

    pub end_column: Option<usize>,
}

impl FoldRange {
    pub fn new(start_line: usize, end_line: usize) -> Self {
        assert!(
            start_line <= end_line,
            "fold start_line must be <= end_line"
        );
        Self {
            start_line,
            end_line,
            start_column: None,
            end_column: None,
        }
    }

    pub fn with_columns(
        start_line: usize,
        end_line: usize,
        start_column: usize,
        end_column: usize,
    ) -> Self {
        Self {
            start_line,
            end_line,
            start_column: Some(start_column),
            end_column: Some(end_column),
        }
    }
}
