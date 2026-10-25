use std::fmt::{Debug, Display};

use gpui::ElementId;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IndexPath {
    pub section: usize,

    pub row: usize,

    pub column: usize,
}

impl From<IndexPath> for ElementId {
    fn from(path: IndexPath) -> Self {
        ElementId::Name(format!("index-path({},{},{})", path.section, path.row, path.column).into())
    }
}

impl Display for IndexPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "IndexPath(section: {}, row: {}, column: {})",
            self.section, self.row, self.column
        )
    }
}

impl IndexPath {
    pub fn new(row: usize) -> Self {
        IndexPath {
            section: 0,
            row,
            ..Default::default()
        }
    }

    pub fn section(mut self, section: usize) -> Self {
        self.section = section;
        self
    }

    pub fn row(mut self, row: usize) -> Self {
        self.row = row;
        self
    }

    pub fn column(mut self, column: usize) -> Self {
        self.column = column;
        self
    }

    pub fn eq_row(&self, index: IndexPath) -> bool {
        self.section == index.section && self.row == index.row
    }
}
