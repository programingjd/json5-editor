use std::{ops::Range, rc::Rc, sync::Arc};

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, Bounds, ClickEvent, ContentMask, Element, ElementId, Entity, Global,
    GlobalElementId, Hitbox, HitboxBehavior, InspectorElementId, InteractiveElement, IntoElement,
    LayoutId, MouseButton, ParentElement, Pixels, Refineable as _, SharedString, StyleRefinement,
    Styled, Window, div, point, px,
};

use crate::StyledExt;
use crate::text::TextViewFormat;
use crate::text::markdown_ext::{MarkdownExtensions, MarkdownNode, MarkdownPlugin};
use crate::text::node::{CodeBlock, TableData};
use crate::text::range_highlight::{PendingReveal, RevealProgress};
use crate::text::state::{LineSpan, SelectionFormat, TextViewState};
use crate::text::stream_fade::TextViewMotion;
use crate::{GlobalState, TextSelection, text::TextViewStyle};

pub(crate) type CodeBlockActionsFn =
    dyn Fn(&CodeBlock, &mut Window, &mut App) -> AnyElement + Send + Sync;

pub(crate) type CodeBlockHighlighterFn =
    dyn Fn(&CodeBlock) -> Vec<(Range<usize>, gpui::HighlightStyle)> + Send + Sync;

#[derive(Clone, Default)]
pub struct TextViewDefaults {
    style: Option<TextViewStyle>,
    code_block_highlighter: Option<Arc<CodeBlockHighlighterFn>>,
    inherit_text_color: bool,
}

impl Global for TextViewDefaults {}

impl TextViewDefaults {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_style(mut self, style: TextViewStyle) -> Self {
        self.style = Some(style);
        self
    }

    pub fn with_code_block_highlighter<F>(mut self, highlighter: F) -> Self
    where
        F: Fn(&CodeBlock) -> Vec<(Range<usize>, gpui::HighlightStyle)> + Send + Sync + 'static,
    {
        self.code_block_highlighter = Some(Arc::new(highlighter));
        self
    }

    pub fn with_inherit_text_color(mut self, inherit: bool) -> Self {
        self.inherit_text_color = inherit;
        self
    }

    pub fn install(self, cx: &mut App) {
        cx.set_global(self);
    }

    pub fn global(cx: &App) -> Self {
        cx.try_global::<Self>().cloned().unwrap_or_default()
    }

    pub fn has_code_block_highlighter(&self) -> bool {
        self.code_block_highlighter.is_some()
    }

    pub fn inherit_text_color(&self) -> bool {
        self.inherit_text_color
    }
}

pub(crate) type TableActionsFn =
    dyn Fn(&TableData, &mut Window, &mut App) -> AnyElement + Send + Sync;

pub(crate) type ImageSourceFn = dyn Fn(&gpui::SharedUri) -> gpui::ImageSource + Send + Sync;

pub(crate) type LinkClickHandlerFn =
    dyn Fn(&SharedString, &ClickEvent, &mut Window, &mut App) + Send + Sync;

pub(crate) type RevealHandlerFn = dyn Fn(Bounds<Pixels>, &mut Window, &mut App);

pub(crate) fn handle_link_click(
    handler: &Option<Arc<LinkClickHandlerFn>>,
    url: SharedString,
    event: ClickEvent,
    window: &mut Window,
    cx: &mut App,
) {
    if let Some(handler) = handler {
        handler(&url, &event, window, cx);
    } else if match &event {
        ClickEvent::Mouse(click) => {
            matches!(click.up.button, MouseButton::Left | MouseButton::Middle)
        }
        ClickEvent::Keyboard(_) => true,
        ClickEvent::Touch(click) => !click.long_press,
    } {
        cx.open_url(&url);
    }
}

#[derive(Clone)]
pub struct TextView {
    id: ElementId,
    format: Option<TextViewFormat>,
    text: Option<SharedString>,
    pub(crate) state: Option<Entity<TextViewState>>,
    text_view_style: Option<TextViewStyle>,
    style: StyleRefinement,
    selectable: bool,
    selection_format: SelectionFormat,
    scrollable: bool,
    max_lines: Option<usize>,
    code_block_actions: Option<Arc<CodeBlockActionsFn>>,
    code_block_highlighter: Option<Arc<CodeBlockHighlighterFn>>,
    table_actions: Option<Arc<TableActionsFn>>,
    link_click_handler: Option<Arc<LinkClickHandlerFn>>,
    image_source: Option<Arc<ImageSourceFn>>,
    reveal_handler: Option<Rc<RevealHandlerFn>>,
    markdown_extensions: Arc<MarkdownExtensions>,
    motion: Option<TextViewMotion>,
}

pub trait TextViewPlugin {
    fn setup(self, text_view: TextView) -> TextView;
}

impl<P> TextViewPlugin for P
where
    P: MarkdownPlugin,
{
    fn setup(self, mut text_view: TextView) -> TextView {
        let extensions = Arc::make_mut(&mut text_view.markdown_extensions);
        let current = std::mem::take(extensions);
        *extensions = current.plugin(self);
        text_view
    }
}

impl Styled for TextView {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl TextView {
    pub fn new(state: &Entity<TextViewState>) -> Self {
        Self {
            id: ElementId::Name(state.entity_id().to_string().into()),
            state: Some(state.clone()),
            format: None,
            text: None,
            text_view_style: None,
            style: StyleRefinement::default(),
            selectable: true,
            selection_format: SelectionFormat::default(),
            scrollable: false,
            max_lines: None,
            code_block_actions: None,
            code_block_highlighter: None,
            table_actions: None,
            link_click_handler: None,
            image_source: None,
            reveal_handler: None,
            markdown_extensions: Arc::default(),
            motion: None,
        }
    }

    pub fn markdown(id: impl Into<ElementId>, markdown: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            format: Some(TextViewFormat::Markdown),
            text: Some(markdown.into()),
            text_view_style: None,
            style: StyleRefinement::default(),
            state: None,
            selectable: true,
            selection_format: SelectionFormat::default(),
            scrollable: false,
            max_lines: None,
            code_block_actions: None,
            code_block_highlighter: None,
            table_actions: None,
            link_click_handler: None,
            image_source: None,
            reveal_handler: None,
            markdown_extensions: Arc::default(),
            motion: None,
        }
    }

    pub fn html(id: impl Into<ElementId>, html: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            format: Some(TextViewFormat::Html),
            text: Some(html.into()),
            text_view_style: None,
            style: StyleRefinement::default(),
            state: None,
            selectable: true,
            selection_format: SelectionFormat::default(),
            scrollable: false,
            max_lines: None,
            code_block_actions: None,
            code_block_highlighter: None,
            table_actions: None,
            link_click_handler: None,
            image_source: None,
            reveal_handler: None,
            markdown_extensions: Arc::default(),
            motion: None,
        }
    }

    pub fn image_source<F>(mut self, resolver: F) -> Self
    where
        F: Fn(&gpui::SharedUri) -> gpui::ImageSource + Send + Sync + 'static,
    {
        self.image_source = Some(Arc::new(resolver));
        self
    }

    pub fn style(mut self, style: TextViewStyle) -> Self {
        self.text_view_style = Some(style);
        self
    }

    pub fn selectable(mut self, selectable: bool) -> Self {
        self.selectable = selectable;
        self
    }

    pub fn selection_format(mut self, selection_format: SelectionFormat) -> Self {
        self.selection_format = selection_format;
        self
    }

    pub fn scrollable(mut self, scrollable: bool) -> Self {
        self.scrollable = scrollable;
        self
    }

    pub fn max_lines(mut self, max_lines: usize) -> Self {
        self.max_lines = Some(max_lines);
        self
    }

    pub fn code_block_actions<F, E>(mut self, f: F) -> Self
    where
        F: Fn(&CodeBlock, &mut Window, &mut App) -> E + Send + Sync + 'static,
        E: IntoElement,
    {
        self.code_block_actions = Some(Arc::new(move |code_block, window, cx| {
            f(&code_block, window, cx).into_any_element()
        }));
        self
    }

    pub fn code_block_highlighter<F>(mut self, highlighter: F) -> Self
    where
        F: Fn(&CodeBlock) -> Vec<(Range<usize>, gpui::HighlightStyle)> + Send + Sync + 'static,
    {
        self.code_block_highlighter = Some(Arc::new(highlighter));
        self
    }

    #[doc(hidden)]
    pub fn shared_code_block_highlighter(
        mut self,
        highlighter: Arc<
            dyn Fn(&CodeBlock) -> Vec<(Range<usize>, gpui::HighlightStyle)> + Send + Sync,
        >,
    ) -> Self {
        self.code_block_highlighter = Some(highlighter);
        self
    }

    pub fn table_actions<F, E>(mut self, f: F) -> Self
    where
        F: Fn(&TableData, &mut Window, &mut App) -> E + Send + Sync + 'static,
        E: IntoElement,
    {
        self.table_actions = Some(Arc::new(move |table, window, cx| {
            f(table, window, cx).into_any_element()
        }));
        self
    }

    pub fn on_link_click<F>(mut self, handler: F) -> Self
    where
        F: Fn(&SharedString, &ClickEvent, &mut Window, &mut App) + Send + Sync + 'static,
    {
        self.link_click_handler = Some(Arc::new(handler));
        self
    }

    pub fn on_reveal<F>(mut self, handler: F) -> Self
    where
        F: Fn(Bounds<Pixels>, &mut Window, &mut App) + 'static,
    {
        self.reveal_handler = Some(Rc::new(handler));
        self
    }

    pub fn markdown_extensions(mut self, extensions: MarkdownExtensions) -> Self {
        self.markdown_extensions = Arc::new(extensions);
        self
    }

    pub fn motion(mut self, motion: TextViewMotion) -> Self {
        self.motion = Some(motion);
        self
    }

    pub fn markdown_mdx(mut self) -> Self {
        let extensions = Arc::make_mut(&mut self.markdown_extensions);
        *extensions = extensions.clone().mdx();
        self
    }

    pub fn markdown_block_parser<F>(mut self, parser: F) -> Self
    where
        F: for<'a> Fn(
                &markdown::mdast::Node,
                &crate::text::MarkdownParseContext<'a>,
            ) -> Option<MarkdownNode>
            + Send
            + Sync
            + 'static,
    {
        Arc::make_mut(&mut self.markdown_extensions).push_block_parser(parser);
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
        Arc::make_mut(&mut self.markdown_extensions).push_block_renderer(name, renderer);
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
    state: Entity<TextViewState>,
    element: AnyElement,
}

pub struct TextViewPrepaintState {
    hitbox: Hitbox,

    clip_bottom: Option<Pixels>,

    touch_handles: crate::TouchHandleLayout,
}

const CLIP_EPSILON: Pixels = px(1.);

fn last_line_bottom_above(spans: &[LineSpan], y: Pixels) -> Option<(Pixels, Pixels)> {
    let mut last: Option<(Pixels, Pixels)> = None;
    let mut keep = |bottom: Pixels, line_height: Pixels| {
        if bottom <= y + CLIP_EPSILON && last.is_none_or(|(last, _)| bottom > last) {
            last = Some((bottom, line_height));
        }
    };

    for span in spans {
        if span.line_height <= px(0.) {
            continue;
        }
        let mut bottom = span.top + span.line_height;
        while bottom <= span.bottom + CLIP_EPSILON {
            keep(bottom, span.line_height);
            bottom += span.line_height;
        }
        // The span's own bottom covers a last line taller than the rest.
        keep(span.bottom, span.line_height);
    }

    last
}

fn line_safe_clip_bottom(
    spans: &[LineSpan],
    box_bottom: Pixels,
    content_bottom: Pixels,
) -> Option<Pixels> {
    let mut clip = box_bottom;

    for span in spans {
        if span.line_height <= px(0.)
            || span.top >= box_bottom
            || span.bottom <= box_bottom + CLIP_EPSILON
        {
            continue;
        }
        let whole_lines = ((box_bottom - span.top) / span.line_height).floor();
        let line_top = span.top + span.line_height * whole_lines;
        // A line starting on the box edge is not straddling it.
        if line_top < box_bottom - CLIP_EPSILON {
            clip = clip.min(line_top);
        }
    }

    let Some((last_line_bottom, line_height)) = last_line_bottom_above(spans, clip) else {
        // Leaving the straddling line out would leave nothing at all — a first
        // line taller than the whole budget, a heading in a one-line box. It
        // keeps the part that fits instead, because an empty clamp reads as
        // broken where a cut one reads as more to come.
        return None;
    };

    // Snap away a scrap. Only content that continues past the box can leave
    // one: the space under the last line of a document that fits is the box's
    // own, not a piece of something below.
    if content_bottom > box_bottom + CLIP_EPSILON {
        let strip = clip - last_line_bottom;
        if strip > CLIP_EPSILON && strip < line_height {
            clip = last_line_bottom;
        }
    }

    (clip < box_bottom - CLIP_EPSILON).then_some(clip)
}

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
        let state = if let Some(state) = self.state.clone() {
            state
        } else {
            let default_format = self.format.unwrap_or(TextViewFormat::Markdown);
            let default_text = self.text.clone().unwrap_or_default();

            let state = window.use_keyed_state(
                SharedString::from(format!("{}/state", self.id)),
                cx,
                move |_, cx| {
                    if default_format == TextViewFormat::Markdown {
                        TextViewState::markdown(default_text.as_str(), cx)
                    } else {
                        TextViewState::html(default_text.as_str(), cx)
                    }
                },
            );
            self.state = Some(state.clone());
            state
        };

        // `max_lines` needs the whole document laid out to snap the clip to a
        // whole line, so it only applies to the fit-content mode.
        let max_lines = self.max_lines.filter(|_| !self.scrollable);

        // Resolve the style by reference: this runs every frame, and the
        // style only reaches the state when it changed.
        let defaults = cx.try_global::<TextViewDefaults>();
        let theme_style;
        let mut text_view_style = match (
            &self.text_view_style,
            defaults.and_then(|d| d.style.as_ref()),
        ) {
            (Some(style), _) | (None, Some(style)) => style,
            (None, None) => {
                theme_style = TextViewStyle::from_theme(&crate::Theme::global(cx));
                &theme_style
            }
        };
        // Follow the text color the container sets for its surface, so rich
        // text stays readable in a filled bubble or a selected row.
        let surface_style;
        let mut on_inverted_surface = false;
        if self.text_view_style.is_none() && defaults.is_some_and(|d| d.inherit_text_color) {
            let color = self
                .style
                .text
                .color
                .unwrap_or_else(|| window.text_style().color);
            if color != text_view_style.foreground() {
                on_inverted_surface = text_view_style.is_inverted_by(color);
                surface_style = text_view_style.on_text_color(color);
                text_view_style = &surface_style;
            }
        }
        let foreground = text_view_style.foreground();
        let text_view_style = (*state.read(cx).text_view_style != *text_view_style)
            .then(|| Arc::new(text_view_style.clone()));
        let code_block_highlighter = self.code_block_highlighter.clone().or_else(|| {
            defaults
                .filter(|_| !on_inverted_surface)
                .and_then(|d| d.code_block_highlighter.clone())
        });

        state.update(cx, |state, cx| {
            if text_view_style.is_some()
                || match (&state.code_block_highlighter, &code_block_highlighter) {
                    (Some(previous), Some(next)) => !Arc::ptr_eq(previous, next),
                    (None, None) => false,
                    _ => true,
                }
            {
                state.preserve_selection_for_reflow();
            }
            state.code_block_actions = self.code_block_actions.clone();
            state.code_block_highlighter = code_block_highlighter;
            state.table_actions = self.table_actions.clone();
            state.link_click_handler = self.link_click_handler.clone();
            state.image_source = self.image_source.clone();
            state.set_markdown_extensions(self.markdown_extensions.clone(), cx);
            if let Some(motion) = &self.motion {
                state.set_motion(motion.clone());
            }
            state.selectable = self.selectable;
            state.selection_format = self.selection_format;
            state.scrollable = self.scrollable;
            state.max_lines = max_lines;
            if let Some(text_view_style) = text_view_style {
                state.text_view_style = text_view_style;
            }

            if let Some(text) = &self.text {
                state.set_element_text(text, cx);
            }
        });

        let focus_handle = state.read(cx).focus_handle.clone();
        let list_state = state.read(cx).list_state.clone();
        // Cap the box at `n` body-text lines (the effective text style may be
        // refined by this view's own style, e.g. `.text_sm()`); hidden
        // overflow also clips descendant hitboxes to the box during prepaint.
        let max_lines_cap = max_lines.map(|max_lines| {
            let mut text_style = window.text_style();
            text_style.refine(&self.style.text);
            text_style.line_height_in_pixels(window.rem_size()) * max_lines as f32
        });

        let mut el = div()
            .id(("text-view-scroll", state.entity_id()))
            .key_context("TextView")
            .track_focus(&focus_handle)
            .when(self.scrollable, |this| this.size_full())
            .when_some(max_lines_cap, |this, cap| this.max_h(cap).overflow_hidden())
            .relative()
            .text_color(foreground)
            .on_action(move |_: &crate::input::Copy, window, cx| {
                let text = TextSelection::selected_text(window, cx).trim().to_string();
                if text.is_empty() {
                    cx.propagate();
                    return;
                }
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
            })
            .on_action(window.listener_for(&state, TextViewState::on_action_select_all))
            .child(state.clone())
            // Overlay controls must paint after the document, otherwise rich
            // content and selection backgrounds cover the thumb and hitbox.
            .when(self.scrollable, |this| {
                this.child(
                    div().absolute().inset_0().child(
                        crate::Scrollbar::vertical(&list_state)
                            .id(("text-view-scrollbar", state.entity_id()))
                            .viewport_from_layout(),
                    ),
                )
            })
            .refine_style(&self.style)
            .into_any_element();
        let layout_id = el.request_layout(window, cx);
        (layout_id, TextViewLayoutState { state, element: el })
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let state = request_layout.state.clone();
        let max_lines_active = state.read(cx).max_lines.is_some();
        if max_lines_active {
            if let Ok(mut line_spans) = state.read(cx).line_spans.lock() {
                line_spans.clear();
            }
            // Descendant `Inline`s report their line spans through the state
            // stack during prepaint (in addition to the paint-time push below).
            GlobalState::global(cx).push_text_view_state(state.clone());
        }
        request_layout.element.prepaint(window, cx);
        if max_lines_active {
            GlobalState::global(cx).pop_text_view_state();
        }

        let mut clip_bottom = None;
        if max_lines_active {
            let (line_spans, content_bottom) = {
                let state = state.read(cx);
                (
                    state
                        .line_spans
                        .lock()
                        .map(|spans| spans.clone())
                        .unwrap_or_default(),
                    state.bounds().bottom(),
                )
            };
            // The content keeps its natural height inside the capped box, so
            // this sees everything the box cannot show — including a tall image
            // that reports no lines of its own.
            let clipped = content_bottom > bounds.bottom() + px(1.);
            // Notify on change so observers (e.g. an "expand" button gated on
            // `is_clamped`) re-render once the flag flips.
            if state.read(cx).clamped != clipped {
                state.update(cx, |state, cx| {
                    state.clamped = clipped;
                    cx.notify();
                });
            }
            if clipped {
                clip_bottom = line_safe_clip_bottom(&line_spans, bounds.bottom(), content_bottom);
            }
        }

        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        // Over the text, so after its hitbox.
        let touch_handles = if self.selectable {
            state
                .read(cx)
                .selection_adapter
                .prepaint_touch_handles(window, cx)
        } else {
            crate::TouchHandleLayout::default()
        };
        TextViewPrepaintState {
            hitbox,
            clip_bottom,
            touch_handles,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let state = &request_layout.state;
        if self.selectable {
            state.read(cx).selection_adapter.begin_frame();
        }

        GlobalState::global(cx).push_text_view_state(state.clone());
        if let Some(clip_bottom) = prepaint.clip_bottom {
            // Snap the `max_lines` clip to the last whole line that fits, so a
            // line of glyphs is never cut in half.
            let mask = ContentMask {
                bounds: Bounds::from_corners(bounds.origin, point(bounds.right(), clip_bottom)),
            };
            window.with_content_mask(Some(mask), |window| {
                request_layout.element.paint(window, cx);
            });
        } else {
            request_layout.element.paint(window, cx);
        }
        GlobalState::global(cx).pop_text_view_state();

        // Every list has scrolled by now, so the line of a reveal is where
        // it ends up this frame.
        if state.read(cx).pending_reveal.is_some() {
            let progress = state.update(cx, |state, _| {
                state.pending_reveal.as_mut().map(PendingReveal::progress)
            });
            match progress {
                Some(RevealProgress::Shown) => {
                    state.update(cx, |state, _| state.pending_reveal = None);
                }
                Some(RevealProgress::Hidden(line)) => {
                    if let Some(handler) = &self.reveal_handler {
                        handler(line, window, cx);
                    }
                }
                Some(RevealProgress::NotLaidOut) | None => {}
            }
        }

        if self.selectable {
            let (adapter, scroll_offset, content_bounds, self_scroll, handle_color) = {
                let state = state.read(cx);
                (
                    state.selection_adapter.clone(),
                    state.scroll_offset(),
                    state.bounds(),
                    state.scrollable,
                    state.text_view_style.selection().alpha(1.),
                )
            };
            let document_order = GlobalState::global(cx).next_selection_document_order();
            adapter.register(
                prepaint.hitbox.clone(),
                content_bounds,
                scroll_offset,
                document_order,
                self_scroll,
                window,
                cx,
            );
            // The handles of a touch selection go over the text, and under
            // whatever is painted over the text after it.
            adapter.paint_touch_handles(&prepaint.touch_handles, handle_color, window, cx);
        }
    }
}
