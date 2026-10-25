use futures::Stream as _;
#[cfg(not(target_family = "wasm"))]
use std::time::Instant;
use std::{
    ops::{Range, RangeInclusive},
    pin::Pin,
    sync::{Arc, Mutex, OnceLock},
    task::Poll,
    time::Duration,
};
#[cfg(target_family = "wasm")]
use web_time::Instant;

use gpui::{
    App, AppContext as _, Bounds, Context, EntityId, FocusHandle, IntoElement, KeyBinding,
    ListState, ParentElement as _, Pixels, Point, Render, SharedString, Styled as _, Task, Window,
    prelude::FluentBuilder as _, px,
};

use crate::{
    AutoScroll, ElementExt, TextSelection,
    async_util::{Receiver, Sender, unbounded},
    input::{self, SelectAll},
    text::{
        CodeBlockActionsFn, CodeBlockHighlighterFn, LinkClickHandlerFn, MarkdownExtensions,
        RangeHighlight, RangeHighlightError, RenderedText, TableActionsFn, TextViewStyle,
        document::ParsedDocument,
        format,
        node::{self, NodeContext},
        range_highlight::{
            LeafRemap, PendingReveal, RangeHighlightFrame, RenderedIndex, RevealRequest,
        },
        selection_adapter::TextViewSelectionAdapter,
        stream_fade::{StreamFadeTracker, TextViewMotion},
    },
    v_flex,
};

const CONTEXT: &'static str = "TextView";
// Keep coalescing bounded so sustained streams still render intermediate updates.
const MAX_COALESCED_UPDATES_PER_PARSE: usize = 64;
// Preserve exact first-layout height for small documents while bounding the
// amount of source parsed synchronously on the UI thread.
const MAX_SYNC_FULL_REPLACE_BYTES: usize = 4 * 1024;
// Repaint a streamed fade at about 30 fps, not at the display refresh rate.
const STREAM_FADE_TICK: Duration = Duration::from_millis(33);

pub(crate) fn init(cx: &mut App) {
    cx.bind_keys(vec![
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-c", input::Copy, Some(CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-c", input::Copy, Some(CONTEXT)),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-a", input::SelectAll, Some(CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-a", input::SelectAll, Some(CONTEXT)),
    ]);
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum TextViewFormat {
    Markdown,

    Html,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SelectionFormat {
    #[default]
    Plain,

    Source,
}

#[derive(Clone, Copy)]
pub(super) struct LineSpan {
    pub(super) top: Pixels,
    pub(super) bottom: Pixels,
    pub(super) line_height: Pixels,
}

pub struct TextViewState {
    entity_id: EntityId,
    pub(super) focus_handle: FocusHandle,
    pub(super) list_state: ListState,

    bounds: Bounds<Pixels>,

    pub(super) selectable: bool,
    pub(super) selection_format: SelectionFormat,
    pub(super) scrollable: bool,
    pub(super) max_lines: Option<usize>,

    pub(super) line_spans: Arc<Mutex<Vec<LineSpan>>>,

    pub(super) clamped: bool,
    pub(super) text_view_style: Arc<TextViewStyle>,
    pub(super) code_block_actions: Option<std::sync::Arc<CodeBlockActionsFn>>,
    pub(super) code_block_highlighter: Option<std::sync::Arc<CodeBlockHighlighterFn>>,
    pub(super) table_actions: Option<std::sync::Arc<TableActionsFn>>,
    pub(super) image_source: Option<std::sync::Arc<super::text_view::ImageSourceFn>>,
    pub(super) link_click_handler: Option<std::sync::Arc<LinkClickHandlerFn>>,
    pub(super) markdown_extensions: Arc<MarkdownExtensions>,

    pub(super) is_selecting: bool,

    pub(super) preserve_inline_selection: bool,
    multi_click_selection: Option<TextViewMultiClickSelection>,
    selected_text_override: Option<String>,
    select_all: bool,
    pub(super) auto_scroll: AutoScroll,
    pub(super) selection_adapter: TextViewSelectionAdapter,

    pub(super) parsed_content: ParsedContent,
    pub(super) stream_fade: StreamFadeTracker,
    fade_tick: Option<Task<()>>,

    format: TextViewFormat,
    text: String,

    element_text: Option<SharedString>,
    revision: usize,

    committed_revision: usize,

    full_update_revision: usize,

    rendered_index: Arc<OnceLock<RenderedIndex>>,
    range_highlights: Option<Arc<RangeHighlightFrame>>,
    pub(super) pending_reveal: Option<PendingReveal>,
    pub(super) selection_revision: usize,
    compatible_layout_update: bool,
    layout_text_style: Option<(gpui::TextStyle, Pixels)>,
    parsed_error: Option<SharedString>,
    tx: Sender<UpdateOptions>,
    _parse_task: Task<()>,
    _receive_task: Task<()>,
}

impl TextViewState {
    pub fn markdown(text: &str, cx: &mut Context<Self>) -> Self {
        Self::new(TextViewFormat::Markdown, text, cx)
    }

    pub fn html(text: &str, cx: &mut Context<Self>) -> Self {
        Self::new(TextViewFormat::Html, text, cx)
    }

    fn new(format: TextViewFormat, text: &str, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        let selection_adapter = TextViewSelectionAdapter::new(cx.entity().downgrade(), cx);

        let (tx, rx) = unbounded::<UpdateOptions>();
        let (tx_result, rx_result) = unbounded::<ParsedUpdate>();
        let _receive_task = cx.spawn({
            async move |weak_self, cx| {
                while let Ok(parsed_update) = rx_result.recv().await {
                    // A result that commits nothing — the background parser
                    // acknowledging a synchronous parse — leaves the state
                    // alone: updating it would tell GPUI Fast's retained
                    // views that read it that it changed.
                    let Ok(commits) =
                        weak_self.read_with(cx, |state, _| state.commits(&parsed_update))
                    else {
                        break;
                    };
                    if !commits {
                        continue;
                    }
                    _ = weak_self.update(cx, |state, cx| {
                        state.commit_parsed_update(parsed_update, cx);
                    });
                }
            }
        });

        let _parse_task = cx.background_spawn(UpdateFuture::new(format, rx, tx_result));

        let mut this = Self {
            entity_id: cx.entity_id(),
            focus_handle,
            bounds: Bounds::default(),
            multi_click_selection: None,
            selected_text_override: None,
            select_all: false,
            selectable: false,
            selection_format: SelectionFormat::default(),
            scrollable: false,
            max_lines: None,
            line_spans: Arc::default(),
            clamped: false,
            // Measure all blocks (not just visible ones) so the scrollbar
            // thumb size stays stable. Without this, off-screen blocks count
            // as zero height until scrolled into view, which makes the
            // scrollbar jitter as more blocks get measured during scrolling.
            list_state: ListState::new(0, gpui::ListAlignment::Top, px(1000.)).measure_all(),
            text_view_style: Arc::default(),
            code_block_actions: None,
            code_block_highlighter: None,
            table_actions: None,
            link_click_handler: None,
            image_source: None,
            markdown_extensions: Arc::default(),
            is_selecting: false,
            preserve_inline_selection: false,
            auto_scroll: AutoScroll::default(),
            selection_adapter,
            parsed_content: Default::default(),
            stream_fade: StreamFadeTracker::default(),
            fade_tick: None,
            format,
            parsed_error: None,
            text: text.to_string(),
            element_text: None,
            revision: 0,
            committed_revision: 0,
            full_update_revision: 0,
            rendered_index: Arc::default(),
            range_highlights: None,
            pending_reveal: None,
            selection_revision: 0,
            compatible_layout_update: false,
            layout_text_style: None,
            tx,
            _parse_task,
            _receive_task,
        };
        this.increment_update(&text, false, cx);
        this
    }

    pub(crate) fn source(&self) -> SharedString {
        self.parsed_content.document.source.clone()
    }

    pub fn selectable(mut self, selectable: bool) -> Self {
        self.selectable = selectable;
        self
    }

    pub fn set_selectable(&mut self, selectable: bool, cx: &mut Context<Self>) {
        self.selectable = selectable;
        cx.notify();
    }

    pub fn selection_format(mut self, selection_format: SelectionFormat) -> Self {
        self.selection_format = selection_format;
        self
    }

    pub fn set_selection_format(
        &mut self,
        selection_format: SelectionFormat,
        cx: &mut Context<Self>,
    ) {
        self.selection_format = selection_format;
        cx.notify();
    }

    pub fn scrollable(mut self, scrollable: bool) -> Self {
        self.scrollable = scrollable;
        self
    }

    pub fn set_scrollable(&mut self, scrollable: bool, cx: &mut Context<Self>) {
        if !scrollable {
            self.reset_selection_and_adapter(cx);
        }
        self.scrollable = scrollable;
        cx.notify();
    }

    pub fn is_clamped(&self) -> bool {
        self.clamped
    }

    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        if self.text.as_str() == text {
            return;
        }
        // HTML blocks carry no spans, so an append would parse the delta as a
        // standalone document. After a failed parse the background parser's
        // document lacks that text, so only a full parse resynchronizes it.
        if self.format == TextViewFormat::Markdown
            && self.parsed_error.is_none()
            && !self.text.is_empty()
            && let Some(delta) = text.strip_prefix(self.text.as_str())
        {
            self.push_str(delta, cx);
            return;
        }
        self.stream_fade.note_replace();

        self.text.clear();
        self.text.push_str(text);
        self.parsed_error = None;
        self.increment_update(text, false, cx);
    }

    pub(super) fn set_element_text(&mut self, text: &SharedString, cx: &mut Context<Self>) {
        let same_string = self.element_text.as_ref().is_some_and(|current| {
            current.as_ptr() == text.as_ptr() && current.len() == text.len()
        });
        if same_string && self.text.len() == text.len() {
            return;
        }
        self.element_text = Some(text.clone());
        self.set_text(text, cx);
    }

    pub fn push_str(&mut self, new_text: &str, cx: &mut Context<Self>) {
        if new_text.is_empty() {
            return;
        }
        self.stream_fade.note_extend(self.text.len());
        self.text.push_str(new_text);
        self.increment_update(new_text, true, cx);
    }

    pub fn motion(mut self, motion: TextViewMotion) -> Self {
        self.set_motion(motion);
        self
    }

    pub fn set_motion(&mut self, motion: TextViewMotion) {
        self.stream_fade.set_motion(motion);
    }

    pub(crate) fn set_markdown_extensions(
        &mut self,
        markdown_extensions: Arc<MarkdownExtensions>,
        cx: &mut Context<Self>,
    ) {
        if self.markdown_extensions.revision() == markdown_extensions.revision() {
            return;
        }

        let parser_configuration_changed = !self
            .markdown_extensions
            .has_same_parser_configuration(&markdown_extensions);
        self.markdown_extensions = markdown_extensions;
        if parser_configuration_changed && self.format == TextViewFormat::Markdown {
            let text = self.text.clone();
            self.increment_update(&text, false, cx);
        }
    }

    pub fn selected_text(&self) -> String {
        self.selected_text_in(None)
    }

    pub fn selected_source_range(&self) -> Option<Range<usize>> {
        if self.format != TextViewFormat::Markdown {
            return None;
        }
        if self.select_all {
            return Some(0..self.source().len());
        }
        self.parsed_content.document.selected_source_range()
    }

    fn effective_format(&self) -> SelectionFormat {
        match self.format {
            TextViewFormat::Markdown => self.selection_format,
            TextViewFormat::Html => SelectionFormat::Plain,
        }
    }

    pub(super) fn selected_text_in(&self, blocks: Option<RangeInclusive<usize>>) -> String {
        let format = self.effective_format();

        if self.select_all {
            if format == SelectionFormat::Source {
                return self.source().to_string();
            }

            return self.parsed_content.document.text();
        }

        // A multi-click stores the plain text it selected, which is a shortcut
        // past the block walk. Source mode cannot take it: the word it stored
        // has lost its markup. The click also set the inline selection it came
        // from, so the walk reconstructs the same range with the markup intact.
        if format != SelectionFormat::Source
            && let Some(text) = &self.selected_text_override
        {
            return text.clone();
        }

        self.parsed_content.document.selected_text(format, blocks)
    }

    fn invalidate_measured_heights(&self) {
        let count = self.list_state.item_count();
        if count > 0 {
            self.list_state.remeasure_items(0..count);
        }
    }

    pub fn invalidate_inline_layout(&mut self, cx: &mut Context<Self>) {
        self.preserve_selection_for_reflow();
        cx.notify();
    }

    pub(super) fn preserve_selection_for_reflow(&mut self) {
        self.preserve_inline_selection = true;
        self.compatible_layout_update = true;
        self.invalidate_measured_heights();
    }

    fn increment_update(&mut self, text: &str, append: bool, cx: &mut Context<Self>) {
        self.revision += 1;
        if !append {
            self.full_update_revision = self.revision;
            self.selection_revision = self.selection_revision.wrapping_add(1);
        }
        let parse_synchronously = !append && text.len() <= MAX_SYNC_FULL_REPLACE_BYTES;
        let update_options = UpdateOptions {
            revision: self.revision,
            append,
            mode: if append {
                ParseMode::Compatible
            } else if parse_synchronously {
                ParseMode::BaselineAck
            } else {
                ParseMode::Replace
            },
            pending_text: text.to_string(),
            markdown_extensions: self.markdown_extensions.clone(),
        };

        // Keep small full replacements synchronous so their first layout has
        // the exact content height. Larger replacements use the existing
        // background parser, bounding synchronous parser input on the UI thread.
        if parse_synchronously {
            match parse_content(self.format, ParsedContent::default(), &update_options) {
                Ok(content) => {
                    self.reconcile_range_highlights(&content.document, self.revision, false);
                    self.stream_fade.record(
                        &self.parsed_content.document,
                        &content.document,
                        Instant::now(),
                    );
                    self.parsed_content = content;
                    self.parsed_error = None;
                    self.invalidate_measured_heights();
                    if !self.is_selecting {
                        self.reset_selection_and_adapter(cx);
                    }
                }
                Err(err) => {
                    self.stream_fade.discard_pending();
                    self.parsed_error = Some(err);
                }
            }
            // Keep the background parser's accumulated document in sync so a
            // later append extends this baseline instead of parsing the delta
            // as a standalone document.
            _ = self.tx.try_send(update_options);
            cx.notify();
            return;
        }

        _ = self.tx.try_send(update_options);
    }

    fn commits(&self, parsed_update: &ParsedUpdate) -> bool {
        if parsed_update.revision < self.full_update_revision
            || parsed_update.revision <= self.committed_revision
        {
            return false;
        }
        debug_assert!(!parsed_update.baseline_ack || parsed_update.full_parse);
        !parsed_update.baseline_ack
    }

    fn commit_parsed_update(&mut self, parsed_update: ParsedUpdate, cx: &mut Context<Self>) {
        if !self.commits(&parsed_update) {
            return;
        }

        match parsed_update.result {
            Ok(content) => {
                let append = parsed_update.selection_compatible && !parsed_update.full_parse;
                if append && self.full_update_revision <= self.committed_revision {
                    self.splice_appended_blocks(&content.document);
                }
                self.reconcile_range_highlights(&content.document, parsed_update.revision, append);
                self.stream_fade.record(
                    &self.parsed_content.document,
                    &content.document,
                    Instant::now(),
                );
                // This result may cover only part of the queued appends.
                // Keep the uncommitted tail pending from this document's end,
                // rather than consuming its fade with the earlier chunk.
                if parsed_update.revision < self.revision {
                    self.stream_fade.note_extend(content.document.source.len());
                }
                self.parsed_content = content;
                self.parsed_error = None;
                self.compatible_layout_update = parsed_update.selection_compatible;
                if parsed_update.full_parse {
                    self.invalidate_measured_heights();
                }
            }
            Err(err) => {
                self.stream_fade.discard_pending();
                self.parsed_error = Some(err);
            }
        }
        // Don't interrupt an active drag-selection; the stored
        // positions remain valid for append-only updates and will
        // self-correct on the next mouse-move event.
        if !parsed_update.selection_compatible && !self.is_selecting {
            self.reset_selection_and_adapter(cx);
        }
        cx.notify();
    }

    pub fn rendered_text(&self) -> RenderedText {
        RenderedText::new(
            self.entity_id,
            self.committed_revision,
            self.parsed_content.document.clone(),
            self.rendered_index.clone(),
        )
    }

    pub fn set_range_highlights(
        &mut self,
        highlights: impl IntoIterator<Item = RangeHighlight>,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeHighlightError> {
        if self.format != TextViewFormat::Markdown {
            return Err(RangeHighlightError::Unsupported);
        }
        let text = self.rendered_text();
        self.range_highlights = RangeHighlightFrame::new(&text, highlights)?.map(Arc::new);
        cx.notify();
        Ok(())
    }

    pub fn reveal_range(
        &mut self,
        range: Range<usize>,
        cx: &mut Context<Self>,
    ) -> Result<(), RangeHighlightError> {
        if self.format != TextViewFormat::Markdown {
            return Err(RangeHighlightError::Unsupported);
        }
        let text = self.rendered_text();
        if text.is_empty() && range == (0..0) {
            // Nothing to reveal in an empty view.
            self.pending_reveal = None;
            return Ok(());
        }
        let now = cx.background_executor().now();
        self.pending_reveal = Some(
            PendingReveal::new(&text, &range, now).ok_or(RangeHighlightError::InvalidRange(0))?,
        );
        cx.notify();
        Ok(())
    }

    pub fn clear_range_highlights(&mut self, cx: &mut Context<Self>) {
        if self.range_highlights.take().is_some() {
            cx.notify();
        }
    }

    fn reconcile_range_highlights(&mut self, new: &ParsedDocument, revision: usize, append: bool) {
        let tail_only = append && self.full_update_revision <= self.committed_revision;
        self.committed_revision = revision;
        self.rendered_index = Arc::default();
        if self.range_highlights.is_some() || self.pending_reveal.is_some() {
            let remap = LeafRemap::new(&self.parsed_content.document, new, tail_only);
            self.range_highlights = self
                .range_highlights
                .take()
                .and_then(|highlights| highlights.remap(&remap))
                .map(Arc::new);
            self.pending_reveal = self
                .pending_reveal
                .take()
                .and_then(|reveal| reveal.remap(&remap));
        }
    }

    fn splice_appended_blocks(&self, new: &ParsedDocument) {
        let old = &self.parsed_content.document.blocks;
        let (old_count, new_count) = (old.len(), new.blocks.len());
        if old_count == 0 || new_count <= old_count || self.list_state.item_count() != old_count {
            return;
        }

        // The last block is always measured again: it was parsed again, and
        // it is no longer the last one.
        let unchanged = old
            .iter()
            .zip(new.blocks.iter())
            .take(old_count - 1)
            .take_while(|(old_block, new_block)| {
                old_block
                    .span()
                    .is_some_and(|span| new_block.span() == Some(span))
            })
            .count();
        // Keeps the replaced blocks' heights as hints and the scroll offset
        // inside them, and has the next layout measure only the blocks that
        // are not measured.
        self.list_state.remeasure_items(unchanged..old_count);
        self.list_state
            .splice(old_count..old_count, new_count - old_count);
    }

    pub(super) fn update_bounds(&mut self, bounds: Bounds<Pixels>, _cx: &mut App) {
        self.bounds = bounds;
    }

    pub(super) fn block_ix_at(&self, content_y: Pixels) -> Option<usize> {
        if !self.scrollable {
            return None;
        }

        let origin = self.bounds.origin.y + self.scroll_offset().y;
        let count = self.list_state.item_count();
        let mut ix = self.list_state.logical_scroll_top().item_ix;
        while ix < count {
            let bounds = self.list_state.bounds_for_item(ix)?;
            if content_y < bounds.bottom() - origin {
                return Some(ix);
            }
            ix += 1;
        }

        count.checked_sub(1)
    }

    #[doc(hidden)]
    pub fn bounds(&self) -> Bounds<Pixels> {
        self.bounds
    }

    #[doc(hidden)]
    pub fn list_state(&self) -> &ListState {
        &self.list_state
    }

    #[doc(hidden)]
    pub fn is_selecting(&self) -> bool {
        self.is_selecting
    }

    #[doc(hidden)]
    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus_handle
    }

    pub(super) fn has_view_selection(&self) -> bool {
        self.select_all
            || self.multi_click_selection.is_some()
            || self.selected_text_override.is_some()
    }

    pub(super) fn stop_auto_scroll(&mut self) {
        self.auto_scroll.stop();
    }

    pub(super) fn reset_selection(&mut self) {
        self.preserve_inline_selection = false;
        self.multi_click_selection = None;
        self.selected_text_override = None;
        self.select_all = false;
        self.is_selecting = false;
        self.auto_scroll.stop();
        // Clear the inline selection state synchronously, so offscreen
        // (virtualized) views that won't repaint don't leak stale selection
        // text into a new cross-view copy.
        self.parsed_content.document.clear_selection();
    }

    fn reset_selection_and_adapter(&mut self, cx: &mut App) {
        self.reset_selection();
        self.selection_adapter.set_local_selection(false, cx);
    }

    pub fn clear_selection(&mut self, cx: &mut Context<Self>) {
        self.reset_selection_and_adapter(cx);
        cx.notify();
    }

    pub(super) fn scroll_offset(&self) -> Point<Pixels> {
        if self.scrollable {
            self.list_state.scroll_px_offset_for_scrollbar()
        } else {
            Point::default()
        }
    }

    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        self.multi_click_selection = None;
        self.selected_text_override = None;
        self.select_all = true;
        self.is_selecting = false;
        self.auto_scroll.stop();
        self.selection_adapter.set_local_selection(true, cx);
        cx.notify();
    }

    pub(crate) fn set_multi_click_selection(
        &mut self,
        pos: Point<Pixels>,
        kind: TextViewMultiClickKind,
        selected_text: String,
        cx: &mut App,
    ) {
        self.preserve_inline_selection = false;
        let scroll_offset = self.scroll_offset();
        let pos = pos - self.bounds.origin - scroll_offset;
        self.multi_click_selection = Some(TextViewMultiClickSelection {
            pos,
            kind,
            line_bounds: None,
        });
        self.selected_text_override = Some(selected_text);
        self.select_all = false;
        self.is_selecting = false;
        self.auto_scroll.stop();
        self.selection_adapter.set_local_selection(true, cx);
    }

    pub(crate) fn set_multi_click_line(&mut self, bounds: Bounds<Pixels>, cx: &mut App) {
        self.set_multi_click_selection(
            bounds.center(),
            TextViewMultiClickKind::Line,
            String::new(),
            cx,
        );
        self.selected_text_override = None;
        let offset = self.bounds.origin + self.scroll_offset();
        if let Some(selection) = self.multi_click_selection.as_mut() {
            selection.line_bounds = Some(Bounds::new(bounds.origin - offset, bounds.size));
        }
    }

    pub(super) fn set_auto_scroll(&mut self, delta: Option<Pixels>, cx: &mut Context<Self>) {
        self.auto_scroll.set(delta, cx, |delta, state, cx| {
            state.list_state.scroll_by(delta);
            cx.notify();
        });
    }

    pub(crate) fn selection_points(&self, cx: &App) -> Option<(Point<Pixels>, Point<Pixels>)> {
        if !self.selectable {
            return None;
        }
        self.selection_adapter.selection_points(cx)
    }

    pub(crate) fn has_selection(&self, cx: &App) -> bool {
        self.has_view_selection() || self.selection_points(cx).is_some()
    }

    pub(super) fn on_action_select_all(
        &mut self,
        _: &SelectAll,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.selectable {
            cx.propagate();
            return;
        }

        self.select_all(cx);
    }

    pub(crate) fn is_selectable(&self) -> bool {
        self.selectable
    }

    pub(crate) fn is_all_selected(&self) -> bool {
        self.select_all
    }

    pub(crate) fn multi_click_selection(&self) -> Option<TextViewMultiClickSelection> {
        let scroll_offset = self.scroll_offset();
        self.multi_click_selection.map(|selection| {
            let pos = selection.pos + scroll_offset + self.bounds.origin;
            let line_bounds = selection.line_bounds.map(|bounds| {
                Bounds::new(
                    bounds.origin + scroll_offset + self.bounds.origin,
                    bounds.size,
                )
            });
            TextViewMultiClickSelection {
                pos,
                line_bounds,
                ..selection
            }
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TextViewMultiClickSelection {
    pub(crate) pos: Point<Pixels>,
    pub(crate) kind: TextViewMultiClickKind,
    pub(crate) line_bounds: Option<Bounds<Pixels>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextViewMultiClickKind {
    Word,
    Paragraph,
    Line,
}

impl TextViewState {
    fn reveal_frame(
        &mut self,
        now: Instant,
        window: &mut Window,
    ) -> (Option<RevealRequest>, Option<usize>) {
        if self.max_lines.is_some()
            || self
                .pending_reveal
                .as_ref()
                .is_some_and(|reveal| reveal.is_expired(now))
        {
            self.pending_reveal = None;
        }
        let Some(pending) = &self.pending_reveal else {
            return (None, None);
        };

        let block_ix = pending.block_ix(&self.parsed_content.document);
        let block_off_screen = |ix: usize| {
            let viewport = self.list_state.viewport_bounds();
            self.list_state.bounds_for_item(ix).is_none_or(|item| {
                item.bottom() <= viewport.top() || item.top() >= viewport.bottom()
            })
        };
        let reveal_block = block_ix.filter(|ix| {
            self.scrollable
                && (pending.is_block() || !pending.was_laid_out())
                && block_off_screen(*ix)
        });
        let request = pending.request();
        if pending.is_block() {
            // A block has no line to wait for.
            self.pending_reveal = None;
        } else {
            window.request_animation_frame();
        }
        (request, reveal_block)
    }
}

impl Render for TextViewState {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let typography = (window.text_style(), window.rem_size());
        let body_font_size = typography.0.font_size.to_pixels(typography.1);
        if self
            .layout_text_style
            .as_ref()
            .is_some_and(|previous| previous != &typography)
        {
            // ListState invalidates its cached rows for width changes, but does
            // not know that an inherited font or rem change affects offscreen
            // inline metrics. Retain logical selections just as for resources.
            self.preserve_selection_for_reflow();
        }
        self.layout_text_style = Some(typography);
        let state = cx.entity();
        let stream_fade = self.stream_fade.frame(Instant::now(), cx.reduce_motion());
        if stream_fade.is_some() && self.fade_tick.is_none() {
            self.fade_tick = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(STREAM_FADE_TICK).await;
                _ = this.update(cx, |state, cx| {
                    state.fade_tick = None;
                    cx.notify();
                });
            }));
        }
        let (reveal, reveal_block) = self.reveal_frame(cx.background_executor().now(), window);
        // Built every frame, so everything in it is shared, not copied.
        let node_cx = NodeContext {
            offset: self.parsed_content.node_cx.offset,
            link_refs: self.parsed_content.node_cx.link_refs.clone(),
            style: self.text_view_style.clone(),
            code_block_actions: self.code_block_actions.clone(),
            code_block_highlighter: self.code_block_highlighter.clone(),
            table_actions: self.table_actions.clone(),
            link_click_handler: self.link_click_handler.clone(),
            image_source: self.image_source.clone(),
            markdown_extensions: self.markdown_extensions.clone(),
            stream_fade,
            range_highlights: self.range_highlights.clone(),
            reveal,
            body_font_size,
        };

        let content = v_flex()
            .w_full()
            // Clamped content must keep its natural height: stretching it to
            // the capped box would hide the overflow the clamp has to measure.
            .when(self.max_lines.is_none(), |this| this.h_full())
            .map(|this| match &self.parsed_error {
                None => this.child(self.parsed_content.document.render_root(
                    if self.scrollable {
                        Some(self.list_state.clone())
                    } else {
                        None
                    },
                    &node_cx,
                    window,
                    cx,
                )),
                Some(err) => this.child(
                    v_flex()
                        .gap_1()
                        .child("Failed to parse content")
                        .child(err.to_string()),
                ),
            })
            .on_prepaint(move |bounds, window, cx| {
                let (
                    size_changed,
                    selection_involves_view,
                    has_selection_snapshot,
                    is_selecting,
                    compatible_layout_update,
                    preserve_width_selection,
                ) = {
                    let state = state.read(cx);
                    let has_selection_snapshot = state.selection_adapter.has_selection_snapshot(cx);
                    let preserve_width_selection = !state.is_selecting
                        && state.bounds().size.width != bounds.size.width
                        && (has_selection_snapshot || state.has_view_selection());
                    (
                        state.bounds().size != bounds.size,
                        state.selection_adapter.is_part_of_window_selection(cx),
                        has_selection_snapshot,
                        state.is_selecting,
                        // A measurement prepaint must not consume compatibility
                        // before the same entity's visible column prepaints.
                        state.compatible_layout_update
                            || preserve_width_selection
                            || state.preserve_inline_selection,
                        preserve_width_selection,
                    )
                };
                let mut revision_changed = false;
                state.update(cx, |state, cx| {
                    revision_changed = state
                        .selection_adapter
                        .update_layout_revision(state.selection_revision, state.is_selecting);
                    if preserve_width_selection {
                        // ListState already remeasures width changes. Arm logical
                        // retention before inline paint without invalidating rows again.
                        state.preserve_inline_selection = true;
                    }
                    state.update_bounds(bounds, cx);
                    state.compatible_layout_update = false;
                });
                if !is_selecting
                    && ((size_changed && selection_involves_view && !compatible_layout_update)
                        || (revision_changed && has_selection_snapshot))
                {
                    TextSelection::clear(window, cx);
                }
            });
        // After `render_root`, which resets the list when the block count
        // changed other than by an append, and with it the scroll position.
        if let Some(block_ix) = reveal_block {
            self.list_state.scroll_to_reveal_item(block_ix);
        }
        content
    }
}

#[derive(Clone, PartialEq, Default)]
pub(crate) struct ParsedContent {
    pub(crate) document: ParsedDocument,
    pub(crate) node_cx: node::NodeContext,
}

struct UpdateFuture {
    format: TextViewFormat,
    content: ParsedContent,
    rx: Pin<Box<Receiver<UpdateOptions>>>,
    tx_result: Sender<ParsedUpdate>,
}

impl UpdateFuture {
    fn new(
        format: TextViewFormat,
        rx: Receiver<UpdateOptions>,
        tx_result: Sender<ParsedUpdate>,
    ) -> Self {
        Self {
            format,
            content: Default::default(),
            rx: Box::pin(rx),
            tx_result,
        }
    }
}

impl Future for UpdateFuture {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        loop {
            match self.rx.as_mut().poll_next(cx) {
                Poll::Ready(Some(mut options)) => {
                    let hit_coalesce_budget =
                        merge_pending_options(&mut options, self.rx.as_ref().get_ref());

                    let res = parse_content(self.format, self.content.clone(), &options);
                    if let Ok(content) = &res {
                        self.content = content.clone();
                    }
                    _ = self.tx_result.try_send(ParsedUpdate {
                        revision: options.revision,
                        full_parse: !options.append,
                        selection_compatible: options.mode == ParseMode::Compatible,
                        baseline_ack: options.mode == ParseMode::BaselineAck,
                        result: res,
                    });
                    if hit_coalesce_budget {
                        cx.waker().wake_by_ref();
                        return Poll::Pending;
                    }
                    continue;
                }
                Poll::Ready(None) => return Poll::Ready(()),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

#[derive(Clone)]
struct UpdateOptions {
    revision: usize,
    pending_text: String,
    append: bool,
    mode: ParseMode,
    markdown_extensions: Arc<MarkdownExtensions>,
}

impl UpdateOptions {
    fn merge(&mut self, next: UpdateOptions) {
        if next.append {
            self.pending_text.push_str(&next.pending_text);
            self.revision = next.revision;
            if self.mode != ParseMode::Replace {
                self.mode = ParseMode::Compatible;
            }
        } else {
            *self = next;
        }
    }
}

struct ParsedUpdate {
    revision: usize,
    full_parse: bool,
    selection_compatible: bool,
    baseline_ack: bool,
    result: Result<ParsedContent, SharedString>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ParseMode {
    BaselineAck,
    Replace,
    Compatible,
}

fn merge_pending_options(options: &mut UpdateOptions, rx: &Receiver<UpdateOptions>) -> bool {
    let mut update_count = 1;

    while update_count < MAX_COALESCED_UPDATES_PER_PARSE {
        match rx.try_recv() {
            Ok(next_options) => {
                options.merge(next_options);
                update_count += 1;
            }
            Err(_) => return false,
        }
    }

    true
}

fn parse_content(
    format: TextViewFormat,
    mut content: ParsedContent,
    options: &UpdateOptions,
) -> Result<ParsedContent, SharedString> {
    let mut node_cx = NodeContext {
        markdown_extensions: options.markdown_extensions.clone(),
        ..NodeContext::default()
    };

    // Re-parse the last block together with the appended text, so a block the
    // new text continues (an unclosed list, a fenced code block) is not split
    // in two. A block without a span cannot be located in `source` — the HTML
    // parser never records spans — so it is left in place and only the
    // appended text is parsed, positioned at the end of the current source.
    let last_span = options
        .append
        .then(|| {
            content
                .document
                .blocks
                .last()
                .and_then(|block| block.span())
        })
        .flatten();

    let mut source = String::new();
    if let Some(span) = last_span {
        Arc::make_mut(&mut content.document.blocks).pop();
        node_cx.offset = span.start;
        source.push_str(&content.document.source[span.start..]);
        source.push_str(&options.pending_text);
    } else {
        if options.append {
            node_cx.offset = content.document.source.len();
        }
        source.push_str(&options.pending_text);
    }

    let new_document = match format {
        TextViewFormat::Markdown => format::markdown::parse(&source, &mut node_cx),
        TextViewFormat::Html => format::html::parse(&source, &mut node_cx),
    }?;

    if options.append {
        content.document.source =
            format!("{}{}", content.document.source, options.pending_text).into();
        Arc::make_mut(&mut content.document.blocks)
            .extend(Arc::unwrap_or_clone(new_document.blocks));
    } else {
        content.document = new_document;
    }

    Ok(content)
}
