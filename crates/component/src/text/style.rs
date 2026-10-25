use std::sync::Arc;

use gpui::{HighlightStyle, Pixels, Rems, StyleRefinement, px, rems};

use crate::highlighter::HighlightTheme;

#[derive(Clone)]
pub struct TextViewStyle {
    pub paragraph_gap: Rems,

    pub heading_base_font_size: Pixels,

    pub heading_font_size: Option<Arc<dyn Fn(u8, Pixels) -> Pixels + Send + Sync + 'static>>,

    pub highlight_theme: Arc<HighlightTheme>,

    pub code_block: StyleRefinement,

    pub table: StyleRefinement,

    pub table_head: StyleRefinement,

    pub table_cell: StyleRefinement,

    pub inline_code: HighlightStyle,

    pub is_dark: bool,
}

impl Default for TextViewStyle {
    fn default() -> Self {
        Self {
            paragraph_gap: rems(0.75),
            heading_base_font_size: px(14.),
            heading_font_size: None,
            highlight_theme: HighlightTheme::default_light().clone(),
            code_block: StyleRefinement::default(),
            table: StyleRefinement::default(),
            table_head: StyleRefinement::default(),
            table_cell: StyleRefinement::default(),
            inline_code: HighlightStyle::default(),
            is_dark: false,
        }
    }
}

impl PartialEq for TextViewStyle {
    fn eq(&self, other: &Self) -> bool {
        self.paragraph_gap == other.paragraph_gap
            && self.heading_base_font_size == other.heading_base_font_size
            && match (&self.heading_font_size, &other.heading_font_size) {
                (Some(left), Some(right)) => (1..=6).all(|level| {
                    left(level, self.heading_base_font_size)
                        == right(level, other.heading_base_font_size)
                }),
                (None, None) => true,
                _ => false,
            }
            && self.highlight_theme == other.highlight_theme
            && self.code_block == other.code_block
            && self.table == other.table
            && self.table_head == other.table_head
            && self.table_cell == other.table_cell
            && self.inline_code == other.inline_code
            && self.is_dark == other.is_dark
    }
}

impl TextViewStyle {
    pub fn paragraph_gap(mut self, gap: Rems) -> Self {
        self.paragraph_gap = gap;
        self
    }

    pub fn heading_font_size<F>(mut self, f: F) -> Self
    where
        F: Fn(u8, Pixels) -> Pixels + Send + Sync + 'static,
    {
        self.heading_font_size = Some(Arc::new(f));
        self
    }

    pub fn code_block(mut self, style: StyleRefinement) -> Self {
        self.code_block = style;
        self
    }

    pub fn inline_code(mut self, style: HighlightStyle) -> Self {
        self.inline_code = style;
        self
    }

    pub fn table(mut self, style: StyleRefinement) -> Self {
        self.table = style;
        self
    }

    pub fn table_head(mut self, style: StyleRefinement) -> Self {
        self.table_head = style;
        self
    }

    pub fn table_cell(mut self, style: StyleRefinement) -> Self {
        self.table_cell = style;
        self
    }
}
