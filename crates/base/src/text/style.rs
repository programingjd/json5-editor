use std::sync::Arc;

use gpui::{HighlightStyle, Hsla, Rems, StyleRefinement, rems};

use crate::ColorTokens;

#[derive(Clone)]
pub struct TextViewStyle {
    foreground: Hsla,
    muted_foreground: Hsla,
    link: Hsla,
    selection: Hsla,
    code_background: Hsla,
    border: Hsla,
    paragraph_gap: Rems,
    heading: Arc<dyn Fn(u8) -> StyleRefinement + Send + Sync + 'static>,
    code_block: StyleRefinement,
    table: StyleRefinement,
    table_head: StyleRefinement,
    table_cell: StyleRefinement,
    inline_code: HighlightStyle,

    table_background: Option<Hsla>,
    is_dark: bool,
}

impl PartialEq for TextViewStyle {
    fn eq(&self, other: &Self) -> bool {
        self.paragraph_gap == other.paragraph_gap
            && self.foreground == other.foreground
            && self.muted_foreground == other.muted_foreground
            && self.link == other.link
            && self.selection == other.selection
            && self.code_background == other.code_background
            && self.border == other.border
            && (1..=6).all(|level| (self.heading)(level) == (other.heading)(level))
            && self.code_block == other.code_block
            && self.table == other.table
            && self.table_head == other.table_head
            && self.table_cell == other.table_cell
            && self.inline_code == other.inline_code
            && self.table_background == other.table_background
            && self.is_dark == other.is_dark
    }
}

impl Default for TextViewStyle {
    fn default() -> Self {
        Self::from_colors(&ColorTokens::light(), false)
    }
}

impl TextViewStyle {
    pub fn from_theme(theme: &crate::Theme) -> Self {
        Self::from_colors(
            &theme.tokens.colors,
            theme.appearance == crate::ThemeAppearance::Dark,
        )
    }

    fn from_colors(colors: &ColorTokens, is_dark: bool) -> Self {
        Self {
            foreground: colors.foreground,
            muted_foreground: colors.muted_foreground,
            link: colors.primary,
            selection: colors.selection,
            code_background: colors.accent,
            border: colors.border,
            paragraph_gap: rems(0.75),
            heading: Arc::new(|_| StyleRefinement::default()),
            code_block: StyleRefinement::default(),
            table: StyleRefinement::default(),
            table_head: StyleRefinement::default(),
            table_cell: StyleRefinement::default(),
            inline_code: HighlightStyle {
                background_color: Some(colors.accent),
                ..Default::default()
            },
            table_background: None,
            is_dark,
        }
    }

    pub fn with_foreground(mut self, color: Hsla) -> Self {
        self.foreground = color;
        self
    }

    pub fn with_muted_foreground(mut self, color: Hsla) -> Self {
        self.muted_foreground = color;
        self
    }

    pub fn with_link(mut self, color: Hsla) -> Self {
        self.link = color;
        self
    }

    pub fn with_selection(mut self, color: Hsla) -> Self {
        self.selection = color;
        self
    }

    pub fn with_code_background(mut self, color: Hsla) -> Self {
        self.code_background = color;
        self
    }

    pub fn with_border(mut self, color: Hsla) -> Self {
        self.border = color;
        self
    }

    pub fn with_paragraph_gap(mut self, gap: Rems) -> Self {
        self.paragraph_gap = gap;
        self
    }

    pub fn with_heading<F>(mut self, heading: F) -> Self
    where
        F: Fn(u8) -> StyleRefinement + Send + Sync + 'static,
    {
        self.heading = Arc::new(heading);
        self
    }

    pub fn with_code_block(mut self, style: StyleRefinement) -> Self {
        self.code_block = style;
        self
    }

    pub fn with_inline_code(mut self, style: HighlightStyle) -> Self {
        self.inline_code = style;
        self
    }

    pub fn with_table(mut self, style: StyleRefinement) -> Self {
        self.table = style;
        self
    }

    pub fn with_table_head(mut self, style: StyleRefinement) -> Self {
        self.table_head = style;
        self
    }

    pub fn with_table_cell(mut self, style: StyleRefinement) -> Self {
        self.table_cell = style;
        self
    }

    pub fn with_dark(mut self, is_dark: bool) -> Self {
        self.is_dark = is_dark;
        self
    }

    pub fn foreground(&self) -> Hsla {
        self.foreground
    }

    pub fn muted_foreground(&self) -> Hsla {
        self.muted_foreground
    }

    pub fn link(&self) -> Hsla {
        self.link
    }

    pub fn selection(&self) -> Hsla {
        self.selection
    }

    pub fn code_background(&self) -> Hsla {
        self.code_background
    }

    pub fn border(&self) -> Hsla {
        self.border
    }

    pub fn paragraph_gap(&self) -> Rems {
        self.paragraph_gap
    }

    pub fn heading(&self, level: u8) -> StyleRefinement {
        (self.heading)(level)
    }

    pub fn code_block(&self) -> &StyleRefinement {
        &self.code_block
    }

    pub fn table(&self) -> &StyleRefinement {
        &self.table
    }

    pub fn table_head(&self) -> &StyleRefinement {
        &self.table_head
    }

    pub fn table_cell(&self) -> &StyleRefinement {
        &self.table_cell
    }

    pub fn inline_code(&self) -> HighlightStyle {
        self.inline_code
    }

    pub fn is_dark(&self) -> bool {
        self.is_dark
    }

    pub(crate) fn table_background(&self) -> Option<Hsla> {
        self.table_background
    }

    pub(crate) fn inline_code_highlight(&self) -> HighlightStyle {
        let mut style = self.inline_code;
        if style.background_color.is_none() {
            style.background_color = Some(self.code_background);
        }
        style
    }

    pub(crate) fn on_text_color(&self, color: Hsla) -> Self {
        let style = self.clone().with_foreground(color);
        if !self.is_inverted_by(color) {
            return style;
        }

        let code_background = color.opacity(0.12);
        let mut table_head = self.table_head.clone();
        table_head.background = Some(code_background.into());
        table_head.text.color = Some(color);
        let mut style = style
            .with_muted_foreground(color.opacity(0.7))
            .with_link(color)
            .with_selection(color.opacity(0.25))
            .with_code_background(code_background)
            .with_border(color.opacity(0.2))
            .with_inline_code(HighlightStyle {
                background_color: Some(code_background),
                ..self.inline_code
            })
            .with_table_head(table_head)
            .with_dark(!self.is_dark);
        style.table_background = Some(gpui::transparent_black());
        style
    }

    pub(crate) fn is_inverted_by(&self, color: Hsla) -> bool {
        const INVERTED_LIGHTNESS_GAP: f32 = 0.6;
        (oklab_lightness(color) - oklab_lightness(self.foreground)).abs() > INVERTED_LIGHTNESS_GAP
    }
}

fn oklab_lightness(color: Hsla) -> f32 {
    let rgb = color.to_rgb();
    let linear = |c: f32| {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let (r, g, b) = (linear(rgb.r), linear(rgb.g), linear(rgb.b));
    let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s
}
