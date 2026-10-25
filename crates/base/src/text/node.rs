use std::{
    borrow::Cow,
    collections::HashMap,
    ops::Range,
    sync::{Arc, Mutex, OnceLock, Weak},
};

use gpui::{
    AnyElement, App, Axis, Bounds, DefiniteLength, Div, Element, ElementId, FontStyle, FontWeight,
    GlobalElementId, HighlightStyle, Hsla, Image, ImageFormat, ImageSource, InspectorElementId,
    InteractiveElement as _, IntoElement, IsZero as _, LayoutId, ObjectFit, Overflow,
    ParentElement, Pixels, Point, Refineable as _, Rems, ScrollHandle, SharedString, SharedUri,
    StatefulInteractiveElement, StyleRefinement, Styled, StyledImage as _, TextStyleRefinement,
    WhiteSpace, Window, div, img, prelude::FluentBuilder as _, px, relative, rems,
};
use markdown::mdast;

use crate::{
    GlobalState, ScrollableMask, Scrollbar, StyledExt, h_flex,
    scrollable_mask::horizontal_scroll_area,
    text::{
        CodeBlockActionsFn, CodeBlockHighlighterFn, LinkClickHandlerFn, MarkdownExtensions,
        MarkdownNode, TableActionsFn,
        document::{NodeRenderOptions, PrevBlock, flow_position},
        inline::{
            Inline, InlineHighlight, InlineState, combine_highlights, fade_highlights,
            point_in_text_selection, text_runs, text_size_ranges,
        },
        inline_flow::{InlineFlow, InlineFlowItem, slice_ranges},
        range_highlight::{RangeHighlightFrame, RevealAt, RevealRequest},
        stream_fade::{StreamFadeFrame, TextLeafKey},
        text_view::handle_link_click,
    },
    theme::ActiveTheme as _,
};

use super::{
    SelectionFormat, TextViewStyle,
    utils::{data_url_image, list_item_prefix, ordered_list_ordinal},
};

const CHECK_SVG_LIGHT: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none"><path d="m3.25 8.25 3 3 6.5-7" stroke="white" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/></svg>"#;
const CHECK_SVG_DARK: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none"><path d="m3.25 8.25 3 3 6.5-7" stroke="black" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/></svg>"#;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum BlockNode {
    Root {
        children: Vec<BlockNode>,
        span: Option<Span>,
    },
    Paragraph(Paragraph),
    Heading {
        level: u8,
        children: Paragraph,
        span: Option<Span>,
    },
    Blockquote {
        children: Vec<BlockNode>,
        span: Option<Span>,
    },
    List {
        children: Vec<BlockNode>,
        ordered: bool,

        start: Option<u32>,
        span: Option<Span>,
    },
    ListItem {
        children: Vec<BlockNode>,
        spread: bool,

        checked: Option<bool>,
        span: Option<Span>,
    },
    CodeBlock(CodeBlock),

    Custom {
        node: MarkdownNode,
        state: BlockState,
    },
    Table(Table),
    Break {
        html: bool,
        span: Option<Span>,
    },
    HorizontalRule {
        span: Option<Span>,
    },

    Definition {
        identifier: SharedString,
        url: SharedString,
        title: Option<SharedString>,
        span: Option<Span>,
    },
    Unknown,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct BlockState {
    // None is unobserved, allowing virtualized copy to include enclosed blocks.
    selection: Arc<Mutex<Option<BlockSelection>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockSelection {
    Empty,
    Full,
}

impl BlockState {
    fn selection(&self) -> Option<BlockSelection> {
        self.selection.lock().ok().and_then(|selection| *selection)
    }

    fn set_selection(&self, value: Option<BlockSelection>) {
        if let Ok(mut selection) = self.selection.lock() {
            *selection = value;
        }
    }

    fn is_selected(&self) -> bool {
        self.selection() == Some(BlockSelection::Full)
    }

    fn has_selection_observation(&self) -> bool {
        self.selection().is_some()
    }

    fn clear_selection(&self) {
        self.set_selection(None);
    }
}

impl PartialEq for BlockState {
    fn eq(&self, _: &Self) -> bool {
        // Runtime selection does not change parsed document identity.
        true
    }
}

struct CustomBlockElement {
    content: AnyElement,
    state: BlockState,
}

impl IntoElement for CustomBlockElement {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for CustomBlockElement {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
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
    ) -> (LayoutId, ()) {
        (self.content.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.content.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let view = GlobalState::global(cx).text_view_state();
        let selection = view.as_ref().and_then(|view| {
            let state = view.read(cx);
            if !state.is_selectable() {
                return None;
            }
            if state.is_all_selected() {
                return Some(BlockSelection::Full);
            }
            if state.preserve_inline_selection {
                return self.state.selection();
            }
            let (start, end) = state.selection_points(cx)?;
            // Selection uses the full content bounds, not the visible clip or
            // the paragraph gap outside this element.
            if bounds.size.width <= Pixels::ZERO
                || bounds.size.height <= Pixels::ZERO
                || bounds.bottom() <= start.y.min(end.y)
                || bounds.top() > start.y.max(end.y)
            {
                return None;
            }
            Some(if custom_block_is_selected(bounds, start, end) {
                BlockSelection::Full
            } else {
                BlockSelection::Empty
            })
        });
        self.state.set_selection(selection);
        if let Some(view) = &view {
            let visible = bounds.intersect(&window.content_mask().bounds);
            if view.read(cx).is_selectable()
                && visible.size.width > Pixels::ZERO
                && visible.size.height > Pixels::ZERO
            {
                view.read(cx)
                    .selection_adapter
                    .register_inline(vec![visible]);
            }
        }
        self.content.paint(window, cx);
        if selection == Some(BlockSelection::Full) {
            let color = view.as_ref().unwrap().read(cx).text_view_style.selection();
            window.paint_quad(gpui::fill(bounds, color));
        }
    }
}

fn custom_block_is_selected(
    bounds: Bounds<Pixels>,
    start: Point<Pixels>,
    end: Point<Pixels>,
) -> bool {
    let (start_inside, end_inside) = (bounds.contains(&start), bounds.contains(&end));
    start != end
        && !(start_inside && end_inside)
        && (start_inside
            || end_inside
            || point_in_text_selection(
                bounds.origin,
                bounds.size.width,
                start,
                end,
                bounds.size.height,
            ))
}

#[derive(Clone, Copy)]
enum BlockTextKind {
    All,
    Selected,

    SelectedSource,
}

impl BlockNode {
    pub(super) fn is_list_item(&self) -> bool {
        matches!(self, Self::ListItem { .. })
    }

    pub(super) fn compact(self) -> BlockNode {
        match self {
            Self::Root { mut children, .. } if children.len() == 1 => children.remove(0).compact(),
            _ => self,
        }
    }

    pub(crate) fn span(&self) -> Option<Span> {
        match self {
            BlockNode::Root { span, .. } => *span,
            BlockNode::Paragraph(paragraph) => paragraph.span,
            BlockNode::Heading { span, .. } => *span,
            BlockNode::Blockquote { span, .. } => *span,
            BlockNode::List { span, .. } => *span,
            BlockNode::ListItem { span, .. } => *span,
            BlockNode::CodeBlock(code_block) => code_block.span,
            BlockNode::Custom { node, .. } => node.span,
            BlockNode::Table(table) => table.span,
            BlockNode::Break { span, .. } => *span,
            BlockNode::HorizontalRule { span, .. } => *span,
            BlockNode::Definition { span, .. } => *span,
            BlockNode::Unknown { .. } => None,
        }
    }

    pub(super) fn text(&self) -> String {
        self.text_by_kind(BlockTextKind::All)
    }

    pub(super) fn selected_text(&self, format: SelectionFormat) -> String {
        self.text_by_kind(match format {
            SelectionFormat::Plain => BlockTextKind::Selected,
            SelectionFormat::Source => BlockTextKind::SelectedSource,
        })
    }

    pub(super) fn selected_source_range(&self) -> SourceRangeSelection {
        let mut selected = SourceRangeSelection::Unselected;
        match self {
            BlockNode::Root { children, .. }
            | BlockNode::Blockquote { children, .. }
            | BlockNode::List { children, .. }
            | BlockNode::ListItem { children, .. } => {
                for child in children {
                    selected.merge(child.selected_source_range());
                }
            }
            BlockNode::Paragraph(paragraph) => selected = paragraph.selected_source_range(),
            BlockNode::Heading { children, .. } => selected = children.selected_source_range(),
            BlockNode::Table(table) => {
                for row in &table.children {
                    for cell in &row.children {
                        selected.merge(cell.children.selected_source_range());
                    }
                }
            }
            BlockNode::CodeBlock(code_block) => selected = code_block.selected_source_range(),
            BlockNode::Custom { node, state } => {
                if state.is_selected() {
                    selected = node
                        .source_range()
                        .map_or(SourceRangeSelection::Unmapped, SourceRangeSelection::Mapped);
                }
            }
            BlockNode::Definition { .. }
            | BlockNode::Break { .. }
            | BlockNode::HorizontalRule { .. }
            | BlockNode::Unknown => {}
        }
        selected
    }

    fn text_by_kind(&self, kind: BlockTextKind) -> String {
        let mut text = String::new();
        match self {
            BlockNode::Root { children, .. } => {
                let block_text = Self::children_text(children, kind);
                if !block_text.is_empty() {
                    text.push_str(&block_text);
                    text.push('\n');
                }
            }
            BlockNode::Paragraph(paragraph) => {
                let block_text = match kind {
                    BlockTextKind::All => paragraph.text(),
                    BlockTextKind::Selected => paragraph.selected_text(),
                    BlockTextKind::SelectedSource => paragraph.selected_source(),
                };
                if !block_text.is_empty() {
                    text.push_str(&block_text);
                    text.push('\n');
                }
            }
            BlockNode::Heading {
                level, children, ..
            } => {
                let block_text = match kind {
                    BlockTextKind::All => children.text(),
                    BlockTextKind::Selected => children.selected_text(),
                    BlockTextKind::SelectedSource => children.selected_source(),
                };
                if !block_text.is_empty() {
                    // In source mode, prefix the heading marker so a selected
                    // heading round-trips as Markdown (e.g. `## Title`).
                    if matches!(kind, BlockTextKind::SelectedSource) {
                        text.push_str(&"#".repeat(*level as usize));
                        text.push(' ');
                    }
                    text.push_str(&block_text);
                    text.push('\n');
                }
            }
            BlockNode::List {
                children,
                ordered,
                start,
                ..
            } => {
                if matches!(kind, BlockTextKind::SelectedSource) {
                    // Reconstruct the list source, indenting nested lists and
                    // restoring list markers and task-list checkboxes.
                    text.push_str(&list_selected_source(children, *ordered, *start, ""));
                } else {
                    text.push_str(&Self::children_text(children, kind));
                }
            }
            BlockNode::ListItem { children, .. } => {
                text.push_str(&Self::children_text(children, kind));
            }
            BlockNode::Blockquote { children, .. } => {
                let block_text = Self::children_text(children, kind);

                if !block_text.is_empty() {
                    if matches!(kind, BlockTextKind::SelectedSource) {
                        // Prefix every line with `> ` so a selected blockquote
                        // round-trips as Markdown.
                        let quoted = block_text
                            .trim_end_matches('\n')
                            .lines()
                            .map(|line| {
                                if line.is_empty() {
                                    ">".to_string()
                                } else {
                                    format!("> {}", line)
                                }
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        text.push_str(&quoted);
                    } else {
                        text.push_str(&block_text);
                    }
                    text.push('\n');
                }
            }
            BlockNode::Table(table) => {
                if matches!(kind, BlockTextKind::SelectedSource) {
                    let block_text = table_selected_source(table);
                    if !block_text.is_empty() {
                        text.push_str(&block_text);
                        text.push('\n');
                    }
                } else {
                    let mut block_text = String::new();
                    for row in table.children.iter() {
                        let mut row_texts = vec![];
                        for cell in row.children.iter() {
                            row_texts.push(match kind {
                                BlockTextKind::All => cell.children.text(),
                                // Source is handled above; only Selected reaches here.
                                _ => cell.children.selected_text(),
                            });
                        }
                        if !row_texts.is_empty() {
                            block_text.push_str(&row_texts.join(" "));
                            block_text.push('\n');
                        }
                    }

                    if !block_text.is_empty() {
                        text.push_str(&block_text);
                        text.push('\n');
                    }
                }
            }
            BlockNode::CodeBlock(code_block) => {
                let block_text = match kind {
                    BlockTextKind::All => code_block.text(),
                    BlockTextKind::Selected => code_block.selected_text(),
                    BlockTextKind::SelectedSource => code_block.selected_source(),
                };
                if !block_text.is_empty() {
                    text.push_str(&block_text);
                    text.push('\n');
                }
            }
            BlockNode::Custom { node, state } => {
                if matches!(kind, BlockTextKind::All) || state.is_selected() {
                    let content = match kind {
                        BlockTextKind::SelectedSource => Cow::Owned(node.to_markdown()),
                        _ => Cow::Borrowed(node.as_text()),
                    };
                    if !content.is_empty() {
                        text.push_str(&content);
                        text.push('\n');
                    }
                }
            }
            BlockNode::Definition { .. }
            | BlockNode::Break { .. }
            | BlockNode::HorizontalRule { .. }
            | BlockNode::Unknown { .. } => {}
        }

        text
    }

    fn children_text(children: &[BlockNode], kind: BlockTextKind) -> String {
        let mut text = String::new();
        for child in children.iter() {
            text.push_str(&child.text_by_kind(kind));
        }

        text
    }

    pub(super) fn has_selection(&self) -> bool {
        match self {
            BlockNode::Root { children, .. }
            | BlockNode::Blockquote { children, .. }
            | BlockNode::List { children, .. }
            | BlockNode::ListItem { children, .. } => {
                children.iter().any(|child| child.has_selection())
            }
            BlockNode::Paragraph(paragraph) => paragraph.has_selection(),
            BlockNode::Heading { children, .. } => children.has_selection(),
            BlockNode::Table(table) => table.children.iter().any(|row| {
                row.children
                    .iter()
                    .any(|cell| cell.children.has_selection())
            }),
            BlockNode::CodeBlock(code_block) => code_block.has_selection(),
            BlockNode::Custom { state, .. } => {
                // An observed empty endpoint must suppress virtualized copy fallback.
                state.has_selection_observation()
            }
            BlockNode::Definition { .. }
            | BlockNode::Break { .. }
            | BlockNode::HorizontalRule { .. }
            | BlockNode::Unknown { .. } => false,
        }
    }

    pub(super) fn clear_selection(&self) {
        match self {
            BlockNode::Root { children, .. }
            | BlockNode::Blockquote { children, .. }
            | BlockNode::List { children, .. }
            | BlockNode::ListItem { children, .. } => {
                for child in children.iter() {
                    child.clear_selection();
                }
            }
            BlockNode::Paragraph(paragraph) => paragraph.clear_selection(),
            BlockNode::Heading { children, .. } => children.clear_selection(),
            BlockNode::Table(table) => {
                for row in table.children.iter() {
                    for cell in row.children.iter() {
                        cell.children.clear_selection();
                    }
                }
            }
            BlockNode::CodeBlock(code_block) => code_block.clear_selection(),
            BlockNode::Custom { state, .. } => {
                state.clear_selection();
            }
            BlockNode::Definition { .. }
            | BlockNode::Break { .. }
            | BlockNode::HorizontalRule { .. }
            | BlockNode::Unknown { .. } => {}
        }
    }
}

#[allow(unused)]
#[derive(Debug, Default, Clone, PartialEq)]
pub struct LinkMark {
    pub url: SharedString,

    pub identifier: Option<SharedString>,
    pub title: Option<SharedString>,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct TextMark {
    pub bold: bool,
    pub italic: bool,
    pub strikethrough: bool,
    pub underline: bool,
    pub code: bool,

    pub highlight: Option<Hsla>,
    pub link: Option<LinkMark>,
}

impl TextMark {
    pub fn bold(mut self) -> Self {
        self.bold = true;
        self
    }

    pub fn italic(mut self) -> Self {
        self.italic = true;
        self
    }

    pub fn strikethrough(mut self) -> Self {
        self.strikethrough = true;
        self
    }

    pub fn underline(mut self) -> Self {
        self.underline = true;
        self
    }

    pub fn code(mut self) -> Self {
        self.code = true;
        self
    }

    pub fn highlight(mut self, color: Hsla) -> Self {
        self.highlight = Some(color);
        self
    }

    pub fn link(mut self, link: impl Into<LinkMark>) -> Self {
        self.link = Some(link.into());
        self
    }

    pub fn merge(&mut self, other: TextMark) {
        self.bold |= other.bold;
        self.italic |= other.italic;
        self.strikethrough |= other.strikethrough;
        self.underline |= other.underline;
        self.code |= other.code;
        if other.highlight.is_some() {
            self.highlight = other.highlight;
        }
        if let Some(link) = other.link {
            self.link = Some(link);
        }
    }
}

#[derive(Debug, Default, Copy, Clone, PartialEq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

#[allow(unused)]
#[derive(Default, Clone)]
pub struct ImageNode {
    pub url: SharedUri,
    pub link: Option<LinkMark>,
    pub title: Option<SharedString>,
    pub alt: Option<SharedString>,
    pub width: Option<DefiniteLength>,
    pub height: Option<DefiniteLength>,
    pub(crate) span: Option<Span>,

    pub(super) embedded: OnceLock<Option<Arc<Image>>>,
}

impl ImageNode {
    pub fn title(&self) -> String {
        self.title
            .clone()
            .unwrap_or_else(|| self.alt.clone().unwrap_or_default())
            .to_string()
    }

    pub(super) fn source(&self) -> ImageSource {
        match self.embedded.get_or_init(|| data_url_image(&self.url)) {
            Some(image) => ImageSource::Image(image.clone()),
            None => self.url.clone().into(),
        }
    }
}

impl std::fmt::Debug for ImageNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageNode")
            .field("url", &self.url)
            .field("link", &self.link)
            .field("title", &self.title)
            .field("alt", &self.alt)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("span", &self.span)
            .finish()
    }
}

impl PartialEq for ImageNode {
    fn eq(&self, other: &Self) -> bool {
        self.url == other.url
            && self.link == other.link
            && self.title == other.title
            && self.alt == other.alt
            && self.width == other.width
            && self.height == other.height
            && self.span == other.span
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SourceSegment {
    pub(crate) rendered: Range<usize>,
    pub(crate) source: Range<usize>,

    pub(crate) linear: bool,
}

pub(crate) enum SourceRangeSelection {
    Unselected,
    Mapped(Range<usize>),
    Unmapped,
}

impl SourceRangeSelection {
    pub(crate) fn merge(&mut self, other: Self) {
        match (&mut *self, other) {
            (_, Self::Unselected) => {}
            (_, Self::Unmapped) => *self = Self::Unmapped,
            (Self::Unselected, mapped @ Self::Mapped(_)) => *self = mapped,
            (Self::Mapped(selected), Self::Mapped(range)) => {
                selected.start = selected.start.min(range.start);
                selected.end = selected.end.max(range.end);
            }
            (Self::Unmapped, Self::Mapped(_)) => {}
        }
    }

    pub(crate) fn into_range(self) -> Option<Range<usize>> {
        match self {
            Self::Mapped(range) => Some(range),
            Self::Unselected | Self::Unmapped => None,
        }
    }
}

fn source_range_for_segments(
    segments: &[SourceSegment],
    selection: Range<usize>,
) -> Option<Range<usize>> {
    fn mapped_source_start(segment: &SourceSegment, rendered_start: usize) -> usize {
        if segment.linear {
            segment.source.start + rendered_start.saturating_sub(segment.rendered.start)
        } else {
            segment.source.start
        }
    }

    fn mapped_source_end(segment: &SourceSegment, rendered_end: usize) -> usize {
        if segment.linear {
            segment.source.start
                + rendered_end
                    .min(segment.rendered.end)
                    .saturating_sub(segment.rendered.start)
        } else {
            segment.source.end
        }
    }

    if selection.start >= selection.end {
        return None;
    }

    let mut overlapping = segments.iter().filter(|segment| {
        segment.rendered.start < selection.end && segment.rendered.end > selection.start
    });
    let first = overlapping.next()?;
    if first.rendered.start > selection.start {
        return None;
    }
    let mut rendered_end = first.rendered.end;
    let source_start = mapped_source_start(first, selection.start);
    let mut source_end = mapped_source_end(first, selection.end);
    for segment in overlapping {
        if segment.rendered.start > rendered_end {
            return None;
        }
        rendered_end = rendered_end.max(segment.rendered.end);
        source_end = mapped_source_end(segment, selection.end);
    }
    (rendered_end >= selection.end).then_some(source_start..source_end)
}

#[derive(Default, Clone, Debug)]
pub(crate) struct InlineNode {
    pub(crate) text: SharedString,
    pub(crate) image: Option<ImageNode>,
    pub(crate) custom: Option<MarkdownNode>,
    custom_selection: Arc<Mutex<bool>>,

    pub(crate) marks: Vec<(Range<usize>, TextMark)>,

    pub(crate) source_segments: Vec<SourceSegment>,

    pub(super) state: Arc<Mutex<InlineState>>,
}

impl PartialEq for InlineNode {
    fn eq(&self, other: &Self) -> bool {
        self.text == other.text
            && self.image == other.image
            && self.custom == other.custom
            && self.marks == other.marks
            && self.source_segments == other.source_segments
    }
}

pub(crate) fn wrap_with_mark(text: &str, mark: &TextMark) -> String {
    if text.is_empty() {
        return String::new();
    }

    let mut out = text.to_string();
    if mark.code {
        out = format!("`{}`", out);
    }
    if mark.italic {
        out = format!("*{}*", out);
    }
    if mark.bold {
        out = format!("**{}**", out);
    }
    if mark.strikethrough {
        out = format!("~~{}~~", out);
    }
    if mark.underline {
        // Markdown has no underline syntax, and `__` reads as bold to most
        // parsers, so fall back to the inline HTML `<u>` parses from.
        out = format!("<u>{}</u>", out);
    }
    if mark.highlight.is_some() {
        out = format!("=={}==", out);
    }
    if let Some(link) = &mark.link {
        out = match &link.title {
            Some(title) => format!("[{}]({} \"{}\")", out, link.url, title),
            None => format!("[{}]({})", out, link.url),
        };
    }
    out
}

#[derive(Default)]
struct MarkdownSource {
    pieces: Vec<(String, Vec<TextMark>)>,
}

impl MarkdownSource {
    fn push_str(&mut self, source: &str) {
        self.push_marked(source, &TextMark::default());
    }

    fn push_marked(&mut self, source: &str, mark: &TextMark) {
        if source.is_empty() {
            return;
        }
        // Outer-to-inner order for equally extensive marks. When a mark spans
        // more neighboring pieces, serialize it outside the shorter marks.
        let mut layers = Vec::new();
        if let Some(link) = &mark.link {
            layers.push(TextMark {
                link: Some(link.clone()),
                ..Default::default()
            });
        }
        if let Some(highlight) = mark.highlight {
            layers.push(TextMark {
                highlight: Some(highlight),
                ..Default::default()
            });
        }
        if mark.underline {
            layers.push(TextMark::default().underline());
        }
        if mark.strikethrough {
            layers.push(TextMark::default().strikethrough());
        }
        if mark.bold {
            layers.push(TextMark::default().bold());
        }
        if mark.italic {
            layers.push(TextMark::default().italic());
        }
        if mark.code {
            layers.push(TextMark::default().code());
        }
        self.pieces.push((source.to_string(), layers));
    }

    fn push_text(
        &mut self,
        text: &str,
        marks: &[(Range<usize>, TextMark)],
        selection: Range<usize>,
    ) {
        let start = selection.start.min(text.len());
        let end = selection.end.min(text.len());
        if start >= end {
            return;
        }
        let mut cursor = start;
        for (range, mark) in marks {
            let lo = range.start.max(start);
            let hi = range.end.min(end);
            if lo >= hi {
                continue;
            }
            if cursor < lo {
                self.push_str(&text[cursor..lo]);
            }
            self.push_marked(&text[lo..hi], mark);
            cursor = hi;
        }
        if cursor < end {
            self.push_str(&text[cursor..end]);
        }
    }

    fn push_object(&mut self, node: &InlineNode) {
        let mut mark = TextMark::default();
        for (_, layer) in &node.marks {
            mark.merge(layer.clone());
        }
        self.push_marked(&node.custom.as_ref().unwrap().to_markdown(), &mark);
    }

    fn is_empty(&self) -> bool {
        self.pieces.is_empty()
    }

    fn finish(mut self) -> String {
        fn write(pieces: &mut [(String, Vec<TextMark>)]) -> String {
            let mut out = String::new();
            let mut offset = 0;
            while offset < pieces.len() {
                let Some((mark, count)) = pieces[offset]
                    .1
                    .iter()
                    .map(|mark| {
                        let count = pieces[offset..]
                            .iter()
                            .take_while(|(_, layers)| layers.contains(mark))
                            .count();
                        (mark.clone(), count)
                    })
                    .reduce(|best, candidate| {
                        if candidate.1 > best.1 {
                            candidate
                        } else {
                            best
                        }
                    })
                else {
                    out.push_str(&pieces[offset].0);
                    offset += 1;
                    continue;
                };
                let group = &mut pieces[offset..offset + count];
                for (_, layers) in group.iter_mut() {
                    layers.retain(|layer| layer != &mark);
                }
                let content = write(group);
                out.push_str(&wrap_with_mark(&content, &mark));
                offset += count;
            }
            out
        }
        write(&mut self.pieces)
    }
}

#[derive(Default)]
struct RunSelection {
    emitted: bool,
    at_start: bool,
    at_end: bool,
}

fn emit_run(
    state: &Arc<Mutex<InlineState>>,
    run: &[(usize, &InlineNode)],
    pending_images: &mut Vec<String>,
    out: &mut MarkdownSource,
) -> RunSelection {
    let mut selected = RunSelection::default();
    let Ok(state) = state.lock() else {
        return selected;
    };
    let Some(selection) = &state.selection else {
        return selected;
    };
    if selection.start >= selection.end {
        return selected;
    }

    selected.at_start = selection.start == 0;
    selected.at_end = selection.end >= state.text.len();

    for (start, child) in run {
        let end = start + child.text.len();
        let lo = selection.start.max(*start);
        let hi = selection.end.min(end);
        if lo >= hi {
            continue;
        }

        if !selected.emitted {
            if selected.at_start {
                out.push_str(&pending_images.join(""));
            }
            pending_images.clear();
        }
        selected.emitted = true;

        out.push_text(&child.text, &child.marks, (lo - start)..(hi - start));
    }

    selected
}

fn image_markdown(image: &ImageNode) -> String {
    let alt = image.alt.clone().unwrap_or_default();
    let title = image
        .title
        .clone()
        .map_or(String::new(), |title| format!(" \"{}\"", title));
    format!("![{}]({}{})", alt, image.url, title)
}

#[cfg(test)]
pub(crate) fn reconstruct_markdown(
    text: &str,
    marks: &[(Range<usize>, TextMark)],
    selection: Range<usize>,
) -> String {
    let mut source = MarkdownSource::default();
    source.push_text(text, marks, selection);
    source.finish()
}

fn table_selected_source(table: &Table) -> String {
    let cell_source = |cell: &TableCell| cell.children.selected_source().replace('\n', " ");

    let any_selected = table.children.iter().any(|row| {
        row.children
            .iter()
            .any(|cell| !cell_source(cell).trim().is_empty())
    });
    if !any_selected {
        return String::new();
    }

    let mut lines: Vec<String> = Vec::new();
    for (row_ix, row) in table.children.iter().enumerate() {
        let cells: Vec<String> = row
            .children
            .iter()
            .map(|cell| cell_source(cell).trim().to_string())
            .collect();
        lines.push(format!("| {} |", cells.join(" | ")));

        // The Markdown delimiter row carries the column alignments and must
        // follow the header row.
        if row_ix == 0 {
            let aligns: Vec<String> = (0..row.children.len())
                .map(|ix| {
                    match table.column_align(ix) {
                        ColumnumnAlign::Left => ":--",
                        ColumnumnAlign::Center => ":-:",
                        ColumnumnAlign::Right => "--:",
                    }
                    .to_string()
                })
                .collect();
            lines.push(format!("| {} |", aligns.join(" | ")));
        }
    }

    lines.join("\n")
}

fn list_selected_source(
    children: &[BlockNode],
    ordered: bool,
    start: Option<u32>,
    indent: &str,
) -> String {
    let mut out = String::new();
    let mut item_ix = 0usize;

    for child in children {
        let BlockNode::ListItem {
            children: item_children,
            checked,
            ..
        } = child
        else {
            continue;
        };

        let marker = if ordered {
            format!("{}. ", ordered_list_ordinal(start, item_ix))
        } else {
            "- ".to_string()
        };
        let checkbox = match checked {
            Some(true) => "[x] ",
            Some(false) => "[ ] ",
            None => "",
        };
        let child_indent = format!("{}{}", indent, " ".repeat(marker.len()));

        // Split the item into its own content and any nested lists, so the
        // nested lists can be indented under the content.
        let mut content = String::new();
        let mut nested = String::new();
        for sub in item_children {
            if let BlockNode::List {
                children: sub_children,
                ordered: sub_ordered,
                start: sub_start,
                ..
            } = sub
            {
                nested.push_str(&list_selected_source(
                    sub_children,
                    *sub_ordered,
                    *sub_start,
                    &child_indent,
                ));
            } else {
                content.push_str(&sub.text_by_kind(BlockTextKind::SelectedSource));
            }
        }
        let content = content.trim_end_matches('\n');

        if content.is_empty() && nested.is_empty() {
            item_ix += 1;
            continue;
        }

        if content.is_empty() {
            // An item whose only selected content is a nested list.
            out.push_str(indent);
            out.push_str(&marker);
            out.push_str(checkbox.trim_end());
            out.push('\n');
        } else {
            // The first line carries the marker and checkbox; continuation
            // lines are indented to align under the item text.
            let mut lines = content.lines();
            if let Some(first) = lines.next() {
                out.push_str(indent);
                out.push_str(&marker);
                out.push_str(checkbox);
                out.push_str(first);
                out.push('\n');
            }
            for line in lines {
                out.push_str(&child_indent);
                out.push_str(line);
                out.push('\n');
            }
        }
        out.push_str(&nested);
        item_ix += 1;
    }

    out
}

impl InlineNode {
    pub(crate) fn new(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            image: None,
            custom: None,
            custom_selection: Arc::default(),
            marks: vec![],
            source_segments: vec![],
            state: Arc::new(Mutex::new(InlineState::default())),
        }
    }

    pub(crate) fn custom(node: MarkdownNode) -> Self {
        let mut this = Self::new(node.as_text().to_string());
        this.custom = Some(node);
        this
    }

    pub(crate) fn image(image: ImageNode) -> Self {
        let mut this = Self::new("");
        this.image = Some(image);
        this
    }

    pub(crate) fn marks(mut self, marks: Vec<(Range<usize>, TextMark)>) -> Self {
        self.marks = marks;
        self
    }

    pub(crate) fn source_segments(mut self, source_segments: Vec<SourceSegment>) -> Self {
        self.source_segments = source_segments;
        self
    }

    fn selected_source_range(&self, selection: Range<usize>) -> Option<Range<usize>> {
        source_range_for_segments(&self.source_segments, selection)
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Paragraph {
    pub(super) span: Option<Span>,
    pub(super) children: Vec<InlineNode>,

    pub(super) link_refs: HashMap<SharedString, SharedString>,

    pub(crate) state: Arc<Mutex<InlineState>>,

    pub(super) render_cache: ParagraphRenderCache,
}

#[derive(Default)]
pub(super) struct ParagraphRenderCache(Mutex<Option<Arc<ParagraphRender>>>);

impl Clone for ParagraphRenderCache {
    fn clone(&self) -> Self {
        let cached = self.0.lock().ok().and_then(|cache| cache.clone());
        Self(Mutex::new(cached))
    }
}

impl std::fmt::Debug for ParagraphRenderCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ParagraphRenderCache")
    }
}

struct ParagraphRender {
    style: Arc<TextViewStyle>,
    mono_font: SharedString,
    text: SharedString,
    highlights: Vec<(Range<usize>, InlineHighlight)>,

    links: Vec<(Range<usize>, LinkMark)>,
}

impl PartialEq for Paragraph {
    fn eq(&self, other: &Self) -> bool {
        self.span == other.span
            && self.children == other.children
            && self.link_refs == other.link_refs
    }
}

impl Paragraph {
    pub(crate) fn new(text: String) -> Self {
        Self {
            span: None,
            children: vec![InlineNode::new(&text)],
            link_refs: HashMap::new(),
            state: Arc::new(Mutex::new(InlineState::default())),
            render_cache: ParagraphRenderCache::default(),
        }
    }

    fn plain_render(
        &self,
        node_cx: &NodeContext,
        cx: &App,
    ) -> (
        SharedString,
        Vec<(Range<usize>, InlineHighlight)>,
        Vec<(Range<usize>, LinkMark)>,
    ) {
        let mono_font = cx.theme().tokens.typography.mono.clone();
        if let Ok(cache) = self.render_cache.0.lock()
            && let Some(cached) = cache.as_ref()
            && (Arc::ptr_eq(&cached.style, &node_cx.style) || *cached.style == *node_cx.style)
            && cached.mono_font == mono_font
        {
            return (
                cached.text.clone(),
                cached.highlights.clone(),
                cached.links.clone(),
            );
        }

        let mut text = String::new();
        let mut highlights: Vec<(Range<usize>, InlineHighlight)> = vec![];
        let mut links: Vec<(Range<usize>, LinkMark)> = vec![];
        let mut offset = 0;
        for inline_node in &self.children {
            let text_len = inline_node.text.len();
            text.push_str(&inline_node.text);
            let mut node_highlights = vec![];
            for (range, style) in &inline_node.marks {
                let inner_range = (offset + range.start)..(offset + range.end);
                let mut highlight = mark_highlight(style, node_cx, cx);
                if let Some(link_mark) = style.link.clone() {
                    highlight.style.color = Some(node_cx.style.link());
                    highlight.style.underline = Some(link_underline(&node_cx.style));
                    links.push((inner_range.clone(), link_mark));
                }
                node_highlights.push((inner_range, highlight));
            }
            highlights = combine_highlights(highlights, node_highlights);
            offset += text_len;
        }
        let text = SharedString::from(text);
        if let Ok(mut cache) = self.render_cache.0.lock() {
            *cache = Some(Arc::new(ParagraphRender {
                style: node_cx.style.clone(),
                mono_font,
                text: text.clone(),
                highlights: highlights.clone(),
                links: links.clone(),
            }));
        }
        (text, highlights, links)
    }

    pub(super) fn selected_text(&self) -> String {
        let mut text = String::new();

        for c in self.children.iter() {
            let Ok(state) = c.state.lock() else {
                continue;
            };
            if let Some(selection) = &state.selection {
                text.push_str(&state.text[selection.start..selection.end]);
            }
            if let Some(custom) = &c.custom
                && c.custom_selection.lock().is_ok_and(|selected| *selected)
            {
                text.push_str(custom.as_text());
            }
        }

        if let Ok(state) = self.state.lock()
            && let Some(selection) = &state.selection
        {
            text.push_str(&state.text[selection.start..selection.end]);
        }

        text
    }

    pub(super) fn selected_source_range(&self) -> SourceRangeSelection {
        let mut selected = SourceRangeSelection::Unselected;
        let mut run: Vec<(usize, &InlineNode)> = Vec::new();
        let mut offset = 0;
        let mut pending_images: Vec<Option<Span>> = Vec::new();
        let mut enters_image = true;

        let include_run = |state: &Arc<Mutex<InlineState>>,
                           run: &[(usize, &InlineNode)]|
         -> (SourceRangeSelection, RunSelection) {
            let Ok(state) = state.lock() else {
                return (SourceRangeSelection::Unmapped, RunSelection::default());
            };
            let Some(selection) = state.selection else {
                return (SourceRangeSelection::Unselected, RunSelection::default());
            };
            if selection.start >= selection.end {
                return (SourceRangeSelection::Unselected, RunSelection::default());
            }

            let mut run_selection = RunSelection {
                at_start: selection.start == 0,
                at_end: selection.end >= state.text.len(),
                ..Default::default()
            };
            let mut mapped = SourceRangeSelection::Unselected;
            let mut rendered_end = selection.start;
            for (start, child) in run {
                let end = start + child.text.len();
                let lo = selection.start.max(*start);
                let hi = selection.end.min(end);
                if lo >= hi {
                    continue;
                }
                run_selection.emitted = true;
                if lo > rendered_end {
                    return (SourceRangeSelection::Unmapped, run_selection);
                }
                let Some(range) = child.selected_source_range((lo - start)..(hi - start)) else {
                    return (SourceRangeSelection::Unmapped, run_selection);
                };
                mapped.merge(SourceRangeSelection::Mapped(range));
                rendered_end = rendered_end.max(hi);
            }
            if rendered_end < selection.end {
                (SourceRangeSelection::Unmapped, run_selection)
            } else {
                (mapped, run_selection)
            }
        };

        let merge_images = |selected: &mut SourceRangeSelection, images: &mut Vec<Option<Span>>| {
            for span in images.drain(..) {
                selected.merge(
                    span.map(|span| SourceRangeSelection::Mapped(span.start..span.end))
                        .unwrap_or(SourceRangeSelection::Unmapped),
                );
            }
        };

        for child in &self.children {
            if child.custom.is_some() {
                let (run_range, run_selection) = include_run(&child.state, &run);
                if run_selection.emitted && run_selection.at_start {
                    merge_images(&mut selected, &mut pending_images);
                }
                selected.merge(run_range);

                match child.custom_selection.lock() {
                    Ok(value) if *value => {
                        if run.is_empty() || (run_selection.emitted && run_selection.at_end) {
                            merge_images(&mut selected, &mut pending_images);
                        }
                        selected.merge(
                            child
                                .custom
                                .as_ref()
                                .and_then(MarkdownNode::source_range)
                                .map(SourceRangeSelection::Mapped)
                                .unwrap_or(SourceRangeSelection::Unmapped),
                        );
                        enters_image = true;
                    }
                    Ok(_) => enters_image = false,
                    Err(_) => {
                        selected.merge(SourceRangeSelection::Unmapped);
                        enters_image = false;
                    }
                }
                pending_images.clear();
                run.clear();
                offset = 0;
                continue;
            }
            if let Some(image) = &child.image {
                let run_before = !run.is_empty();
                let (run_range, run_selection) = include_run(&child.state, &run);
                if run_selection.emitted && run_selection.at_start {
                    merge_images(&mut selected, &mut pending_images);
                }
                selected.merge(run_range);
                if run_before {
                    enters_image = run_selection.emitted && run_selection.at_end;
                }
                if enters_image {
                    pending_images.push(image.span);
                } else {
                    pending_images.clear();
                }
                run.clear();
                offset = 0;
                continue;
            }
            run.push((offset, child));
            offset += child.text.len();
        }

        let (trailing_range, trailing) = include_run(&self.state, &run);
        if trailing.emitted && trailing.at_start {
            merge_images(&mut selected, &mut pending_images);
        }
        selected.merge(trailing_range);
        if !trailing.emitted
            && enters_image
            && !matches!(selected, SourceRangeSelection::Unselected)
        {
            merge_images(&mut selected, &mut pending_images);
        }
        selected
    }

    pub(super) fn selected_source(&self) -> String {
        let mut source = MarkdownSource::default();
        let mut pending_images: Vec<String> = Vec::new();
        let mut run: Vec<(usize, &InlineNode)> = Vec::new();
        let mut offset = 0;
        let mut enters_image = true;

        for child in self.children.iter() {
            if child.custom.is_some() {
                let selected = emit_run(&child.state, &run, &mut pending_images, &mut source);
                let object_selected = child
                    .custom_selection
                    .lock()
                    .is_ok_and(|selected| *selected);
                if object_selected {
                    if run.is_empty() || (selected.emitted && selected.at_end) {
                        source.push_str(&pending_images.join(""));
                    }
                    source.push_object(child);
                }
                pending_images.clear();
                enters_image = object_selected;
                run.clear();
                offset = 0;
                continue;
            }
            let Some(image) = &child.image else {
                run.push((offset, child));
                offset += child.text.len();
                continue;
            };

            // The run before an image is stored in that image's own state.
            let run_before = !run.is_empty();
            let selected = emit_run(&child.state, &run, &mut pending_images, &mut source);
            if run_before {
                enters_image = selected.emitted && selected.at_end;
            }
            if enters_image {
                pending_images.push(image_markdown(image));
            } else {
                pending_images.clear();
            }

            run.clear();
            offset = 0;
        }

        let trailing = emit_run(&self.state, &run, &mut pending_images, &mut source);
        // Trailing images have no run after them to flush them.
        if !trailing.emitted && enters_image && !source.is_empty() {
            source.push_str(&pending_images.join(""));
        }

        source.finish()
    }

    pub(super) fn text(&self) -> String {
        let mut text = String::new();
        for node in self.children.iter() {
            text.push_str(&node.text);
        }
        text
    }

    pub(super) fn has_selection(&self) -> bool {
        self.children.iter().any(|c| {
            c.state.lock().is_ok_and(|state| state.selection.is_some())
                || c.custom_selection.lock().is_ok_and(|selected| *selected)
        }) || self
            .state
            .lock()
            .is_ok_and(|state| state.selection.is_some())
    }

    pub(super) fn clear_selection(&self) {
        for c in self.children.iter() {
            if let Ok(mut selected) = c.custom_selection.lock() {
                *selected = false;
            }
            if let Ok(mut state) = c.state.lock() {
                state.selection = None;
            }
        }

        if let Ok(mut state) = self.state.lock() {
            state.selection = None;
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Table {
    pub(crate) children: Vec<TableRow>,
    pub(crate) column_aligns: Vec<ColumnumnAlign>,
    pub(crate) span: Option<Span>,

    pub(crate) table_data_cache: TableDataCache,
    pub(crate) column_widths_cache: TableColumnWidthsCache,
}

#[derive(Default)]
pub(crate) struct TableDataCache(Mutex<Option<Arc<TableData>>>);

impl Clone for TableDataCache {
    fn clone(&self) -> Self {
        let cached = self.0.lock().ok().and_then(|cache| cache.clone());
        Self(Mutex::new(cached))
    }
}

impl PartialEq for TableDataCache {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl std::fmt::Debug for TableDataCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TableDataCache")
    }
}

#[derive(Default)]
pub(crate) struct TableColumnWidthsCache(Mutex<Option<Arc<TableColumnWidths>>>);

impl Clone for TableColumnWidthsCache {
    fn clone(&self) -> Self {
        let cached = self.0.lock().ok().and_then(|cache| cache.clone());
        Self(Mutex::new(cached))
    }
}

impl PartialEq for TableColumnWidthsCache {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl std::fmt::Debug for TableColumnWidthsCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TableColumnWidthsCache")
    }
}

struct TableColumnWidths {
    text_system: Weak<gpui::WindowTextSystem>,

    text_styles: TableTextStyles,
    rem_size: Pixels,
    mono_family: SharedString,
    inline_code: HighlightStyle,

    widths: Vec<f32>,
    custom_cells: Vec<(usize, usize)>,
}

#[derive(Clone, PartialEq)]
struct TableTextStyles {
    head: gpui::TextStyle,
    body: gpui::TextStyle,
    head_refinement: TextStyleRefinement,
    body_refinement: TextStyleRefinement,
}

impl TableTextStyles {
    fn new(style: &TextViewStyle, window: &Window) -> Self {
        let mut body_refinement = style.table().text.clone();
        let mut head_refinement = body_refinement.clone();
        head_refinement.refine(&style.table_head().text);
        head_refinement.refine(&style.table_cell().text);
        body_refinement.refine(&style.table_cell().text);
        let mut head = window.text_style();
        let mut body = head.clone();
        head.refine(&head_refinement);
        body.refine(&body_refinement);
        Self {
            head,
            body,
            head_refinement,
            body_refinement,
        }
    }

    fn row(&self, row_ix: usize) -> &gpui::TextStyle {
        if row_ix == 0 { &self.head } else { &self.body }
    }

    fn row_refinement(&self, row_ix: usize) -> &TextStyleRefinement {
        if row_ix == 0 {
            &self.head_refinement
        } else {
            &self.body_refinement
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TableData {
    pub headers: Vec<String>,

    pub rows: Vec<Vec<String>>,

    pub markdown: String,

    pub span: Option<Range<usize>>,
}

impl Table {
    pub(crate) fn column_align(&self, index: usize) -> ColumnumnAlign {
        self.column_aligns.get(index).copied().unwrap_or_default()
    }

    pub(crate) fn to_markdown(&self) -> String {
        let mut lines: Vec<String> = Vec::with_capacity(self.children.len() + 1);

        for (row_ix, row) in self.children.iter().enumerate() {
            let cells: Vec<String> = row
                .children
                .iter()
                .map(|cell| {
                    cell.children
                        .to_markdown()
                        .trim()
                        .replace('\n', " ")
                        .replace('|', "\\|")
                })
                .collect();
            lines.push(format!("| {} |", cells.join(" | ")));

            // The Markdown delimiter row carries the column alignments and must
            // follow the header row.
            if row_ix == 0 {
                let aligns: Vec<String> = (0..row.children.len())
                    .map(|ix| {
                        match self.column_align(ix) {
                            ColumnumnAlign::Left => ":--",
                            ColumnumnAlign::Center => ":-:",
                            ColumnumnAlign::Right => "--:",
                        }
                        .to_string()
                    })
                    .collect();
                lines.push(format!("| {} |", aligns.join(" | ")));
            }
        }

        lines.join("\n")
    }

    pub(crate) fn table_data(&self) -> TableData {
        let row_text = |row: &TableRow| {
            row.children
                .iter()
                .map(|cell| cell.children.text().trim().to_string())
                .collect::<Vec<_>>()
        };

        TableData {
            headers: self.children.first().map(row_text).unwrap_or_default(),
            rows: self.children.iter().skip(1).map(row_text).collect(),
            markdown: self.to_markdown(),
            span: self.span.map(|span| span.start..span.end),
        }
    }

    fn cached_table_data(&self) -> Arc<TableData> {
        if let Ok(cache) = self.table_data_cache.0.lock()
            && let Some(cached) = cache.as_ref()
        {
            return cached.clone();
        }

        let data = Arc::new(self.table_data());
        if let Ok(mut cache) = self.table_data_cache.0.lock() {
            *cache = Some(data.clone());
        }
        data
    }
}

#[derive(Debug, Default, Copy, Clone, PartialEq)]
pub(crate) enum ColumnumnAlign {
    #[default]
    Left,
    Center,
    Right,
}

impl From<mdast::AlignKind> for ColumnumnAlign {
    fn from(value: mdast::AlignKind) -> Self {
        match value {
            mdast::AlignKind::None => ColumnumnAlign::Left,
            mdast::AlignKind::Left => ColumnumnAlign::Left,
            mdast::AlignKind::Center => ColumnumnAlign::Center,
            mdast::AlignKind::Right => ColumnumnAlign::Right,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct TableRow {
    pub children: Vec<TableCell>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct TableCell {
    pub children: Paragraph,
    pub width: Option<DefiniteLength>,
}

impl Paragraph {
    pub(crate) fn take(&mut self) -> Paragraph {
        std::mem::replace(
            self,
            Paragraph {
                span: None,
                children: vec![],
                link_refs: Default::default(),
                state: Arc::new(Mutex::new(InlineState::default())),
                render_cache: ParagraphRenderCache::default(),
            },
        )
    }

    pub(crate) fn is_image(&self) -> bool {
        false
    }

    pub(crate) fn set_span(&mut self, span: Span) {
        self.span = Some(span);
    }

    pub(crate) fn push_str(&mut self, text: &str) {
        self.children.push(
            InlineNode::new(text.to_string()).marks(vec![(0..text.len(), TextMark::default())]),
        );
        self.invalidate_render_cache();
    }

    pub(crate) fn push(&mut self, text: InlineNode) {
        self.children.push(text);
        self.invalidate_render_cache();
    }

    pub(crate) fn push_image(&mut self, image: ImageNode) {
        self.children.push(InlineNode::image(image));
        self.invalidate_render_cache();
    }

    fn invalidate_render_cache(&mut self) {
        self.render_cache = ParagraphRenderCache::default();
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.children.is_empty()
            || self
                .children
                .iter()
                .all(|node| node.text.is_empty() && node.image.is_none())
    }

    pub(crate) fn text_len(&self) -> usize {
        self.children
            .iter()
            .map(|node| node.text.len())
            .sum::<usize>()
    }

    pub(crate) fn merge(&mut self, other: Self) {
        self.children.extend(other.children);
        self.invalidate_render_cache();
    }
}

#[derive(Debug, Clone)]
pub struct CodeBlock {
    lang: Option<SharedString>,
    state: Arc<Mutex<InlineState>>,
    highlight_cache: Arc<Mutex<Option<CachedCodeBlockHighlights>>>,

    pub(crate) source_segments: Vec<SourceSegment>,
    pub span: Option<Span>,
}

struct CachedCodeBlockHighlights {
    highlighter: Arc<CodeBlockHighlighterFn>,
    styles: Vec<(Range<usize>, HighlightStyle)>,
}

impl std::fmt::Debug for CachedCodeBlockHighlights {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CachedCodeBlockHighlights")
            .field("styles", &self.styles)
            .finish_non_exhaustive()
    }
}

impl PartialEq for CodeBlock {
    fn eq(&self, other: &Self) -> bool {
        self.lang == other.lang && self.code() == other.code() && self.span == other.span
    }
}

impl CodeBlock {
    pub fn lang(&self) -> Option<SharedString> {
        self.lang.clone()
    }

    pub fn code(&self) -> SharedString {
        self.state
            .lock()
            .map(|state| state.text.clone())
            .unwrap_or_default()
    }

    pub fn from_code(code: impl Into<SharedString>, lang: Option<impl Into<SharedString>>) -> Self {
        Self::new(code.into(), lang.map(Into::into), None::<Span>)
    }

    pub(crate) fn new(
        code: SharedString,
        lang: Option<SharedString>,
        span: Option<impl Into<Span>>,
    ) -> Self {
        let state = Arc::new(Mutex::new(InlineState::default()));
        if let Ok(mut state) = state.lock() {
            state.set_text(code);
        }

        Self {
            lang,
            state,
            highlight_cache: Arc::new(Mutex::new(None)),
            source_segments: vec![],
            span: span.map(|s| s.into()),
        }
    }

    pub(crate) fn source_segments(mut self, source_segments: Vec<SourceSegment>) -> Self {
        self.source_segments = source_segments;
        self
    }

    #[cfg(test)]
    pub(crate) fn set_selection(&self, selection: Range<usize>) {
        if let Ok(mut state) = self.state.lock() {
            state.selection = Some(selection.into());
        }
    }

    pub(super) fn selected_source_range(&self) -> SourceRangeSelection {
        let Ok(state) = self.state.lock() else {
            return SourceRangeSelection::Unmapped;
        };
        let Some(selection) = state.selection else {
            return SourceRangeSelection::Unselected;
        };
        if selection.start >= selection.end {
            return SourceRangeSelection::Unselected;
        }
        source_range_for_segments(&self.source_segments, selection.start..selection.end)
            .map(SourceRangeSelection::Mapped)
            .unwrap_or(SourceRangeSelection::Unmapped)
    }

    fn highlighted_styles(
        &self,
        highlighter: &Arc<CodeBlockHighlighterFn>,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        if let Ok(cache) = self.highlight_cache.lock()
            && let Some(cache) = cache.as_ref()
            && Arc::ptr_eq(&cache.highlighter, highlighter)
        {
            return cache.styles.clone();
        }

        let code_len = self.code().len();
        let styles = highlighter(self)
            .into_iter()
            .filter(|(range, _)| range.start <= range.end && range.end <= code_len)
            .collect::<Vec<_>>();
        if let Ok(mut cache) = self.highlight_cache.lock() {
            *cache = Some(CachedCodeBlockHighlights {
                highlighter: highlighter.clone(),
                styles: styles.clone(),
            });
        }
        styles
    }

    pub(super) fn selected_text(&self) -> String {
        let mut text = String::new();
        if let Ok(state) = self.state.lock()
            && let Some(selection) = &state.selection
        {
            text.push_str(&state.text[selection.start..selection.end]);
        }
        text
    }

    pub(super) fn selected_source(&self) -> String {
        let code = self.selected_text();
        if code.is_empty() {
            return String::new();
        }
        let lang = self.lang.clone().unwrap_or_default();
        // Trim trailing newlines so the closing fence sits on its own line
        // directly after the last code line (no blank line before it).
        let code = code.trim_end_matches('\n');
        format!("```{}\n{}\n```", lang, code)
    }

    pub(super) fn text(&self) -> String {
        self.state
            .lock()
            .map(|state| state.text.to_string())
            .unwrap_or_default()
    }

    pub(super) fn has_selection(&self) -> bool {
        self.state
            .lock()
            .is_ok_and(|state| state.selection.is_some())
    }

    pub(super) fn clear_selection(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.selection = None;
        }
    }

    fn render(
        &self,
        options: &NodeRenderOptions,
        node_cx: &NodeContext,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let style = &node_cx.style;
        let leaf_key = self.span.map(|span| TextLeafKey::block(span.start));
        // The language sits quietly in the top-right corner, where custom
        // actions go instead when the caller provides them; the top padding
        // keeps the first line clear of it. Like the actions, it stays out of
        // the scrolled content so it holds its corner.
        let lang_label = self
            .lang()
            .filter(|lang| !lang.is_empty() && node_cx.code_block_actions.is_none())
            .map(|lang| {
                div()
                    .absolute()
                    .top(px(7.))
                    .right_3()
                    .font_family(cx.theme().tokens.typography.sans.clone())
                    .text_size(cx.theme().tokens.typography.xs.size)
                    .line_height(px(16.))
                    .text_color(style.muted_foreground())
                    .child(lang)
            });

        let block = div()
            .w_full()
            .min_w_0()
            .px_4()
            .pt(if lang_label.is_some() {
                px(28.)
            } else {
                px(14.)
            })
            .pb(px(14.))
            .bg(style.code_background())
            .border_1()
            .border_color(style.border())
            .font_family(cx.theme().tokens.typography.mono.clone())
            .text_size(cx.theme().tokens.typography.mono_md.size)
            .line_height(relative(1.6))
            .relative()
            .refine_style(&style.code_block())
            .child(
                Inline::new(
                    self.state.clone(),
                    vec![],
                    fade_highlights(
                        node_cx
                            .code_block_highlighter
                            .as_ref()
                            .map(|highlighter| self.highlighted_styles(highlighter))
                            .unwrap_or_default()
                            .into_iter()
                            .map(|(range, style)| (range, InlineHighlight::from(style)))
                            .collect(),
                        node_cx.stream_fades(leaf_key),
                    ),
                    node_cx.link_click_handler.clone(),
                )
                .range_backgrounds(node_cx.range_backgrounds(leaf_key).to_vec())
                .reveal(node_cx.reveal_at(leaf_key, 0, self.code().len())),
            );
        let actions = node_cx.code_block_actions.clone().map(|actions| {
            div()
                .id("actions")
                .absolute()
                .top_2()
                .right_2()
                .bg(style.code_background())
                .rounded(cx.theme().tokens.radius.md)
                .child(actions(&self, window, cx))
        });
        // The id scopes the caller's action ids per code block, so plain ids
        // like `"copy"` don't collide across blocks.
        let id = block_element_id("codeblock", self.span, options.ix);
        let block = if matches!(style.code_block().overflow.y, Some(Overflow::Scroll)) {
            let scroll_handle = window
                .use_keyed_state(
                    block_element_id("codeblock-scroll", self.span, options.ix),
                    cx,
                    |_, _| ScrollHandle::default(),
                )
                .read(cx)
                .clone();
            // Scroll mode is opted in by setting `style.code_block`'s
            // `overflow.y` to `Overflow::Scroll`.
            // The mask consumes the wheel in the capture phase, so an ancestor
            // `gpui::list` doesn't scroll on the same event. Mask, scrollbar
            // and actions are siblings of the scrolled block, so they stay
            // pinned to the viewport instead of moving with the code.
            div()
                .id(id.clone())
                .w_full()
                .min_w_0()
                .relative()
                .child(block.id("scroll").track_scroll(&scroll_handle))
                .child(ScrollableMask::new(Axis::Vertical, &scroll_handle).id(id))
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .child(Scrollbar::vertical(&scroll_handle)),
                )
                .children(actions)
                .children(lang_label)
                .into_any_element()
        } else {
            // Without actions nothing under the block needs element state.
            match actions {
                Some(actions) => block.id(id).child(actions).into_any_element(),
                None => block.children(lang_label).into_any_element(),
            }
        };

        gapped(
            block,
            if options.is_last {
                rems(0.)
            } else {
                style.paragraph_gap()
            },
        )
    }
}

#[derive(Default, Clone)]
pub(crate) struct NodeContext {
    pub(crate) offset: usize,
    pub(crate) link_refs: HashMap<SharedString, LinkMark>,
    pub(crate) style: Arc<TextViewStyle>,
    pub(crate) code_block_actions: Option<Arc<CodeBlockActionsFn>>,
    pub(crate) code_block_highlighter: Option<Arc<CodeBlockHighlighterFn>>,
    pub(crate) table_actions: Option<Arc<TableActionsFn>>,
    pub(crate) image_source: Option<Arc<super::text_view::ImageSourceFn>>,
    pub(crate) link_click_handler: Option<Arc<LinkClickHandlerFn>>,
    pub(crate) markdown_extensions: Arc<MarkdownExtensions>,

    pub(crate) stream_fade: Option<Arc<StreamFadeFrame>>,

    pub(crate) range_highlights: Option<Arc<RangeHighlightFrame>>,

    pub(crate) reveal: Option<RevealRequest>,

    pub(crate) body_font_size: Pixels,
}

impl NodeContext {
    fn image_source(&self, image: &ImageNode) -> ImageSource {
        match &self.image_source {
            Some(resolve) => resolve(&image.url),
            None => image.source(),
        }
    }

    pub(super) fn add_ref(&mut self, identifier: SharedString, link: LinkMark) {
        self.link_refs.insert(identifier, link);
    }

    fn stream_fades(&self, key: Option<TextLeafKey>) -> &[(Range<usize>, f32)] {
        match (&self.stream_fade, key) {
            (Some(frame), Some(key)) => frame.fades(key).unwrap_or_default(),
            _ => &[],
        }
    }

    fn range_backgrounds(&self, key: Option<TextLeafKey>) -> &[(Range<usize>, Hsla)] {
        match (&self.range_highlights, key) {
            (Some(frame), Some(key)) => frame.backgrounds(key),
            _ => &[],
        }
    }

    fn reveal_at(&self, key: Option<TextLeafKey>, start: usize, end: usize) -> Option<RevealAt> {
        self.reveal.as_ref()?.at(key, start, end)
    }
}

impl PartialEq for NodeContext {
    fn eq(&self, other: &Self) -> bool {
        self.link_refs == other.link_refs && self.style == other.style
        // Note: code_block_actions, table_actions and markdown_extensions are
        // intentionally not compared (closures can't be compared)
    }
}

fn mark_highlight(mark: &TextMark, node_cx: &NodeContext, cx: &App) -> InlineHighlight {
    let mut highlight = HighlightStyle::default();
    if mark.bold {
        highlight.font_weight = Some(FontWeight::BOLD);
    }
    if mark.italic {
        highlight.font_style = Some(FontStyle::Italic);
    }
    if mark.strikethrough {
        highlight.strikethrough = Some(gpui::StrikethroughStyle {
            thickness: gpui::px(1.),
            ..Default::default()
        });
    }
    if mark.underline {
        highlight.underline = Some(gpui::UnderlineStyle {
            thickness: gpui::px(1.),
            ..Default::default()
        });
    }
    let mut font_family = None;
    if mark.code {
        highlight = highlight.highlight(node_cx.style.inline_code_highlight());
        font_family = Some(cx.theme().tokens.typography.mono.clone());
    }
    if let Some(color) = mark.highlight {
        highlight.background_color = Some(color);
    }
    InlineHighlight {
        style: highlight,
        font_family,
        font_size_scale: mark.code.then_some(0.875),
    }
}

impl Paragraph {
    fn inline_highlights(
        &self,
        node_cx: &NodeContext,
        cx: &App,
    ) -> Vec<(Range<usize>, InlineHighlight)> {
        let mut highlights = vec![];
        let mut offset = 0;
        for inline_node in &self.children {
            let node_highlights = inline_node
                .marks
                .iter()
                .map(|(range, mark)| {
                    (
                        (offset + range.start)..(offset + range.end),
                        mark_highlight(mark, node_cx, cx),
                    )
                })
                .collect::<Vec<_>>();
            highlights = combine_highlights(highlights, node_highlights);
            offset += inline_node.text.len();
        }
        highlights
    }

    fn render(
        &self,
        fade_key: Option<TextLeafKey>,
        node_cx: &NodeContext,
        _window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let children = &self.children;
        let fades = node_cx.stream_fades(fade_key);
        let backgrounds = node_cx.range_backgrounds(fade_key);

        if self.should_render_inline_flow() {
            return InlineFlow::new(
                leaf_element_id(fade_key),
                self.inline_flow_items(fade_key, fades, backgrounds, node_cx, cx),
                node_cx.link_click_handler.clone(),
            )
            .into_any_element();
        }

        let has_image = children.iter().any(|child| child.image.is_some());
        // Text alone is one `Inline`, which needs no box of its own, and its
        // text, highlights and links are cached across frames.
        if !has_image {
            let (text, highlights, mut links) = self.plain_render(node_cx, cx);
            if text.is_empty() {
                return div().into_any_element();
            }
            for (_, link_mark) in &mut links {
                if let Some(identifier) = link_mark.identifier.as_ref()
                    && let Some(mark) = node_cx.link_refs.get(identifier)
                {
                    *link_mark = mark.clone();
                }
            }
            let highlights = fade_highlights(highlights, &slice_fades(fades, 0, text.len()));
            let backgrounds = slice_backgrounds(backgrounds, 0, text.len());
            let reveal = node_cx.reveal_at(fade_key, 0, text.len());
            if let Ok(mut state) = self.state.lock() {
                state.set_text(text);
            }
            return Inline::new(
                self.state.clone(),
                links,
                highlights,
                node_cx.link_click_handler.clone(),
            )
            .range_backgrounds(backgrounds)
            .reveal(reveal)
            .into_any_element();
        }

        let mut child_nodes: Vec<AnyElement> = vec![];

        let mut text = String::new();
        let mut highlights: Vec<(Range<usize>, InlineHighlight)> = vec![];
        let mut links: Vec<(Range<usize>, LinkMark)> = vec![];
        let mut offset = 0;
        // Where `text` starts in the paragraph's whole rendered text, which
        // is the byte space the fade ranges use.
        let mut consumed = 0;

        for (ix, inline_node) in children.iter().enumerate() {
            let text_len = inline_node.text.len();
            text.push_str(&inline_node.text);

            if let Some(image) = &inline_node.image {
                if text.len() > 0 {
                    if let Ok(mut state) = inline_node.state.lock() {
                        state.set_text(text.clone().into());
                    }
                    child_nodes.push(
                        Inline::new(
                            inline_node.state.clone(),
                            links.clone(),
                            fade_highlights(
                                highlights.clone(),
                                &slice_fades(fades, consumed, consumed + text.len()),
                            ),
                            node_cx.link_click_handler.clone(),
                        )
                        .range_backgrounds(slice_backgrounds(
                            backgrounds,
                            consumed,
                            consumed + text.len(),
                        ))
                        .reveal(node_cx.reveal_at(fade_key, consumed, consumed + text.len()))
                        .into_any_element(),
                    );
                }
                let link_click_handler = node_cx.link_click_handler.clone();
                child_nodes.push(
                    img(node_cx.image_source(image))
                        .id(ix)
                        .object_fit(ObjectFit::Contain)
                        .max_w(relative(1.))
                        .when_some(image.width, |this, width| this.w(width))
                        .when_some(image.link.clone(), |this, link| {
                            let link_click_handler = link_click_handler.clone();
                            let aux_link = link.clone();
                            let aux_link_click_handler = link_click_handler.clone();
                            this.cursor_pointer()
                                .on_click(move |event, window, cx| {
                                    crate::TextSelection::end(window, cx);
                                    cx.stop_propagation();
                                    handle_link_click(
                                        &link_click_handler,
                                        link.url.clone(),
                                        event.clone(),
                                        window,
                                        cx,
                                    );
                                })
                                .on_aux_click(move |event, window, cx| {
                                    crate::TextSelection::end(window, cx);
                                    cx.stop_propagation();
                                    handle_link_click(
                                        &aux_link_click_handler,
                                        aux_link.url.clone(),
                                        event.clone(),
                                        window,
                                        cx,
                                    );
                                })
                        })
                        .into_any_element(),
                );

                consumed += text.len();
                text.clear();
                links.clear();
                highlights.clear();
                offset = 0;
            } else {
                let mut node_highlights = vec![];
                for (range, style) in &inline_node.marks {
                    let inner_range = (offset + range.start)..(offset + range.end);
                    let mut highlight = mark_highlight(style, node_cx, cx);

                    if let Some(mut link_mark) = style.link.clone() {
                        highlight.style.color = Some(node_cx.style.link());
                        highlight.style.underline = Some(link_underline(&node_cx.style));

                        // convert link references, replace link
                        if let Some(identifier) = link_mark.identifier.as_ref() {
                            if let Some(mark) = node_cx.link_refs.get(identifier) {
                                link_mark = mark.clone();
                            }
                        }

                        links.push((inner_range.clone(), link_mark));
                    }

                    node_highlights.push((inner_range, highlight));
                }

                highlights = combine_highlights(highlights, node_highlights);
                offset += text_len;
            }
        }

        // Add the last text node
        if text.len() > 0 {
            let text_end = consumed + text.len();
            let highlights = fade_highlights(highlights, &slice_fades(fades, consumed, text_end));
            if let Ok(mut state) = self.state.lock() {
                state.set_text(text.into());
            }
            child_nodes.push(
                Inline::new(
                    self.state.clone(),
                    links,
                    highlights,
                    node_cx.link_click_handler.clone(),
                )
                .range_backgrounds(slice_backgrounds(backgrounds, consumed, text_end))
                .reveal(node_cx.reveal_at(fade_key, consumed, text_end))
                .into_any_element(),
            );
        }

        // Text alone is one `Inline`, which needs no box of its own. Images
        // keep an identified box: an image's element state is its animation,
        // and the box scopes that state per paragraph.
        if !has_image {
            return child_nodes
                .pop()
                .unwrap_or_else(|| div().into_any_element());
        }

        div()
            .id(leaf_element_id(fade_key))
            .children(child_nodes)
            .into_any_element()
    }

    fn should_render_inline_flow(&self) -> bool {
        let has_image = self.children.iter().any(|child| child.image.is_some());
        let has_text = self.children.iter().any(|child| !child.text.is_empty());
        self.children.iter().any(|child| child.custom.is_some())
            || (has_image && has_text)
            || self
                .children
                .iter()
                .any(|child| child.marks.iter().any(|(_, mark)| mark.code))
    }

    fn inline_flow_items(
        &self,
        leaf_key: Option<TextLeafKey>,
        fades: &[(Range<usize>, f32)],
        backgrounds: &[(Range<usize>, Hsla)],
        node_cx: &NodeContext,
        cx: &mut App,
    ) -> Vec<InlineFlowItem> {
        let mut items = Vec::new();
        let mut text = String::new();
        let mut highlights: Vec<(Range<usize>, InlineHighlight)> = vec![];
        let mut links: Vec<(Range<usize>, LinkMark)> = vec![];
        let mut offset = 0;
        // Where `text` starts in the paragraph's whole rendered text, which
        // is the byte space the fade and background ranges use.
        let mut consumed = 0;

        for inline_node in &self.children {
            if let Some(node) = &inline_node.custom {
                if let Ok(mut state) = inline_node.state.lock() {
                    state.set_text(text.clone().into());
                }
                if !text.is_empty() {
                    let item_fades = slice_fades(fades, consumed, consumed + text.len());
                    let item_backgrounds =
                        slice_backgrounds(backgrounds, consumed, consumed + text.len());
                    let item_reveal = node_cx.reveal_at(leaf_key, consumed, consumed + text.len());
                    consumed += text.len();
                    items.push(InlineFlowItem::Text {
                        state: inline_node.state.clone(),
                        text: std::mem::take(&mut text).into(),
                        links: std::mem::take(&mut links),
                        highlights: fade_highlights(std::mem::take(&mut highlights), &item_fades),
                        backgrounds: item_backgrounds,
                        reveal: item_reveal,
                    });
                }
                let mut object_style = HighlightStyle::default();
                let mut object_link = None;
                for (_, mark) in &inline_node.marks {
                    object_style = object_style.highlight(mark_highlight(mark, node_cx, cx).style);
                    if let Some(link) = &mark.link {
                        object_link = Some(
                            link.identifier
                                .as_ref()
                                .and_then(|id| node_cx.link_refs.get(id))
                                .unwrap_or(link)
                                .clone(),
                        );
                        object_style.color = Some(node_cx.style.link());
                        object_style.underline = Some(link_underline(&node_cx.style));
                    }
                }
                let rendered_node = node.clone();
                let extensions = node_cx.markdown_extensions.clone();
                items.push(InlineFlowItem::Object {
                    text: node.shared_text(),
                    accessibility_label: node.shared_accessibility_name(),
                    id: node.source_range().map_or(items.len(), |range| range.start),
                    renderer: Arc::new(move |context, window, cx| {
                        extensions.render_inline(&rendered_node, context, window, cx)
                    }),
                    selected: inline_node.custom_selection.clone(),
                    style: object_style,
                    link: object_link,
                });
                consumed += inline_node.text.len();
                offset = 0;
                continue;
            }
            let text_len = inline_node.text.len();
            text.push_str(&inline_node.text);

            if let Some(image) = &inline_node.image {
                if !text.is_empty() {
                    if let Ok(mut state) = inline_node.state.lock() {
                        state.set_text(text.clone().into());
                    }
                    items.push(InlineFlowItem::Text {
                        state: inline_node.state.clone(),
                        text: text.clone().into(),
                        links: links.clone(),
                        highlights: fade_highlights(
                            highlights.clone(),
                            &slice_fades(fades, consumed, consumed + text.len()),
                        ),
                        backgrounds: slice_backgrounds(
                            backgrounds,
                            consumed,
                            consumed + text.len(),
                        ),
                        reveal: node_cx.reveal_at(leaf_key, consumed, consumed + text.len()),
                    });
                }

                items.push(InlineFlowItem::Image {
                    source: node_cx.image_source(image),
                    link: image.link.clone(),
                    title: image.title(),
                    width: image.width,
                    height: image.height,
                });

                consumed += text.len();
                text.clear();
                links.clear();
                highlights.clear();
                offset = 0;
            } else {
                let mut node_highlights = vec![];
                for (range, style) in &inline_node.marks {
                    let inner_range = (offset + range.start)..(offset + range.end);
                    let mut highlight = mark_highlight(style, node_cx, cx);

                    if let Some(mut link_mark) = style.link.clone() {
                        highlight.style.color = Some(node_cx.style.link());
                        highlight.style.underline = Some(link_underline(&node_cx.style));

                        if let Some(identifier) = link_mark.identifier.as_ref()
                            && let Some(mark) = node_cx.link_refs.get(identifier)
                        {
                            link_mark = mark.clone();
                        }

                        links.push((inner_range.clone(), link_mark));
                    }

                    node_highlights.push((inner_range, highlight));
                }

                highlights = combine_highlights(highlights, node_highlights);
                offset += text_len;
            }
        }

        if !text.is_empty() {
            if let Ok(mut state) = self.state.lock() {
                state.set_text(text.clone().into());
            }
            let highlights = fade_highlights(
                highlights,
                &slice_fades(fades, consumed, consumed + text.len()),
            );
            let backgrounds = slice_backgrounds(backgrounds, consumed, consumed + text.len());
            let reveal = node_cx.reveal_at(leaf_key, consumed, consumed + text.len());
            items.push(InlineFlowItem::Text {
                state: self.state.clone(),
                text: text.into(),
                links,
                highlights,
                backgrounds,
                reveal,
            });
        }

        items
    }
}

fn block_element_id(kind: &'static str, span: Option<Span>, ix: usize) -> ElementId {
    (kind, span.map_or(ix, |span| span.start)).into()
}

fn leaf_element_id(fade_key: Option<TextLeafKey>) -> ElementId {
    fade_key.map_or_else(|| ElementId::from("p"), ElementId::from)
}

fn link_underline(style: &TextViewStyle) -> gpui::UnderlineStyle {
    gpui::UnderlineStyle {
        thickness: px(1.),
        color: Some(style.link().opacity(0.4)),
        ..Default::default()
    }
}

fn body_font_size(node_cx: &NodeContext, window: &Window) -> Pixels {
    if node_cx.body_font_size > Pixels::ZERO {
        node_cx.body_font_size
    } else {
        window.text_style().font_size.to_pixels(window.rem_size())
    }
}

fn heading_metrics(level: u8) -> (f32, f32, f32) {
    match level {
        1 => (1.8, 1.25, 1.1),
        2 => (4. / 3., 1.35, 1.6),
        3 => (17. / 15., 1.35, 1.4),
        4 => (1., 1.35, 1.4),
        5 => (14. / 15., 1.35, 1.4),
        _ => (13. / 15., 1.35, 1.4),
    }
}

const HEADING_BOTTOM_GAP: f32 = 0.35;

fn heading_size(level: u8, node_cx: &NodeContext, window: &Window) -> Pixels {
    node_cx.style.heading(level).text.font_size.map_or_else(
        || body_font_size(node_cx, window) * heading_metrics(level).0,
        |size| size.to_pixels(window.rem_size()),
    )
}

fn rule_space(body_font_size: Pixels) -> Pixels {
    body_font_size * 1.6
}

fn block_gap(options: &NodeRenderOptions, style: &TextViewStyle) -> Rems {
    if options.in_list || options.is_last {
        rems(0.)
    } else {
        style.paragraph_gap()
    }
}

fn gapped(block: AnyElement, gap: Rems) -> AnyElement {
    if gap.is_zero() {
        block
    } else {
        div().pb(gap).child(block).into_any_element()
    }
}

fn slice_fades(
    fades: &[(Range<usize>, f32)],
    start: usize,
    end: usize,
) -> Vec<(Range<usize>, f32)> {
    slice_ranges(fades, start, end, |range, fade_out| (range, *fade_out))
}

fn slice_backgrounds(
    backgrounds: &[(Range<usize>, Hsla)],
    start: usize,
    end: usize,
) -> Vec<(Range<usize>, Hsla)> {
    slice_ranges(backgrounds, start, end, |range, color| (range, *color))
}

struct TableColumn {
    max: f32,

    min: f32,
    divider: f32,
}

fn table_cell_padding_x(style: &TextViewStyle, rem_size: Pixels) -> f32 {
    let padding = &style.table_cell().padding;
    let side = |length: Option<DefiniteLength>| match length {
        Some(DefiniteLength::Absolute(length)) => f32::from(length.to_pixels(rem_size)),
        _ => CELL_PAD_PX / 2.,
    };
    side(padding.left) + side(padding.right)
}

const CELL_PAD_PX: f32 = 24.0; // px_3 horizontal padding
const CELL_MIN_PX: f32 = 48.0;
const CELL_BORDER_PX: f32 = 1.0; // border_r_1 drawn by every column but the last

fn measure_table_columns(
    table: &Table,
    col_count: usize,
    node_cx: &NodeContext,
    window: &mut Window,
    cx: &mut App,
) -> Vec<f32> {
    let text_styles = TableTextStyles::new(&node_cx.style, window);
    let rem_size = window.rem_size();
    let mono_family = cx.theme().tokens.typography.mono.clone();
    let inline_code = node_cx.style.inline_code_highlight();
    let cached = table.column_widths_cache.0.lock().ok().and_then(|cache| {
        cache
            .as_ref()
            .filter(|cached| {
                cached.widths.len() == col_count
                    && cached.text_styles == text_styles
                    && cached.rem_size == rem_size
                    && cached.mono_family == mono_family
                    && cached.inline_code == inline_code
                    && cached
                        .text_system
                        .upgrade()
                        .is_some_and(|system| Arc::ptr_eq(&system, window.text_system()))
            })
            .cloned()
    });
    let measured = cached.unwrap_or_else(|| {
        let (widths, custom_cells) =
            measure_table_text_columns(table, col_count, &text_styles, node_cx, window, cx);
        let measured = Arc::new(TableColumnWidths {
            text_system: Arc::downgrade(window.text_system()),
            text_styles: text_styles.clone(),
            rem_size,
            mono_family,
            inline_code,
            widths,
            custom_cells,
        });
        if let Ok(mut cache) = table.column_widths_cache.0.lock() {
            *cache = Some(measured.clone());
        }
        measured
    });

    // Resource-backed cells must be measured again, even when their text is
    // unchanged. Keep callbacks outside the cache lock and do not retain their
    // widths: a loaded image or a plugin can get narrower as well as wider.
    let mut col_w = measured.widths.clone();
    for &(row_ix, ix) in &measured.custom_cells {
        let cell = &table.children[row_ix].children[ix];
        let items = cell.children.inline_flow_items(None, &[], &[], node_cx, cx);
        let style = text_styles.row_refinement(row_ix).clone();
        let width = window.with_text_style(Some(style), |window| {
            super::inline_flow::intrinsic_width(&items, window, cx)
        });
        let border = if ix + 1 < col_count {
            CELL_BORDER_PX
        } else {
            0.
        };
        col_w[ix] = col_w[ix].max(f32::from(width) + CELL_PAD_PX + border);
    }
    col_w
}

// The first measurement still visits all ordinary cells. This cache avoids
// repeating that work; it is not a budget for the initial table layout.
fn measure_table_text_columns(
    table: &Table,
    col_count: usize,
    text_styles: &TableTextStyles,
    node_cx: &NodeContext,
    window: &mut Window,
    cx: &mut App,
) -> (Vec<f32>, Vec<(usize, usize)>) {
    let mut col_w = vec![CELL_MIN_PX; col_count];
    let mut custom_cells = Vec::new();
    for (row_ix, row) in table.children.iter().enumerate() {
        let text_style = text_styles.row(row_ix);
        let font_size = text_style.font_size.to_pixels(window.rem_size());
        for (ix, cell) in row.children.iter().enumerate() {
            let Some(slot) = col_w.get_mut(ix) else {
                continue;
            };
            if cell
                .children
                .children
                .iter()
                .any(|node| node.custom.is_some())
            {
                custom_cells.push((row_ix, ix));
                continue;
            }
            let text = cell.children.text();
            let highlights = cell.children.inline_highlights(node_cx, cx);
            let mut w = 0.0_f32;
            let mut line_start = 0;
            for line in text.split('\n') {
                let start = line_start + (line.len() - line.trim_start().len());
                let line_end = line_start + line.len();
                line_start = line_end + 1;
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let end = start + line.len();
                let line_highlights = highlights
                    .iter()
                    .filter_map(|(range, highlight)| {
                        let clipped = range.start.max(start)..range.end.min(end);
                        (clipped.start < clipped.end).then(|| {
                            (
                                clipped.start - start..clipped.end - start,
                                highlight.clone(),
                            )
                        })
                    })
                    .collect::<Vec<_>>();
                let mut line_w = gpui::Pixels::ZERO;
                for (range, scale) in text_size_ranges(line.len(), &line_highlights) {
                    let highlights = slice_ranges(
                        &line_highlights,
                        range.start,
                        range.end,
                        |range, highlight| (range, highlight.clone()),
                    );
                    if highlights.iter().any(|(_, h)| h.font_size_scale.is_some()) {
                        line_w += px(crate::text::inline_flow::INLINE_CODE_PADDING * 2.);
                    }
                    let runs = text_runs(range.len(), text_style, &highlights);
                    line_w += window
                        .text_system()
                        .layout_line(&line[range], font_size * scale, &runs, None)
                        .width;
                }
                w = w.max(f32::from(line_w));
            }
            // Border-box widths, so the padding and border the cell draws
            // must leave the measured text its full width.
            let border = if ix + 1 < col_count {
                CELL_BORDER_PX
            } else {
                0.
            };
            *slot = slot.max(w + CELL_PAD_PX + border);
        }
    }
    (col_w, custom_cells)
}

impl Paragraph {
    fn to_markdown(&self) -> String {
        if self.children.iter().any(|node| node.custom.is_some()) {
            let mut source = MarkdownSource::default();
            for node in &self.children {
                if node.custom.is_some() {
                    source.push_object(node);
                } else {
                    source.push_text(&node.text, &node.marks, 0..node.text.len());
                    if let Some(image) = &node.image {
                        source.push_str(&image_markdown(image));
                    }
                }
            }
            let mut text = source.finish();
            text.push_str("\n\n");
            return text;
        }
        let mut text = self
            .children
            .iter()
            .map(|text_node| {
                let mut text = text_node.text.to_string();
                for (range, style) in &text_node.marks {
                    if style.bold {
                        text = format!("**{}**", &text_node.text[range.clone()]);
                    }
                    if style.italic {
                        text = format!("*{}*", &text_node.text[range.clone()]);
                    }
                    if style.strikethrough {
                        text = format!("~~{}~~", &text_node.text[range.clone()]);
                    }
                    if style.code {
                        text = format!("`{}`", &text_node.text[range.clone()]);
                    }
                    if style.highlight.is_some() {
                        text = format!("=={}==", &text_node.text[range.clone()]);
                    }
                    if let Some(link) = &style.link {
                        text = format!("[{}]({})", &text_node.text[range.clone()], link.url);
                    }
                }

                if let Some(image) = &text_node.image {
                    let alt = image.alt.clone().unwrap_or_default();
                    let title = image
                        .title
                        .clone()
                        .map_or(String::new(), |t| format!(" \"{}\"", t));
                    text.push_str(&format!("![{}]({}{})", alt, image.url, title))
                }

                text
            })
            .collect::<Vec<_>>()
            .join("");

        text.push_str("\n\n");
        text
    }
}

impl BlockNode {
    #[allow(dead_code)]
    pub(crate) fn to_markdown(&self) -> String {
        match self {
            BlockNode::Root { children, .. } => children
                .iter()
                .map(|child| child.to_markdown())
                .collect::<Vec<_>>()
                .join("\n\n"),
            BlockNode::Paragraph(paragraph) => paragraph.to_markdown(),
            BlockNode::Heading {
                level, children, ..
            } => {
                let hashes = "#".repeat(*level as usize);
                format!("{} {}", hashes, children.to_markdown())
            }
            BlockNode::Blockquote { children, .. } => {
                let content = children
                    .iter()
                    .map(|child| child.to_markdown())
                    .collect::<Vec<_>>()
                    .join("\n\n");

                content
                    .lines()
                    .map(|line| format!("> {}", line))
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            BlockNode::List {
                children,
                ordered,
                start,
                ..
            } => children
                .iter()
                .enumerate()
                .map(|(i, child)| {
                    let prefix = if *ordered {
                        format!("{}. ", ordered_list_ordinal(*start, i))
                    } else {
                        "- ".to_string()
                    };
                    format!("{}{}", prefix, child.to_markdown())
                })
                .collect::<Vec<_>>()
                .join("\n"),
            BlockNode::ListItem {
                children, checked, ..
            } => {
                let checkbox = if let Some(checked) = checked {
                    if *checked { "[x] " } else { "[ ] " }
                } else {
                    ""
                };
                format!(
                    "{}{}",
                    checkbox,
                    children
                        .iter()
                        .map(|child| child.to_markdown())
                        .collect::<Vec<_>>()
                        .join("\n")
                )
            }
            BlockNode::CodeBlock(code_block) => {
                format!(
                    "```{}\n{}\n```",
                    code_block.lang.clone().unwrap_or_default(),
                    code_block.code()
                )
            }
            BlockNode::Table(table) => table.to_markdown(),
            BlockNode::Break { html, .. } => {
                if *html {
                    "<br>".to_string()
                } else {
                    "\n".to_string()
                }
            }
            BlockNode::HorizontalRule { .. } => "---".to_string(),
            BlockNode::Custom { node, .. } => node.to_markdown(),
            BlockNode::Definition {
                identifier,
                url,
                title,
                ..
            } => {
                if let Some(title) = title {
                    format!("[{}]: {} \"{}\"", identifier, url, title)
                } else {
                    format!("[{}]: {}", identifier, url)
                }
            }
            BlockNode::Unknown { .. } => "".to_string(),
        }
        .trim()
        .to_string()
    }
}

impl BlockNode {
    fn render_list_item_row(
        content: AnyElement,
        ix: usize,
        options: NodeRenderOptions,
        checked: Option<bool>,
        style: &TextViewStyle,
        line_height: Pixels,
        indent: Pixels,
    ) -> Div {
        h_flex()
            .w_full()
            .min_w_0()
            .relative()
            .items_start()
            .content_start()
            .when(!options.todo && checked.is_none(), |this| {
                // The marker hangs in an indent column, right-aligned against
                // the text like an outside list marker, and steps back in
                // color so the item text leads.
                this.child(
                    div()
                        .flex_none()
                        .min_w(indent)
                        .pr(indent * 0.3)
                        .text_right()
                        .text_color(style.muted_foreground())
                        .child(
                            list_item_prefix(
                                ix,
                                options.list_start,
                                options.ordered,
                                options.depth,
                            )
                            .trim_end()
                            .to_string(),
                        ),
                )
            })
            .when_some(checked, |this, checked| {
                // Todo list checkbox
                let check_svg = if style.is_dark() {
                    CHECK_SVG_DARK
                } else {
                    CHECK_SVG_LIGHT
                };
                this.child(
                    div()
                        .flex()
                        .mr_2()
                        .h(line_height)
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .flex()
                                .size(rems(0.9375))
                                .items_center()
                                .justify_center()
                                .rounded(px(4.))
                                .border(px(1.5))
                                .border_color(style.muted_foreground())
                                .when(checked, |this| {
                                    this.bg(style.link()).border_color(style.link()).child(
                                        img(Arc::new(Image::from_bytes(
                                            ImageFormat::Svg,
                                            check_svg.to_vec(),
                                        )))
                                        .size(rems(0.75)),
                                    )
                                }),
                        ),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    // A done task steps back so the open ones stand out.
                    .when(checked == Some(true), |this| {
                        this.text_color(style.muted_foreground())
                    })
                    .child(content),
            )
    }

    fn render_list_item(
        item: &BlockNode,
        ix: usize,
        options: NodeRenderOptions,
        node_cx: &NodeContext,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        match item {
            BlockNode::ListItem {
                children,
                spread,
                checked,
                ..
            } => {
                let body = body_font_size(node_cx, window);
                let indent = body * 1.4;
                div()
                    .w_full()
                    .min_w_0()
                    // Items breathe a little apart, less than paragraphs do.
                    .when(ix > 0, |this| this.pt(body * 0.25))
                    .when(*spread, |this| this.child(div()))
                    .children({
                        let mut items: Vec<Div> = Vec::with_capacity(children.len());

                        for (child_ix, child) in children.iter().enumerate() {
                            match child {
                                BlockNode::Paragraph { .. } => {
                                    let last_not_list = child_ix > 0
                                        && !matches!(
                                            children[child_ix - 1],
                                            BlockNode::List { .. }
                                        );

                                    let text = child.render_block(
                                        NodeRenderOptions {
                                            depth: options.depth + 1,
                                            todo: checked.is_some(),
                                            is_last: true,
                                            ..options
                                        },
                                        node_cx,
                                        window,
                                        cx,
                                    );

                                    // Continuation paragraph — stack vertically below
                                    // the previous row, indented to align with the text
                                    // column (past bullet/number prefix).
                                    if last_not_list {
                                        if let Some(preceding_row) = items.pop() {
                                            items.push(
                                                div().child(preceding_row).child(
                                                    div()
                                                        .w_full()
                                                        .pl(indent)
                                                        .overflow_hidden()
                                                        .child(text),
                                                ),
                                            );
                                            continue;
                                        }
                                    }

                                    items.push(Self::render_list_item_row(
                                        text,
                                        ix,
                                        options,
                                        *checked,
                                        &node_cx.style,
                                        window.line_height(),
                                        indent,
                                    ));
                                }
                                BlockNode::List { .. } => {
                                    items.push(div().ml(indent).child(child.render_block(
                                        NodeRenderOptions {
                                            depth: options.depth + 1,
                                            todo: checked.is_some(),
                                            is_last: true,
                                            ..options
                                        },
                                        node_cx,
                                        window,
                                        cx,
                                    )));
                                }
                                BlockNode::Root { .. }
                                | BlockNode::Heading { .. }
                                | BlockNode::Blockquote { .. }
                                | BlockNode::CodeBlock(_)
                                | BlockNode::Custom { .. }
                                | BlockNode::Table(_)
                                | BlockNode::HorizontalRule { .. } => {
                                    let block = child.render_block(
                                        NodeRenderOptions {
                                            depth: options.depth + 1,
                                            todo: checked.is_some(),
                                            is_last: true,
                                            ..options
                                        },
                                        node_cx,
                                        window,
                                        cx,
                                    );

                                    if child_ix == 0 {
                                        items.push(Self::render_list_item_row(
                                            block,
                                            ix,
                                            options,
                                            *checked,
                                            &node_cx.style,
                                            window.line_height(),
                                            indent,
                                        ));
                                    } else {
                                        // Indent continuation blocks to align with a
                                        // nested sub-list (`ml(indent)`) and with
                                        // continuation paragraphs.
                                        items.push(
                                            div()
                                                .w_full()
                                                .min_w_0()
                                                .pl(indent)
                                                .overflow_hidden()
                                                .child(block),
                                        );
                                    }
                                }
                                BlockNode::ListItem { .. }
                                | BlockNode::Break { .. }
                                | BlockNode::Definition { .. }
                                | BlockNode::Unknown => {}
                            }
                        }
                        items
                    })
                    .into_any_element()
            }
            _ => div().into_any_element(),
        }
    }

    fn render_table(
        item: &BlockNode,
        options: &NodeRenderOptions,
        node_cx: &NodeContext,
        text_size: Pixels,
        window: &mut Window,
        cx: &mut App,
    ) -> impl IntoElement {
        // A scrolling column stops shrinking (and the table starts to scroll)
        // at a floor scaled to its content: roughly the width at which the
        // text wraps to `CELL_WRAP_MAX_LINES` lines, clamped between the two
        // bounds so moderate columns can still wrap meaningfully while one
        // huge column cannot push the scroll threshold arbitrarily high.
        const CELL_WRAP_MAX_LINES: f32 = 2.0;
        const CELL_WRAP_MIN_PX: f32 = 160.0;
        const CELL_WRAP_MAX_PX: f32 = 480.0;
        const TABLE_BORDER_PX: f32 = 2.0; // the frame's border_1, left + right

        let table = match item {
            BlockNode::Table(table) => table,
            _ => return div().into_any_element(),
        };

        let style = &node_cx.style;
        let col_count = table
            .children
            .iter()
            .map(|row| row.children.len())
            .max()
            .unwrap_or(0);
        let col_w = measure_table_columns(table, col_count, node_cx, window, cx);
        let scroll = matches!(style.table().overflow.x, Some(Overflow::Scroll));
        let nowrap = style.table_cell().text.white_space == Some(WhiteSpace::Nowrap);
        let rem_size = window.rem_size();
        let wrap_min_px = f32::from(rems(4.).to_pixels(rem_size));
        // Widths were measured with the default cell padding.
        let padding_delta = table_cell_padding_x(style, rem_size) - CELL_PAD_PX;
        // Lengths are rounded to device pixels for layout. Round the content
        // widths up, or a column can lose the fraction of a pixel that keeps
        // its text on one line.
        let scale_factor = window.scale_factor();
        let device_ceil = |width: f32| (width * scale_factor).ceil() / scale_factor;
        let columns: Vec<TableColumn> = col_w
            .iter()
            .enumerate()
            .map(|(ix, &w)| {
                let w = w + padding_delta;
                let floor = if scroll && nowrap {
                    w
                } else if scroll {
                    (w / CELL_WRAP_MAX_LINES)
                        .clamp(CELL_WRAP_MIN_PX, CELL_WRAP_MAX_PX)
                        .min(w)
                } else {
                    w.min(wrap_min_px)
                };
                let divider = if ix + 1 < col_count {
                    CELL_BORDER_PX
                } else {
                    0.
                };
                let max = device_ceil(w - divider);
                TableColumn {
                    min: device_ceil(floor - divider).min(max),
                    max,
                    divider,
                }
            })
            .collect();

        let rows = Self::render_table_rows(table, &columns, node_cx, window, cx);
        let frame = StyleRefinement::default()
            .text_size(text_size)
            .bg(style
                .table_background()
                .unwrap_or(cx.theme().tokens.colors.surface))
            .border_1()
            .border_color(style.border())
            .refine_style(style.table());
        let body = if scroll {
            let scroll_handle = window
                .use_keyed_state(
                    block_element_id("table-scroll", table.span, options.ix),
                    cx,
                    |_, _| ScrollHandle::default(),
                )
                .read(cx)
                .clone();
            let min_total_w = columns
                .iter()
                .map(|column| column.min + column.divider)
                .sum::<f32>()
                + TABLE_BORDER_PX;
            // Scroll viewport owns the visible frame, including any
            // caller-provided radius. Keeping the border here makes the
            // rounded frame stable while the wider row track moves below it.
            //
            // `horizontal_scroll_area` clips with `overflow_hidden` and
            // delegates the wheel to a sibling `ScrollableMask`, so the
            // gesture is locked to its starting axis and a horizontal swipe
            // is consumed before an ancestor scroller (`gpui::list` under
            // `TextView::scrollable`) can take its vertical component.
            horizontal_scroll_area(
                block_element_id("table", table.span, options.ix),
                &scroll_handle,
                &frame,
                // Row track sized to `max(viewport, column floors)`:
                // `min_w_full` fills the frame while the columns can still
                // shrink-to-fit (their text wrapping), the definite
                // `w(min_total_w)` keeps the floors once they are reached,
                // letting the track exceed the viewport and scroll.
                div().min_w_full().w(px(min_total_w)).children(rows),
            )
            .into_any_element()
        } else {
            div()
                .w_full()
                .overflow_hidden()
                .refine_style(&frame)
                .children(rows)
                .into_any_element()
        };

        div()
            .pb(block_gap(options, style))
            .w_full()
            .child(body)
            // Custom actions row (e.g. copy / download) rendered below the
            // table. The hook's element spans full width; alignment is up to
            // the caller (e.g. `h_flex().justify_end()`). The gap keeps hover
            // backgrounds of the action buttons off the table border, and the
            // id scopes the caller's element ids per table, so plain ids like
            // `"copy"` don't collide across tables (same as code blocks).
            .children(node_cx.table_actions.clone().map(|f| {
                div()
                    .id(block_element_id("table-actions", table.span, options.ix))
                    .mt_1()
                    .child(f(&table.cached_table_data(), window, cx))
            }))
            .into_any_element()
    }

    fn table_row_corner_radii(style: &TextViewStyle, window: &Window) -> [Option<Pixels>; 4] {
        let rem_size = window.rem_size();
        let inset = |radius: Option<gpui::AbsoluteLength>| {
            radius.map(|radius| (radius.to_pixels(rem_size) - px(1.)).max(px(0.)))
        };
        let radii = &style.table().corner_radii;
        [
            inset(radii.top_left),
            inset(radii.top_right),
            inset(radii.bottom_left),
            inset(radii.bottom_right),
        ]
    }

    fn render_table_rows(
        table: &Table,
        columns: &[TableColumn],
        node_cx: &NodeContext,
        window: &mut Window,
        cx: &mut App,
    ) -> Vec<AnyElement> {
        // The slack's share of the frame while any cell is still growing is
        // `SLACK_GROW / (cells * CELL_GROW)`, a negligible fraction of a
        // pixel. The slack factors still add up to `SLACK_GROW`: flex layout
        // hands out only that fraction of the free space when the factors
        // sum to less than one, and the slack must take all of it.
        const CELL_GROW: f32 = 1e6;
        const SLACK_GROW: f32 = 1.0;

        let style = &node_cx.style;
        let col_count = columns.len();
        let total_w = columns.iter().map(|column| column.max).sum::<f32>().max(1.);
        // The slack is part of the column, so it carries the cell's fill and
        // the column divider in the cell's border color.
        let mut slack_style = StyleRefinement::default();
        slack_style.background = style.table_cell().background.clone();
        slack_style.border_color = style.table_cell().border_color;
        let slack = |grow: f32| {
            div()
                .flex_basis(px(0.))
                .flex_grow(grow)
                .flex_shrink(0.)
                .border_color(style.border())
                .refine_style(&slack_style)
        };

        let row_count = table.children.len();
        let [top_left, top_right, bottom_left, bottom_right] =
            Self::table_row_corner_radii(style, window);
        let mut rows = Vec::with_capacity(row_count);
        let mut cell_ordinal = 0;
        for (row_ix, row) in table.children.iter().enumerate() {
            let mut cells = Vec::with_capacity(col_count * 2);
            for (ix, column) in columns.iter().enumerate() {
                let is_last_col = ix + 1 == col_count;
                let align = table.column_align(ix);
                let content = row.children.get(ix).map(|cell| {
                    let fade_key = table
                        .span
                        .map(|span| TextLeafKey::table_cell(span.start, cell_ordinal));
                    cell_ordinal += 1;
                    cell.children.render(fade_key, node_cx, window, cx)
                });

                let grow = SLACK_GROW * column.max / total_w;
                let (lead, trail) = match align {
                    ColumnumnAlign::Left => (0., grow),
                    ColumnumnAlign::Center => (grow / 2., grow / 2.),
                    ColumnumnAlign::Right => (grow, 0.),
                };
                if lead > 0. {
                    cells.push(slack(lead).into_any_element());
                }
                cells.push(
                    div()
                        .flex_basis(px(0.))
                        .flex_grow(CELL_GROW)
                        .min_w(px(column.min))
                        .max_w(px(column.max))
                        .overflow_hidden()
                        .when(align == ColumnumnAlign::Center, |this| this.text_center())
                        .when(align == ColumnumnAlign::Right, |this| this.text_right())
                        .px_3()
                        .py(px(7.))
                        .refine_style(&style.table_cell())
                        .children(content)
                        .into_any_element(),
                );
                if trail > 0. || !is_last_col {
                    cells.push(
                        slack(trail)
                            .when(!is_last_col, |this| this.border_r_1())
                            .into_any_element(),
                    );
                }
            }
            rows.push(
                div()
                    .w_full()
                    .when(row_ix < row_count - 1, |this| this.border_b_1())
                    .border_color(style.border())
                    .flex()
                    .flex_row()
                    // The first row is the header, as everywhere else that
                    // reads a table (`table_data`, `to_markdown`). The
                    // refinement comes last so it can override the defaults.
                    .when(row_ix == 0, |this| {
                        this.bg(style.code_background())
                            .text_color(style.foreground())
                            .when_some(top_left, |this, radius| this.rounded_tl(radius))
                            .when_some(top_right, |this, radius| this.rounded_tr(radius))
                            .refine_style(&style.table_head())
                    })
                    .when(row_ix + 1 == row_count, |this| {
                        this.when_some(bottom_left, |this, radius| this.rounded_bl(radius))
                            .when_some(bottom_right, |this, radius| this.rounded_br(radius))
                    })
                    .children(cells)
                    .into_any_element(),
            );
        }
        rows
    }

    pub(crate) fn render_block(
        &self,
        options: NodeRenderOptions,
        node_cx: &NodeContext,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let mb = block_gap(&options, &node_cx.style);

        match self {
            BlockNode::Root { children, .. } => div()
                .children(children.into_iter().enumerate().map(move |(ix, node)| {
                    // A nested root (an HTML block container) continues the
                    // flow around it: until it shows a block of its own, it
                    // stands where the container stands.
                    let (is_first, prev) = match flow_position(children, ix) {
                        (true, _) => (options.is_first, options.prev),
                        (false, prev) => (false, prev),
                    };
                    node.render_block(
                        NodeRenderOptions {
                            ix,
                            is_first,
                            prev,
                            is_last: options.is_last && ix + 1 == children.len(),
                            ..options
                        },
                        node_cx,
                        window,
                        cx,
                    )
                }))
                .into_any_element(),
            BlockNode::Paragraph(paragraph) => gapped(
                paragraph.render(
                    paragraph.span.map(|span| TextLeafKey::block(span.start)),
                    node_cx,
                    window,
                    cx,
                ),
                mb,
            ),
            BlockNode::Heading {
                level,
                children,
                span,
            } => {
                let (_, line_height, section_gap) = heading_metrics(*level);
                let refinement = node_cx.style.heading(*level);
                let text_size = heading_size(*level, node_cx, window);
                // The previous block already leaves a gap below itself, so
                // only the rest of the section gap is added here -- the two
                // collapse like CSS margins. A heading right after another one
                // belongs to the same section and stays close. Nested in a
                // list item, a heading sits flush with the item.
                let top = if options.is_first || options.depth > 0 {
                    px(0.)
                } else {
                    match options.prev {
                        PrevBlock::Heading(_) => text_size * 0.25,
                        PrevBlock::Rule => (text_size * section_gap
                            - rule_space(body_font_size(node_cx, window)))
                        .max(px(0.)),
                        PrevBlock::Other => (text_size * section_gap
                            - node_cx.style.paragraph_gap().to_pixels(window.rem_size()))
                        .max(px(0.)),
                    }
                };
                let bottom = if options.is_last {
                    px(0.)
                } else {
                    text_size * HEADING_BOTTOM_GAP
                };

                div()
                    .pt(top)
                    .pb(bottom)
                    .whitespace_normal()
                    .text_size(text_size)
                    .line_height(relative(line_height))
                    .font_weight(FontWeight::SEMIBOLD)
                    .when(*level >= 6, |this| {
                        this.text_color(node_cx.style.muted_foreground())
                    })
                    .refine_style(&refinement)
                    .child(children.render(
                        span.map(|span| TextLeafKey::block(span.start)),
                        node_cx,
                        window,
                        cx,
                    ))
                    .into_any_element()
            }
            BlockNode::Blockquote { children, .. } => gapped(
                div()
                    .w_full()
                    .text_color(node_cx.style.muted_foreground())
                    .border_l_2()
                    .border_color(node_cx.style.border())
                    .pl(body_font_size(node_cx, window) * 0.9)
                    .children({
                        let children_len = children.len();
                        children.into_iter().enumerate().map(move |(index, c)| {
                            let is_last = index == children_len - 1;
                            let (is_first, prev) = flow_position(children, index);
                            c.render_block(
                                NodeRenderOptions {
                                    is_first,
                                    prev,
                                    ..options.is_last(is_last)
                                },
                                node_cx,
                                window,
                                cx,
                            )
                        })
                    })
                    .into_any_element(),
                mb,
            ),
            BlockNode::List {
                children,
                ordered,
                start,
                ..
            } => div()
                .w_full()
                .min_w_0()
                .pb(mb)
                .children({
                    let mut items = Vec::with_capacity(children.len());
                    let mut item_index = 0;
                    for (ix, item) in children.into_iter().enumerate() {
                        let is_item = item.is_list_item();

                        items.push(Self::render_list_item(
                            item,
                            item_index,
                            NodeRenderOptions {
                                ix,
                                ordered: *ordered,
                                list_start: *start,
                                ..options
                            },
                            node_cx,
                            window,
                            cx,
                        ));

                        if is_item {
                            item_index += 1;
                        }
                    }
                    items
                })
                .into_any_element(),
            BlockNode::CodeBlock(code_block) => code_block.render(&options, node_cx, window, cx),
            BlockNode::Custom { node, state } => {
                let inner = match node_cx.markdown_extensions.render_block(node, window, cx) {
                    Some(rendered) => rendered,
                    None => div().child(node.as_text().to_string()).into_any_element(),
                };

                div()
                    .pb(mb)
                    .child(CustomBlockElement {
                        content: inner,
                        state: state.clone(),
                    })
                    .into_any_element()
            }
            BlockNode::Table { .. } => {
                // Tables are data and read a step denser than the body. The
                // columns are measured while rendering, so they are measured
                // at the same size the cells are laid out with; the actions
                // row below keeps the body size.
                let text_size = body_font_size(node_cx, window) * (14. / 15.);
                window.with_text_style(
                    Some(TextStyleRefinement {
                        font_size: Some(text_size.into()),
                        ..Default::default()
                    }),
                    |window| {
                        Self::render_table(self, &options, node_cx, text_size, window, cx)
                            .into_any_element()
                    },
                )
            }
            BlockNode::HorizontalRule { .. } => {
                // A section break: a hairline with more room around it than
                // a paragraph gap. The previous block's own gap counts toward
                // the space above, as with headings.
                let space = rule_space(body_font_size(node_cx, window));
                let top = if options.is_first || options.depth > 0 {
                    px(0.)
                } else {
                    let above = match options.prev {
                        PrevBlock::Rule => space,
                        PrevBlock::Heading(level) => {
                            heading_size(level, node_cx, window) * HEADING_BOTTOM_GAP
                        }
                        PrevBlock::Other => {
                            node_cx.style.paragraph_gap().to_pixels(window.rem_size())
                        }
                    };
                    (space - above).max(px(0.))
                };
                div()
                    .pt(top)
                    .when(!options.is_last, |this| this.pb(space))
                    .child(div().bg(node_cx.style.border()).h(px(1.)))
                    .into_any_element()
            }
            BlockNode::Break { .. } => div().into_any_element(),
            BlockNode::Unknown { .. } | BlockNode::Definition { .. } => div().into_any_element(),
            _ => {
                if cfg!(debug_assertions) {
                    tracing::warn!("unknown implementation: {:?}", self);
                }

                div().into_any_element()
            }
        }
    }
}
