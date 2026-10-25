use gpui::Corners;
use std::{
    cell::RefCell,
    collections::HashMap,
    mem,
    ops::Range,
    rc::Rc,
    sync::{Arc, Mutex, Weak},
};

use gpui::{
    App, BorderStyle, Bounds, ClickEvent, CursorStyle, Edges, Element, ElementId, GlobalElementId,
    Half, HighlightStyle, Hitbox, HitboxBehavior, Hsla, InspectorElementId, IntoElement, LayoutId,
    MouseButton, MouseClickEvent, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point,
    SharedString, StyledText, TextAlign, TextLayout, TextRun, TextStyle, Window, point, px, quad,
    size,
};

use crate::{
    GlobalState, TextSelection,
    input::Selection,
    text::TextViewMultiClickKind,
    text::node::LinkMark,
    text::range_highlight::RevealAt,
    text::selection::word_range_at,
    text::state::LineSpan,
    text::text_view::{LinkClickHandlerFn, handle_link_click},
    text_selection::text_rows_extent,
};

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct InlineHighlight {
    pub(super) style: HighlightStyle,
    pub(super) font_family: Option<SharedString>,
    pub(super) font_size_scale: Option<f32>,
}

impl InlineHighlight {
    fn highlight(mut self, other: &InlineHighlight) -> Self {
        self.style = self.style.highlight(other.style);
        if other.font_family.is_some() {
            self.font_family = other.font_family.clone();
        }
        if other.font_size_scale.is_some() {
            self.font_size_scale = other.font_size_scale;
        }
        self
    }
}

impl From<HighlightStyle> for InlineHighlight {
    fn from(style: HighlightStyle) -> Self {
        Self {
            style,
            font_family: None,
            font_size_scale: None,
        }
    }
}

pub(super) fn combine_highlights(
    a: impl IntoIterator<Item = (Range<usize>, InlineHighlight)>,
    b: impl IntoIterator<Item = (Range<usize>, InlineHighlight)>,
) -> Vec<(Range<usize>, InlineHighlight)> {
    let mut endpoints = Vec::new();
    let mut highlights = Vec::new();
    for (range, highlight) in a.into_iter().chain(b) {
        if !range.is_empty() {
            let id = highlights.len();
            endpoints.push((range.start, id, true));
            endpoints.push((range.end, id, false));
            highlights.push(highlight);
        }
    }
    endpoints.sort_unstable_by_key(|(position, _, _)| *position);

    let mut combined = Vec::new();
    let mut active: Vec<usize> = Vec::new();
    let mut ix = 0;
    for (position, id, is_start) in endpoints {
        if position > ix && !active.is_empty() {
            let style = active.iter().fold(InlineHighlight::default(), |acc, id| {
                acc.highlight(&highlights[*id])
            });
            combined.push((ix..position, style));
        }
        ix = position;
        if is_start {
            active.push(id);
        } else {
            active.retain(|active_id| *active_id != id);
        }
    }
    combined
}

pub(super) fn fade_highlights(
    highlights: Vec<(Range<usize>, InlineHighlight)>,
    fades: &[(Range<usize>, f32)],
) -> Vec<(Range<usize>, InlineHighlight)> {
    if fades.is_empty() {
        return highlights;
    }
    let fade_highlights = fades.iter().map(|(range, fade_out)| {
        (
            range.clone(),
            InlineHighlight::from(HighlightStyle {
                fade_out: Some(*fade_out),
                ..Default::default()
            }),
        )
    });
    let mut combined = combine_highlights(highlights, fade_highlights);
    // GPUI fades only the glyph color, so explicit decoration colors fade
    // here too, or a link underline would show before its text does.
    for (_, highlight) in &mut combined {
        let Some(fade_out) = highlight.style.fade_out else {
            continue;
        };
        let style = &mut highlight.style;
        for color in [
            style.background_color.as_mut(),
            style.underline.as_mut().and_then(|u| u.color.as_mut()),
            style.strikethrough.as_mut().and_then(|s| s.color.as_mut()),
        ]
        .into_iter()
        .flatten()
        {
            color.fade_out(fade_out);
        }
    }
    combined
}

pub(super) fn text_runs(
    text_len: usize,
    default_style: &TextStyle,
    highlights: &[(Range<usize>, InlineHighlight)],
) -> Vec<TextRun> {
    let mut runs = Vec::with_capacity(highlights.len() * 2 + 1);
    let mut ix = 0;
    for (range, highlight) in highlights {
        if ix < range.start {
            runs.push(default_style.clone().to_run(range.start - ix));
        }
        let mut run = default_style
            .clone()
            .highlight(highlight.style)
            .to_run(range.len());
        if let Some(family) = &highlight.font_family {
            run.font.family = family.clone();
        }
        runs.push(run);
        ix = range.end;
    }
    if ix < text_len {
        runs.push(default_style.to_run(text_len - ix));
    }
    runs
}

pub(super) fn text_size_ranges(
    text_len: usize,
    highlights: &[(Range<usize>, InlineHighlight)],
) -> Vec<(Range<usize>, f32)> {
    let mut ranges: Vec<(Range<usize>, f32)> = Vec::new();
    let mut push = |range: Range<usize>, scale: f32| {
        if range.is_empty() {
            return;
        }
        if let Some((last, last_scale)) = ranges.last_mut()
            && *last_scale == scale
            && last.end == range.start
        {
            last.end = range.end;
        } else {
            ranges.push((range, scale));
        }
    };
    let mut cursor = 0;
    for (range, highlight) in highlights {
        push(cursor..range.start, 1.);
        push(range.clone(), highlight.font_size_scale.unwrap_or(1.));
        cursor = range.end;
    }
    push(cursor..text_len, 1.);
    ranges
}

pub(super) struct Inline {
    text: SharedString,
    links: Rc<Vec<(Range<usize>, LinkMark)>>,
    highlights: Vec<(Range<usize>, InlineHighlight)>,
    styled_text: StyledText,

    text_style: Option<TextStyle>,
    paint_origin: Option<Point<Pixels>>,
    selection_bounds: Option<Bounds<Pixels>>,
    selection_source: Option<(Arc<Mutex<InlineState>>, Range<usize>)>,

    range_backgrounds: Vec<(Range<usize>, Hsla)>,

    reveal: Option<RevealAt>,
    link_click_handler: Option<Arc<LinkClickHandlerFn>>,

    retained_key: Option<(Vec<TextRun>, TextStyle)>,

    handed_over: bool,

    state: Arc<Mutex<InlineState>>,
}

#[derive(Debug, Default, PartialEq)]
pub(crate) struct InlineState {
    pub(super) text: SharedString,
    pub(super) selection: Option<Selection>,
}

struct RetainedLayout {
    state: Weak<Mutex<InlineState>>,
    styled_text: StyledText,
    text: SharedString,
    runs: Vec<TextRun>,
    text_style: TextStyle,
}

thread_local! {
    static RETAINED_LAYOUTS: RefCell<HashMap<usize, RetainedLayout>> = RefCell::new(HashMap::new());
}

const RETAINED_SWEEP_AT: usize = 4096;

fn state_key(state: &Arc<Mutex<InlineState>>) -> usize {
    Arc::as_ptr(state) as usize
}

fn take_retained_layout(state: &Arc<Mutex<InlineState>>) -> Option<RetainedLayout> {
    RETAINED_LAYOUTS.with(|layouts| {
        let retained = layouts.borrow_mut().remove(&state_key(state))?;
        // The address may belong to a new state by now.
        retained
            .state
            .upgrade()
            .is_some_and(|live| Arc::ptr_eq(&live, state))
            .then_some(retained)
    })
}

fn has_retained_layout(state: &Arc<Mutex<InlineState>>) -> bool {
    RETAINED_LAYOUTS.with(|layouts| layouts.borrow().contains_key(&state_key(state)))
}

fn retain_layout(state: &Arc<Mutex<InlineState>>, retained: RetainedLayout) {
    RETAINED_LAYOUTS.with(|layouts| {
        let mut layouts = layouts.borrow_mut();
        if layouts.len() >= RETAINED_SWEEP_AT {
            layouts.retain(|_, retained| retained.state.strong_count() > 0);
        }
        layouts.insert(state_key(state), retained);
    });
}

impl InlineState {
    pub(crate) fn set_text(&mut self, text: SharedString) {
        self.text = text;
    }
}

impl Inline {
    fn retain_styled_text(&mut self) {
        if self.handed_over || self.retained_key.is_none() || has_retained_layout(&self.state) {
            return;
        }
        let Some((runs, text_style)) = self.retained_key.take() else {
            return;
        };
        retain_layout(
            &self.state,
            RetainedLayout {
                state: Arc::downgrade(&self.state),
                styled_text: mem::replace(&mut self.styled_text, StyledText::new("")),
                text: self.text.clone(),
                runs,
                text_style,
            },
        );
        self.handed_over = true;
    }

    fn reclaim_styled_text(&mut self) -> bool {
        if !self.handed_over {
            return true;
        }
        let Some(retained) = take_retained_layout(&self.state) else {
            return false;
        };
        self.styled_text = retained.styled_text;
        self.retained_key = Some((retained.runs, retained.text_style));
        self.handed_over = false;
        true
    }

    pub(super) fn new(
        state: Arc<Mutex<InlineState>>,
        links: Vec<(Range<usize>, LinkMark)>,
        highlights: Vec<(Range<usize>, InlineHighlight)>,
        link_click_handler: Option<Arc<LinkClickHandlerFn>>,
    ) -> Self {
        let text = state
            .lock()
            .map(|state| state.text.clone())
            .unwrap_or_default();

        Self {
            links: Rc::new(links),
            highlights,
            text: text.clone(),
            styled_text: StyledText::new(text),
            text_style: None,
            paint_origin: None,
            selection_bounds: None,
            selection_source: None,
            range_backgrounds: Vec::new(),
            reveal: None,
            link_click_handler,
            retained_key: None,
            handed_over: false,
            state,
        }
    }

    pub(super) fn text_style(mut self, text_style: TextStyle) -> Self {
        self.text_style = Some(text_style);
        self
    }

    pub(super) fn paint_origin(mut self, origin: Point<Pixels>) -> Self {
        self.paint_origin = Some(origin);
        self
    }

    pub(super) fn selection_bounds(mut self, bounds: Bounds<Pixels>) -> Self {
        self.selection_bounds = Some(bounds);
        self
    }

    pub(super) fn selection_source(
        mut self,
        state: Arc<Mutex<InlineState>>,
        range: Range<usize>,
    ) -> Self {
        self.selection_source = Some((state, range));
        self
    }

    pub(super) fn range_backgrounds(mut self, backgrounds: Vec<(Range<usize>, Hsla)>) -> Self {
        self.range_backgrounds = backgrounds;
        self
    }

    pub(super) fn reveal(mut self, reveal: Option<RevealAt>) -> Self {
        self.reveal = reveal;
        self
    }

    fn request_reveal(&self, window: &mut Window) {
        let Some(reveal) = &self.reveal else {
            return;
        };
        let text_layout = self.styled_text.layout();
        let bounds = text_layout.bounds();
        let line_height = text_layout.line_height();
        let glyphs = glyph_boxes(
            text_layout,
            window.text_style().text_align,
            bounds.size.width,
        );
        // The glyph drawing the text at the offset, or, for text with no
        // glyph of its own such as a line break, the next glyph, or the last.
        let offset = reveal.offset();
        let (row, left, right) = range_boxes(&glyphs, offset..offset + 1)
            .first()
            .copied()
            .or_else(|| {
                glyphs
                    .iter()
                    .find(|glyph| glyph.text.start >= offset)
                    .or(glyphs.last())
                    .map(|glyph| (glyph.row, glyph.left, glyph.right))
            })
            .unwrap_or((0, Pixels::ZERO, Pixels::ZERO));
        let line = Bounds::from_corners(
            point(
                bounds.left() + left,
                bounds.top() + line_height * row as f32,
            ),
            point(
                bounds.left() + right.max(left + px(1.)),
                bounds.top() + line_height * (row + 1) as f32,
            ),
        );
        window.request_autoscroll(line);
        // A list scrolls the line to its edge, which layout may miss by a
        // fraction of a pixel.
        let visible = window.content_mask().bounds.dilate(px(0.5));
        reveal.report(
            line,
            line.top() >= visible.top() && line.bottom() <= visible.bottom(),
        );
    }

    fn link_for_position(
        layout: &TextLayout,
        links: &Vec<(Range<usize>, LinkMark)>,
        position: Point<Pixels>,
    ) -> Option<LinkMark> {
        let offset = layout.index_for_position(position).ok()?;
        for (range, link) in links.iter() {
            if range.contains(&offset) {
                return Some(link.clone());
            }
        }

        None
    }

    fn link_range_for_position(
        layout: &TextLayout,
        links: &[(Range<usize>, LinkMark)],
        position: Point<Pixels>,
    ) -> Option<Range<usize>> {
        let offset = layout.index_for_position(position).ok()?;
        links
            .iter()
            .find(|(range, _)| range.contains(&offset))
            .map(|(range, _)| range.clone())
    }

    #[allow(unused)]
    fn paint_selected_bounds(&self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        window.paint_quad(gpui::PaintQuad {
            bounds,
            background: gpui::hsla(0.58, 0.85, 0.62, 0.01).into(),
            corner_radii: Corners::default(),
            border_color: gpui::transparent_black(),
            border_style: BorderStyle::default(),
            border_widths: gpui::Edges::all(px(0.)),
        });
    }

    fn layout_selections(
        &self,
        text_layout: &TextLayout,
        bounds: &Bounds<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) -> (bool, bool, Option<Selection>) {
        let Some(text_view_state) = GlobalState::global(cx).text_view_state() else {
            return (false, false, None);
        };

        let text_view_state = text_view_state.read(cx);
        let is_selectable = text_view_state.is_selectable();
        if !is_selectable {
            return (false, false, None);
        }

        if text_view_state.is_all_selected() {
            return (is_selectable, true, Some((0..self.text.len()).into()));
        }

        if text_view_state.preserve_inline_selection {
            let selection = if let Some((source, range)) = &self.selection_source {
                source
                    .lock()
                    .ok()
                    .and_then(|state| state.selection)
                    .and_then(|selection| {
                        let start = selection.start.max(range.start);
                        let end = selection.end.min(range.end);
                        (start < end)
                            .then(|| Selection::new(start - range.start, end - range.start))
                    })
            } else {
                self.state.lock().ok().and_then(|state| state.selection)
            };
            return (true, selection.is_some(), selection);
        }

        if let Some(selection) = text_view_state.multi_click_selection() {
            if selection.kind == TextViewMultiClickKind::Line {
                return (
                    true,
                    true,
                    selection
                        .line_bounds
                        .filter(|row| row.contains(&bounds.center()))
                        .map(|_| Selection::new(0, self.text.len())),
                );
            }
            return (
                is_selectable,
                true,
                selection_for_multi_click(
                    &self.text,
                    text_layout,
                    *bounds,
                    selection.pos,
                    selection.kind,
                )
                .map(Selection::from),
            );
        }

        let Some((selection_start, selection_end)) = text_view_state.selection_points(cx) else {
            return (is_selectable, false, None);
        };
        let line_height = window.line_height();

        // Use for debug selection bounds
        // self.paint_selected_bounds(Bounds::from_corners(selection_start, selection_end), window, cx);

        // NOTE: the selection is computed purely from the geometric band
        // (`selection_start`..`selection_end`), NOT from what is currently
        // visible. Every glyph of a *painted* element is laid out (its
        // `position_for_index` is valid) even when it is scrolled out of, or
        // clipped by, an ancestor's viewport — the content mask only clips the
        // painted pixels. Because the copied text is derived from
        // `InlineState.selection`, gating the selection on `content_mask` here
        // used to drop scrolled-out-but-selected glyphs, so a selection taller
        // than the viewport (e.g. a long chat message, or a drag with
        // auto-scroll) copied only the portion that happened to be on screen.
        //
        // This does not resurrect the #2156 clipped-hit-testing behavior: a
        // selection can only START on visible text (window selection resolves
        // endpoints with hitbox hover testing against visible Inline bounds),
        // so the band's endpoints are always anchored to on-screen text.
        // Content that is merely `overflow_hidden`
        // (not scrolled) lies outside that band and is still excluded, while
        // the highlight quads painted for off-screen glyphs are clipped away by
        // GPUI's content mask as before.
        //
        // Each character is tested with its row's top and height, so an inline
        // whose rows all miss the band, or all lie strictly inside it with no
        // endpoint on any row, has the same answer for every character. Decide
        // those without the walk below, which scans the layout twice per
        // character: one long code block alone made every paint of a held
        // selection cost tens of milliseconds.
        if self.text.is_empty() {
            return (true, true, None);
        }
        let (rows_top, rows_bottom) = match self.selection_bounds {
            Some(bounds) => (bounds.top(), bounds.top() + bounds.size.height),
            None => text_rows_extent(text_layout, line_height),
        };
        let band_top = selection_start.y.min(selection_end.y);
        let band_bottom = selection_start.y.max(selection_end.y);
        if rows_bottom <= band_top || rows_top > band_bottom {
            return (true, true, None);
        }
        if band_top < rows_top && band_bottom >= rows_bottom && text_layout.len() >= self.text.len()
        {
            return (true, true, Some((0..self.text.len()).into()));
        }

        let mut selection: Option<Selection> = None;
        let mut offset = 0;
        let mut chars = self.text.chars().peekable();
        while let Some(c) = chars.next() {
            let Some(pos) = text_layout.position_for_index(offset) else {
                offset += c.len_utf8();
                continue;
            };

            let next_offset = offset + c.len_utf8();
            let mut char_width = line_height.half();
            if let Some(next_pos) = text_layout.position_for_index(next_offset) {
                if next_pos.y == pos.y {
                    char_width = next_pos.x - pos.x;
                }
            }

            let selection_pos = self
                .selection_bounds
                .map_or(pos, |bounds| point(pos.x, bounds.top()));
            let selection_height = self
                .selection_bounds
                .map_or(line_height, |bounds| bounds.size.height);
            if point_in_text_selection(
                selection_pos,
                char_width,
                selection_start,
                selection_end,
                selection_height,
            ) {
                if selection.is_none() {
                    selection = Some((offset..offset).into());
                }

                if let Some(selection) = selection.as_mut() {
                    selection.end = next_offset;
                }
            }

            offset = next_offset;
        }

        (true, true, selection)
    }

    fn text_line_bounds(
        text_layout: &TextLayout,
        line_height: Pixels,
        mask_bounds: Bounds<Pixels>,
    ) -> Vec<Bounds<Pixels>> {
        let origin = text_layout.bounds().origin;
        let lines = text_layout.line_layouts();
        let row_count: usize = lines
            .iter()
            .map(|line| line.wrap_boundaries.len() + 1)
            .sum();
        let mut line_bounds = Vec::with_capacity(row_count);
        let mut row_ix = 0;
        let mut y = origin.y;
        for line in &lines {
            let layout = &line.unwrapped_layout;
            let mut row_start = 0;
            let row_ends = line
                .wrap_boundaries
                .iter()
                .map(|boundary| layout.runs[boundary.run_ix].glyphs[boundary.glyph_ix].index)
                .chain([line.len()]);
            for row_end in row_ends {
                let mut width = layout.x_for_index(row_end) - layout.x_for_index(row_start);
                row_ix += 1;
                if row_ix < row_count {
                    width += line_height.half();
                }
                let bounds = Bounds::new(point(origin.x, y), size(width, line_height))
                    .intersect(&mask_bounds);
                if bounds.size.width > px(0.) && bounds.size.height > px(0.) {
                    line_bounds.push(bounds);
                }
                y += line_height;
                row_start = row_end;
            }
        }
        line_bounds
    }

    fn selection_edges(
        selection: &Selection,
        text_layout: &TextLayout,
    ) -> Option<(Bounds<Pixels>, Bounds<Pixels>)> {
        let (start, end) = (
            selection.start.min(selection.end),
            selection.start.max(selection.end),
        );
        let line_height = text_layout.line_height();
        Some((
            crate::touch_selection::caret_line_box(
                text_layout.position_for_index(start)?,
                line_height,
            ),
            crate::touch_selection::caret_line_box(
                text_layout.position_for_index(end)?,
                line_height,
            ),
        ))
    }

    fn paint_selection(
        selection: &Selection,
        text_layout: &TextLayout,
        bounds: &Bounds<Pixels>,
        window: &mut Window,
        color: gpui::Hsla,
    ) {
        let mut start = selection.start;
        let mut end = selection.end;
        if end < start {
            std::mem::swap(&mut start, &mut end);
        }
        let Some(start_position) = text_layout.position_for_index(start) else {
            return;
        };
        let Some(end_position) = text_layout.position_for_index(end) else {
            return;
        };

        let line_height = text_layout.line_height();
        if start_position.y == end_position.y {
            window.paint_quad(quad(
                Bounds::from_corners(
                    start_position,
                    point(end_position.x, end_position.y + line_height),
                ),
                px(0.),
                color,
                Edges::default(),
                gpui::transparent_black(),
                BorderStyle::default(),
            ));
        } else {
            window.paint_quad(quad(
                Bounds::from_corners(
                    start_position,
                    point(bounds.right(), start_position.y + line_height),
                ),
                px(0.),
                color,
                Edges::default(),
                gpui::transparent_black(),
                BorderStyle::default(),
            ));

            if end_position.y > start_position.y + line_height {
                window.paint_quad(quad(
                    Bounds::from_corners(
                        point(bounds.left(), start_position.y + line_height),
                        point(bounds.right(), end_position.y),
                    ),
                    px(0.),
                    color,
                    Edges::default(),
                    gpui::transparent_black(),
                    BorderStyle::default(),
                ));
            }

            window.paint_quad(quad(
                Bounds::from_corners(
                    point(bounds.left(), end_position.y),
                    point(end_position.x, end_position.y + line_height),
                ),
                px(0.),
                color,
                Edges::default(),
                gpui::transparent_black(),
                BorderStyle::default(),
            ));
        }
    }

    fn paint_range_highlights(&self, text_layout: &TextLayout, window: &mut Window) {
        let glyphs = glyph_boxes(
            text_layout,
            window.text_style().text_align,
            text_layout.bounds().size.width,
        );
        let origin = text_layout.bounds().origin;
        let line_height = text_layout.line_height();
        for (range, color) in &self.range_backgrounds {
            for (row, left, right) in range_boxes(&glyphs, range.clone()) {
                window.paint_quad(gpui::fill(
                    Bounds::from_corners(
                        point(origin.x + left, origin.y + line_height * row as f32),
                        point(origin.x + right, origin.y + line_height * (row + 1) as f32),
                    ),
                    *color,
                ));
            }
        }
    }
}

impl IntoElement for Inline {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Inline {
    type RequestLayoutState = ();
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        global_element_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let text_style = self
            .text_style
            .clone()
            .unwrap_or_else(|| window.text_style());
        let runs = text_runs(self.text.len(), &text_style, &self.highlights);

        // Reuse the previous frame's shaped text when it was shaped from the
        // same text, runs and style; `StyledText` consumes its runs on every
        // layout, so they are handed over again either way.
        let retained = take_retained_layout(&self.state).filter(|retained| {
            retained.text == self.text && retained.runs == runs && retained.text_style == text_style
        });
        self.styled_text = match retained {
            Some(retained) => retained.styled_text.with_runs(runs.clone()),
            None => StyledText::new(self.text.clone()).with_runs(runs.clone()),
        };
        self.retained_key = Some((runs, text_style));
        let (layout_id, _) =
            self.styled_text
                .request_layout(global_element_id, inspector_id, window, cx);

        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let bounds = Bounds::new(self.paint_origin.unwrap_or(bounds.origin), bounds.size);
        self.styled_text
            .prepaint(id, inspector_id, bounds, &mut (), window, cx);

        // Report this element's laid-out extent so an ancestor TextView with
        // `max_lines` can snap its clip to a whole-line boundary. The state
        // stack only holds an entry during prepaint when that view set
        // `max_lines`, so this is a no-op otherwise.
        if let Some(text_view_state) = GlobalState::global(cx).text_view_state() {
            let state = text_view_state.read(cx);
            if state.max_lines.is_some()
                && let Ok(mut line_spans) = state.line_spans.lock()
            {
                line_spans.push(LineSpan {
                    top: bounds.top(),
                    bottom: bounds.bottom(),
                    line_height: window.line_height(),
                });
            }
        }

        self.request_reveal(window);

        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        self.retain_styled_text();
        hitbox
    }

    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let bounds = Bounds::new(self.paint_origin.unwrap_or(bounds.origin), bounds.size);
        let current_view = window.current_view();
        let hitbox = prepaint;
        if !self.reclaim_styled_text() {
            // Cannot happen (only this element takes what it handed over,
            // and a live state is never swept); skip the frame rather than
            // paint an unmeasured placeholder.
            return;
        }
        let text_layout = self.styled_text.layout().clone();
        if !self.range_backgrounds.is_empty() {
            self.paint_range_highlights(&text_layout, window);
        }
        self.styled_text
            .paint(global_id, None, bounds, &mut (), &mut (), window, cx);

        // layout selections
        let (is_selectable, is_selection, selection) =
            self.layout_selections(&text_layout, &bounds, window, cx);

        let Ok(mut state) = self.state.lock() else {
            return;
        };

        state.selection = selection;
        if let Some((source, range)) = &self.selection_source
            && let Some(selection) = selection
            && let Ok(mut source) = source.lock()
        {
            let start = range.start + selection.start;
            let end = range.start + selection.end;
            source.selection = Some(match source.selection {
                Some(previous) => Selection::new(previous.start.min(start), previous.end.max(end)),
                None => Selection::new(start, end),
            });
        }

        if is_selection || is_selectable {
            window.set_cursor_style(CursorStyle::IBeam, &hitbox);
        }

        // link cursor pointer
        let hovered_link =
            Self::link_range_for_position(&text_layout, &self.links, window.mouse_position());
        if hovered_link.is_some() {
            window.set_cursor_style(CursorStyle::PointingHand, &hitbox);
        }

        if let Some(selection) = &state.selection {
            let color = GlobalState::global(cx)
                .text_view_state()
                .map(|state| state.read(cx).text_view_style.selection())
                .unwrap_or_else(|| crate::Theme::global(cx).tokens.colors.selection);
            Self::paint_selection(selection, &text_layout, &bounds, window, color);
            if let Some((start, end)) = Self::selection_edges(selection, &text_layout)
                && let Some(text_view_state) = GlobalState::global(cx).text_view_state()
            {
                text_view_state
                    .read(cx)
                    .selection_adapter
                    .register_selection_edges(start, end);
            }
        }

        if is_selectable {
            if let Some(text_view_state) = GlobalState::global(cx).text_view_state() {
                let text_bounds = Self::text_line_bounds(
                    &text_layout,
                    text_layout.line_height(),
                    window.content_mask().bounds,
                );
                let adapter = &text_view_state.read(cx).selection_adapter;
                adapter.register_inline(text_bounds);
                adapter.register_text_run(crate::TextSelectionRun::new(
                    self.text.clone(),
                    text_layout.clone(),
                    hitbox.bounds,
                ));
            }

            window.on_mouse_event({
                let hitbox = hitbox.clone();
                let text_layout = text_layout.clone();
                let inline_state = self.state.clone();
                let text = self.text.clone();
                let text_view_state = GlobalState::global(cx).text_view_state();
                let line_bounds = self.selection_bounds;
                move |event: &MouseDownEvent, phase, window, cx| {
                    if !phase.bubble()
                        || !hitbox.is_hovered(window)
                        || event.button != MouseButton::Left
                    {
                        return;
                    }

                    if event.click_count == 3
                        && let Some(line_bounds) = line_bounds
                    {
                        GlobalState::suppress_text_selection(cx);
                        if let Some(view) = &text_view_state {
                            view.update(cx, |state, cx| {
                                state.set_multi_click_line(line_bounds, cx)
                            });
                        }
                        cx.notify(current_view);
                        return;
                    }

                    // A finger selects read-only text with a long press only;
                    // a double tap selects nothing here, neither the mouse's
                    // plain word nor the window layer's touch selection. The
                    // handles and the menu on a double tap belong to `Input`.
                    if event.click_count == 2 && GlobalState::is_touch_press(cx) {
                        GlobalState::suppress_text_selection(cx);
                        return;
                    }

                    let kind = match event.click_count {
                        2 => TextViewMultiClickKind::Word,
                        3 => TextViewMultiClickKind::Paragraph,
                        _ => return,
                    };

                    let Some(range) = selection_for_multi_click(
                        &text,
                        &text_layout,
                        hitbox.bounds,
                        event.position,
                        kind,
                    ) else {
                        return;
                    };

                    let selected_text = text[range.clone()].to_string();

                    // This renderer owns multi-click selection. Prevent the
                    // window selection layer from handling the same press.
                    GlobalState::suppress_text_selection(cx);

                    if let Ok(mut inline_state) = inline_state.lock() {
                        inline_state.selection = Some(range.into());
                    }
                    if let Some(text_view_state) = &text_view_state {
                        text_view_state.update(cx, |state, cx| {
                            state.set_multi_click_selection(
                                event.position,
                                kind,
                                selected_text,
                                cx,
                            );
                        });
                    }
                    cx.notify(current_view);
                }
            });
        }

        // Mouse move: repaint only when the pointer enters, leaves or moves
        // between links, so the link cursor follows it. Hovering plain text
        // changes nothing painted.
        if !self.links.is_empty() {
            window.on_mouse_event({
                let hitbox = hitbox.clone();
                let text_layout = text_layout.clone();
                let links = self.links.clone();
                let mut hovered_link = hovered_link;
                move |event: &MouseMoveEvent, phase, window, cx| {
                    if !phase.bubble() || !hitbox.is_hovered(window) {
                        return;
                    }

                    let updated =
                        Self::link_range_for_position(&text_layout, &links, event.position);
                    if hovered_link != updated {
                        hovered_link = updated;
                        cx.notify(current_view);
                    }
                }
            });
        }

        if !is_selection {
            // click to open link
            window.on_mouse_event({
                let links = self.links.clone();
                let text_layout = text_layout.clone();
                let hitbox = hitbox.clone();
                let text_view_state = GlobalState::global(cx).text_view_state();
                let link_click_handler = self.link_click_handler.clone();

                move |event: &MouseUpEvent, phase, window, cx| {
                    if !phase.bubble() || !hitbox.is_hovered(window) {
                        return;
                    }
                    if text_view_state
                        .as_ref()
                        .is_some_and(|state| state.read(cx).has_selection(cx))
                    {
                        return;
                    }

                    if let Some(link) =
                        Self::link_for_position(&text_layout, &links, event.position)
                    {
                        TextSelection::end(window, cx);
                        cx.stop_propagation();
                        let click = ClickEvent::Mouse(MouseClickEvent {
                            down: MouseDownEvent {
                                button: event.button,
                                position: event.position,
                                modifiers: event.modifiers,
                                click_count: event.click_count,
                                first_mouse: false,
                            },
                            up: event.clone(),
                        });
                        handle_link_click(&link_click_handler, link.url, click, window, cx);
                    }
                }
            });
        }

        drop(state);
        self.retain_styled_text();
    }
}

#[derive(Clone, Debug, PartialEq)]
struct GlyphBox {
    text: Range<usize>,
    row: usize,
    left: Pixels,
    right: Pixels,
}

fn glyph_boxes(text_layout: &TextLayout, align: TextAlign, align_width: Pixels) -> Vec<GlyphBox> {
    let mut boxes = Vec::new();
    let mut row = 0;
    let mut line_start = 0;
    for line in text_layout.line_layouts() {
        let layout = &line.unwrapped_layout;
        let glyphs = layout
            .runs
            .iter()
            .flat_map(|run| run.glyphs.iter())
            .collect::<Vec<_>>();
        // Each row starts at a wrap boundary glyph, and ends where the next
        // row starts, or at the end of the line.
        let run_offsets = layout
            .runs
            .iter()
            .scan(0, |offset, run| {
                let start = *offset;
                *offset += run.glyphs.len();
                Some(start)
            })
            .collect::<Vec<_>>();
        let row_starts = line
            .wrap_boundaries
            .iter()
            .map(|boundary| run_offsets[boundary.run_ix] + boundary.glyph_ix)
            .collect::<Vec<_>>();
        let line_boxes = boxes.len();
        let mut from = 0;
        for (row_in_line, to) in row_starts.iter().copied().chain([glyphs.len()]).enumerate() {
            let start_x = if row_in_line == 0 {
                Pixels::ZERO
            } else {
                glyphs[from].position.x
            };
            let end_x = glyphs
                .get(to)
                .map_or(layout.width, |glyph| glyph.position.x);
            let shift = aligned_row_left(align, align_width, end_x - start_x) - start_x;
            for ix in from..to {
                let glyph = glyphs[ix];
                let right = if ix + 1 < to {
                    glyphs[ix + 1].position.x
                } else {
                    end_x
                };
                boxes.push(GlyphBox {
                    text: line_start + glyph.index..line_start + glyph.index,
                    row: row + row_in_line,
                    left: shift + glyph.position.x,
                    right: shift + right,
                });
            }
            from = to;
        }

        // A glyph draws its text up to where the next glyph's text starts.
        let line_boxes = &mut boxes[line_boxes..];
        line_boxes.sort_by_key(|glyph| glyph.text.start);
        let line_end = line_start + line.len();
        for ix in 0..line_boxes.len() {
            let start = line_boxes[ix].text.start;
            line_boxes[ix].text.end = line_boxes[ix + 1..]
                .iter()
                .map(|glyph| glyph.text.start)
                .find(|next| *next > start)
                .unwrap_or(line_end);
        }

        row += line.wrap_boundaries.len() + 1;
        line_start = line_end + 1;
    }
    boxes
}

fn aligned_row_left(align: TextAlign, align_width: Pixels, width: Pixels) -> Pixels {
    match align {
        TextAlign::Left => Pixels::ZERO,
        TextAlign::Center => (align_width - width) / 2.,
        TextAlign::Right => align_width - width,
    }
}

fn range_boxes(glyphs: &[GlyphBox], range: Range<usize>) -> Vec<(usize, Pixels, Pixels)> {
    let first = glyphs.partition_point(|glyph| glyph.text.end <= range.start);
    let mut hits = glyphs[first..]
        .iter()
        .take_while(|glyph| glyph.text.start < range.end)
        .map(|glyph| (glyph.row, glyph.left, glyph.right))
        .collect::<Vec<_>>();
    hits.sort_by(|a, b| {
        (a.0, a.1)
            .partial_cmp(&(b.0, b.1))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut boxes: Vec<(usize, Pixels, Pixels)> = Vec::with_capacity(hits.len());
    for (row, left, right) in hits {
        match boxes.last_mut() {
            Some(last) if last.0 == row && left <= last.2 => last.2 = last.2.max(right),
            _ => boxes.push((row, left, right)),
        }
    }
    boxes
}

fn selection_for_multi_click(
    text: &str,
    text_layout: &TextLayout,
    bounds: Bounds<Pixels>,
    pos: Point<Pixels>,
    kind: TextViewMultiClickKind,
) -> Option<std::ops::Range<usize>> {
    if !bounds.contains(&pos) {
        return None;
    }

    let offset = text_layout.index_for_position(pos).ok()?;

    match kind {
        TextViewMultiClickKind::Word => word_range_at(text, offset),
        // Known limitation: a paragraph maps to a single Inline run here. When a
        // paragraph embeds an inline image it is split into multiple Inline runs,
        // so triple-click only selects the run on the clicked side of the image.
        TextViewMultiClickKind::Paragraph | TextViewMultiClickKind::Line => {
            (!text.is_empty()).then_some(0..text.len())
        }
    }
}

pub(super) fn point_in_text_selection(
    pos: Point<Pixels>,
    char_width: Pixels,
    selection_start: Point<Pixels>,
    selection_end: Point<Pixels>,
    line_height: Pixels,
) -> bool {
    let point_in_line = |point: Point<Pixels>| point.y >= pos.y && point.y < pos.y + line_height;
    let top = selection_start.y.min(selection_end.y);
    let bottom = selection_start.y.max(selection_end.y);
    let x = pos.x + char_width.half();

    // Out of the vertical bounds
    if pos.y + line_height <= top || pos.y > bottom {
        return false;
    }

    // Treat the selection as single-line when both drag points fall within the
    // same rendered line, even if their y coordinates differ inside that line.
    if point_in_line(selection_start) && point_in_line(selection_end) {
        let left = selection_start.x.min(selection_end.x);
        let right = selection_start.x.max(selection_end.x);
        return x >= left && x <= right;
    }

    let (top_point, bottom_point) = if selection_start.y < selection_end.y {
        (selection_start, selection_end)
    } else {
        (selection_end, selection_start)
    };
    let is_top_line = point_in_line(top_point);
    let is_bottom_line = point_in_line(bottom_point);

    if is_top_line {
        return x >= top_point.x;
    } else if is_bottom_line {
        return x <= bottom_point.x;
    } else {
        return true;
    }
}
