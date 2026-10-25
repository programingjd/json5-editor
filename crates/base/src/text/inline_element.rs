use gpui::{AnyElement, IntoElement, Pixels, TextStyle};

pub struct InlineElement {
    pub(crate) element: AnyElement,
    pub(crate) baseline: Option<Pixels>,
}

impl InlineElement {
    pub fn new(element: impl IntoElement) -> Self {
        Self {
            element: element.into_any_element(),
            baseline: None,
        }
    }

    pub fn with_baseline(mut self, baseline: Pixels) -> Self {
        self.baseline = Some(baseline);
        self
    }
}

#[derive(Clone)]
pub struct InlineRenderContext {
    text_style: TextStyle,
    font_size: Pixels,
    line_height: Pixels,
    rem_size: Pixels,
}

impl InlineRenderContext {
    pub(crate) fn new(
        text_style: TextStyle,
        font_size: Pixels,
        line_height: Pixels,
        rem_size: Pixels,
    ) -> Self {
        Self {
            text_style,
            font_size,
            line_height,
            rem_size,
        }
    }

    pub fn text_style(&self) -> &TextStyle {
        &self.text_style
    }

    pub fn font_size(&self) -> Pixels {
        self.font_size
    }

    pub fn line_height(&self) -> Pixels {
        self.line_height
    }

    pub fn rem_size(&self) -> Pixels {
        self.rem_size
    }
}
