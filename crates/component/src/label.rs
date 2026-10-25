use std::ops::Range;

use gpui::{
    App, HighlightStyle, IntoElement, ParentElement, RenderOnce, SharedString, StyleRefinement,
    Styled, StyledText, Window, div, prelude::FluentBuilder, rems,
};

use crate::{ActiveTheme, StyledExt};

const MASKED: &'static str = "•";

#[derive(Clone)]
pub enum HighlightsMatch {
    Prefix(SharedString),
    Full(SharedString),
}

impl HighlightsMatch {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Prefix(s) => s.as_str(),
            Self::Full(s) => s.as_str(),
        }
    }

    #[inline]
    pub fn is_prefix(&self) -> bool {
        matches!(self, Self::Prefix(_))
    }
}

impl From<&str> for HighlightsMatch {
    fn from(value: &str) -> Self {
        Self::Full(value.to_string().into())
    }
}

impl From<String> for HighlightsMatch {
    fn from(value: String) -> Self {
        Self::Full(value.into())
    }
}

impl From<SharedString> for HighlightsMatch {
    fn from(value: SharedString) -> Self {
        Self::Full(value)
    }
}

#[derive(IntoElement)]
pub struct Label {
    style: StyleRefinement,
    label: SharedString,
    secondary: Option<SharedString>,
    masked: bool,
    highlights_text: Option<HighlightsMatch>,
}

impl Label {
    pub fn new(label: impl Into<SharedString>) -> Self {
        let label: SharedString = label.into();
        Self {
            style: Default::default(),
            label,
            secondary: None,
            masked: false,
            highlights_text: None,
        }
    }

    pub fn secondary(mut self, secondary: impl Into<SharedString>) -> Self {
        self.secondary = Some(secondary.into());
        self
    }

    pub fn masked(mut self, masked: bool) -> Self {
        self.masked = masked;
        self
    }

    pub fn highlights(mut self, text: impl Into<HighlightsMatch>) -> Self {
        self.highlights_text = Some(text.into());
        self
    }

    fn full_text(&self) -> SharedString {
        match &self.secondary {
            Some(secondary) => format!("{} {}", self.label, secondary).into(),
            None => self.label.clone(),
        }
    }

    fn highlight_ranges(&self, total_length: usize) -> Vec<Range<usize>> {
        let mut ranges = Vec::new();
        let full_text = self.full_text();

        if self.secondary.is_some() {
            ranges.push(0..self.label.len());
            ranges.push(self.label.len()..total_length);
        }

        if let Some(matched) = &self.highlights_text {
            let matched_str = matched.as_str();
            if !matched_str.is_empty() {
                let search_lower = matched_str.to_lowercase();
                let full_text_lower = full_text.to_lowercase();

                if matched.is_prefix() {
                    // For prefix matching, only check if the text starts with the search term
                    if full_text_lower.starts_with(&search_lower) {
                        ranges.push(0..matched_str.len());
                    }
                } else {
                    // For full matching, find all occurrences
                    let mut search_start = 0;
                    while let Some(pos) = full_text_lower[search_start..].find(&search_lower) {
                        let match_start = search_start + pos;
                        let match_end = match_start + matched_str.len();

                        if match_end <= full_text.len() {
                            ranges.push(match_start..match_end);
                        }

                        search_start = match_start + 1;
                        while !full_text.is_char_boundary(search_start)
                            && search_start < full_text.len()
                        {
                            search_start += 1;
                        }

                        if search_start >= full_text.len() {
                            break;
                        }
                    }
                }
            }
        }

        ranges
    }

    fn measure_highlights(
        &self,
        length: usize,
        cx: &mut App,
    ) -> Option<Vec<(Range<usize>, HighlightStyle)>> {
        if self.masked {
            return None;
        }

        let ranges = self.highlight_ranges(length);
        if ranges.is_empty() {
            return None;
        }

        let mut highlights = Vec::new();
        let mut highlight_ranges_added = 0;

        if self.secondary.is_some() {
            highlights.push((ranges[0].clone(), HighlightStyle::default()));
            highlights.push((
                ranges[1].clone(),
                HighlightStyle {
                    color: Some(cx.theme().muted_foreground),
                    ..Default::default()
                },
            ));
            highlight_ranges_added = 2;
        }

        for range in ranges.iter().skip(highlight_ranges_added) {
            highlights.push((
                range.clone(),
                HighlightStyle {
                    color: Some(cx.theme().blue),
                    ..Default::default()
                },
            ));
        }

        Some(gpui::combine_highlights(vec![], highlights).collect())
    }
}

impl Styled for Label {
    fn style(&mut self) -> &mut gpui::StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Label {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let mut text = self.full_text();
        let chars_count = text.chars().count();

        if self.masked {
            text = SharedString::from(MASKED.repeat(chars_count))
        };

        let highlights = self.measure_highlights(text.len(), cx);

        div()
            .line_height(rems(1.25))
            .text_color(cx.theme().foreground)
            .refine_style(&self.style)
            .child(
                StyledText::new(&text).when_some(highlights, |this, hl| this.with_highlights(hl)),
            )
    }
}
