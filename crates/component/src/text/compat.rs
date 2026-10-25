use gpui::{
    AnyElement, App, Bounds, ClickEvent, Element, ElementId, Entity, GlobalElementId,
    HighlightStyle, InspectorElementId, IntoElement, LayoutId, Pixels, Refineable as _, RenderOnce,
    SharedString, StyleRefinement, Styled, Window,
};

use std::time::Duration;

use super::{
    MarkdownExtensions, MarkdownNode, MarkdownParseContext, MarkdownPlugin, SelectionFormat,
    TableData, TextViewMotion, TextViewState, TextViewStyle,
};
use gpui_base::{Easing, text::CodeBlock};

const STREAM_FADE: Duration = Duration::from_millis(280);
const STREAM_FADE_STAGGER: Duration = Duration::from_millis(10);

#[derive(Clone)]
pub struct TextView {
    id: ElementId,
    inner: gpui_base::TextView,
    text_style: Option<TextViewStyle>,
    motion: Option<TextViewMotion>,

    stream_fade: Option<bool>,
}

impl Styled for TextView {
    fn style(&mut self) -> &mut StyleRefinement {
        gpui::Styled::style(&mut self.inner)
    }
}

impl TextView {
    pub fn new(state: &Entity<TextViewState>) -> Self {
        Self {
            id: ElementId::Name(state.entity_id().to_string().into()),
            inner: gpui_base::TextView::new(state),
            text_style: None,
            motion: None,
            stream_fade: None,
        }
    }

    pub fn markdown(id: impl Into<ElementId>, text: impl Into<SharedString>) -> Self {
        let id = id.into();
        Self {
            id: id.clone(),
            inner: gpui_base::TextView::markdown(id, text),
            text_style: None,
            motion: None,
            stream_fade: None,
        }
    }

    pub fn html(id: impl Into<ElementId>, text: impl Into<SharedString>) -> Self {
        let id = id.into();
        Self {
            id: id.clone(),
            inner: gpui_base::TextView::html(id, text),
            text_style: None,
            motion: None,
            stream_fade: None,
        }
    }

    pub fn style(mut self, style: TextViewStyle) -> Self {
        self.text_style = Some(style);
        self
    }

    pub fn selectable(mut self, value: bool) -> Self {
        self.inner = self.inner.selectable(value);
        self
    }

    pub fn selection_format(mut self, value: SelectionFormat) -> Self {
        self.inner = self.inner.selection_format(value);
        self
    }

    pub fn scrollable(mut self, value: bool) -> Self {
        self.inner = self.inner.scrollable(value);
        self
    }

    pub fn stream_fade(mut self, value: bool) -> Self {
        self.stream_fade = Some(value);
        self
    }

    pub fn motion(mut self, motion: TextViewMotion) -> Self {
        self.motion = Some(motion);
        self
    }

    pub fn max_lines(mut self, value: usize) -> Self {
        self.inner = self.inner.max_lines(value);
        self
    }

    pub fn code_block_actions<F, E>(mut self, f: F) -> Self
    where
        F: Fn(&CodeBlock, &mut Window, &mut App) -> E + Send + Sync + 'static,
        E: IntoElement,
    {
        self.inner = self.inner.code_block_actions(f);
        self
    }

    pub fn table_actions<F, E>(mut self, f: F) -> Self
    where
        F: Fn(&TableData, &mut Window, &mut App) -> E + Send + Sync + 'static,
        E: IntoElement,
    {
        self.inner = self.inner.table_actions(f);
        self
    }

    pub fn on_link_click<F>(mut self, f: F) -> Self
    where
        F: Fn(&SharedString, &ClickEvent, &mut Window, &mut App) + Send + Sync + 'static,
    {
        self.inner = self.inner.on_link_click(f);
        self
    }

    pub fn on_reveal<F>(mut self, f: F) -> Self
    where
        F: Fn(Bounds<Pixels>, &mut Window, &mut App) + 'static,
    {
        self.inner = self.inner.on_reveal(f);
        self
    }

    pub fn markdown_extensions(mut self, value: MarkdownExtensions) -> Self {
        self.inner = self.inner.markdown_extensions(value);
        self
    }

    pub fn markdown_mdx(mut self) -> Self {
        self.inner = self.inner.markdown_mdx();
        self
    }

    pub fn markdown_block_parser<F>(mut self, parser: F) -> Self
    where
        F: for<'a> Fn(&markdown::mdast::Node, &MarkdownParseContext<'a>) -> Option<MarkdownNode>
            + Send
            + Sync
            + 'static,
    {
        self.inner = self.inner.markdown_block_parser(parser);
        self
    }

    pub fn markdown_block_renderer<F, E>(
        mut self,
        name: impl Into<SharedString>,
        renderer: F,
    ) -> Self
    where
        F: Fn(&MarkdownNode, &mut Window, &mut App) -> E + Send + Sync + 'static,
        E: IntoElement,
    {
        self.inner = self.inner.markdown_block_renderer(name, renderer);
        self
    }

    pub fn plugin<P>(self, plugin: P) -> Self
    where
        P: TextViewPlugin,
    {
        plugin.setup(self)
    }
}

impl IntoElement for TextView {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

pub struct TextViewLayoutState {
    element: AnyElement,
}

pub struct TextViewPrepaintState;

impl Element for TextView {
    type RequestLayoutState = TextViewLayoutState;
    type PrepaintState = TextViewPrepaintState;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut inner = self.inner.clone();
        if let Some(style) = self.text_style.clone() {
            // `request_layout` runs every frame, so this asks whether the
            // caller ever replaced the theme -- a pointer comparison against
            // the shared default -- rather than comparing two whole themes
            // field by field.
            #[cfg(feature = "tree-sitter")]
            if !std::sync::Arc::ptr_eq(
                &style.highlight_theme,
                &crate::highlighter::HighlightTheme::default_light(),
            ) {
                inner = inner.shared_code_block_highlighter(super::shared_code_block_highlighter(
                    &style.highlight_theme,
                ));
            }
            inner = inner.style(resolve_component_style(
                crate::ActiveTheme::theme(cx),
                style,
            ));
        }
        let motion = self.motion.clone().or_else(|| {
            self.stream_fade.map(|fade| {
                if !fade {
                    return TextViewMotion::default();
                }
                TextViewMotion::default()
                    .with_stream_fade(STREAM_FADE)
                    .with_stream_fade_stagger(STREAM_FADE_STAGGER)
                    .with_stream_fade_easing(Easing::EaseOut)
            })
        });
        if let Some(motion) = motion {
            inner = inner.motion(motion);
        }
        let mut element = inner.into_any_element();
        let layout_id = element.request_layout(window, cx);
        (layout_id, TextViewLayoutState { element })
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        element: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        element.element.prepaint(window, cx);
        TextViewPrepaintState
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        element: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        element.element.paint(window, cx);
    }
}

pub(super) fn resolve_component_style(
    theme: &crate::Theme,
    legacy: TextViewStyle,
) -> gpui_base::TextViewStyle {
    let themed = super::base_text_view_style(theme);

    let refined = |mut base: gpui::StyleRefinement, overlay: &StyleRefinement| {
        base.refine(overlay);
        base
    };
    let code_block = refined(themed.code_block().clone(), &legacy.code_block);
    let table = refined(themed.table().clone(), &legacy.table);
    let table_head = refined(themed.table_head().clone(), &legacy.table_head);
    let table_cell = refined(themed.table_cell().clone(), &legacy.table_cell);

    let mut inline_code = themed.inline_code();
    refine_highlight_style(&mut inline_code, legacy.inline_code);

    // `is_dark` only ever turns on: the component theme already answered the
    // question, and a legacy style left at its `false` default must not undo
    // a dark theme.
    let is_dark = themed.is_dark() || legacy.is_dark;

    let heading_base_font_size = legacy.heading_base_font_size;
    let heading_font_size = legacy.heading_font_size;
    // A style that never touched heading sizes keeps the Base default, which
    // scales headings with the body text; only an explicit base size or
    // resolver pins them to pixels.
    let pins_heading_size = heading_font_size.is_some()
        || heading_base_font_size != TextViewStyle::default().heading_base_font_size;
    let style = themed
        .with_paragraph_gap(legacy.paragraph_gap)
        .with_heading(move |level| {
            if !pins_heading_size {
                return StyleRefinement::default();
            }
            let default_size = match level {
                1 => gpui::rems(2.),
                2 => gpui::rems(1.5),
                3 => gpui::rems(1.25),
                4 => gpui::rems(1.125),
                _ => gpui::rems(1.),
            }
            .to_pixels(heading_base_font_size);
            let text_size = heading_font_size.as_ref().map_or(default_size, |resolve| {
                resolve(level, heading_base_font_size)
            });
            StyleRefinement::default().text_size(text_size)
        })
        .with_code_block(code_block)
        .with_table(table)
        .with_table_head(table_head)
        .with_table_cell(table_cell)
        .with_inline_code(inline_code)
        .with_dark(is_dark);
    style
}

fn refine_highlight_style(style: &mut HighlightStyle, refinement: HighlightStyle) {
    if refinement.color.is_some() {
        style.color = refinement.color;
    }
    if refinement.font_weight.is_some() {
        style.font_weight = refinement.font_weight;
    }
    if refinement.font_style.is_some() {
        style.font_style = refinement.font_style;
    }
    if refinement.background_color.is_some() {
        style.background_color = refinement.background_color;
    }
    if refinement.underline.is_some() {
        style.underline = refinement.underline;
    }
    if refinement.strikethrough.is_some() {
        style.strikethrough = refinement.strikethrough;
    }
    if refinement.fade_out.is_some() {
        style.fade_out = refinement.fade_out;
    }
}

pub trait TextViewPlugin {
    fn setup(self, text_view: TextView) -> TextView;
}
impl<P> TextViewPlugin for P
where
    P: MarkdownPlugin,
{
    fn setup(self, mut text_view: TextView) -> TextView {
        text_view.inner = text_view.inner.plugin(self);
        text_view
    }
}

#[derive(IntoElement, Clone)]
pub enum Text {
    String(SharedString),
    TextView(Box<TextView>),
}
impl From<SharedString> for Text {
    fn from(value: SharedString) -> Self {
        Self::String(value)
    }
}
impl From<String> for Text {
    fn from(value: String) -> Self {
        Self::String(value.into())
    }
}
impl From<&str> for Text {
    fn from(value: &str) -> Self {
        Self::String(value.to_string().into())
    }
}
impl From<TextView> for Text {
    fn from(value: TextView) -> Self {
        Self::TextView(Box::new(value))
    }
}
impl Text {
    pub fn style(self, style: TextViewStyle) -> Self {
        match self {
            Self::String(value) => Self::String(value),
            Self::TextView(view) => Self::TextView(Box::new(view.style(style))),
        }
    }
    pub(crate) fn get_text(&self, cx: &App) -> SharedString {
        match self {
            Self::String(value) => value.clone(),
            Self::TextView(view) => gpui_base::Text::from(view.inner.clone()).get_text(cx),
        }
    }
}
impl RenderOnce for Text {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        match self {
            Self::String(value) => value.into_any_element(),
            Self::TextView(view) => view.into_any_element(),
        }
    }
}

#[track_caller]
pub fn markdown(source: impl Into<SharedString>) -> TextView {
    TextView::markdown(
        ElementId::CodeLocation(*std::panic::Location::caller()),
        source,
    )
}

#[track_caller]
pub fn html(source: impl Into<SharedString>) -> TextView {
    TextView::html(
        ElementId::CodeLocation(*std::panic::Location::caller()),
        source,
    )
}
