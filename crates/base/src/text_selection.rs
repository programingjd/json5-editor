use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    ops::Range,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};

use gpui::{
    App, AppContext as _, Bounds, Context, Element, ElementId, Entity, EntityId, EventEmitter,
    Global, GlobalElementId, Half, Hitbox, HitboxBehavior, Hsla, InputEvent as _,
    InspectorElementId, IntoElement, LayoutId, LongPressEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, ScrollDelta, ScrollWheelEvent, SharedString,
    Style, Subscription, TextLayout, TouchDragEvent, TouchPhase, WeakEntity, Window, point, px,
};

use crate::text_boundary::{line_range_at, word_range_at};
use crate::touch_selection::{
    EdgeDrag, SelectionEdge, TouchHandle, TouchSelectionSnapshot, caret_in_view,
};
use crate::{AutoScroll, GlobalState};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TextSelectionScopeId(u64);

impl TextSelectionScopeId {
    pub fn new() -> Self {
        static NEXT_SCOPE_ID: AtomicU64 = AtomicU64::new(1);
        let value = NEXT_SCOPE_ID
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .expect("text selection scope identifiers exhausted");
        Self(value)
    }

    #[cfg(test)]
    const fn from_raw(value: u64) -> Self {
        Self(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextSelectionContentKey(u64);

impl TextSelectionContentKey {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextSelectionEndpoint {
    entity_id: Option<EntityId>,
    point: Point<Pixels>,
    content_key: Option<TextSelectionContentKey>,
}

impl TextSelectionEndpoint {
    pub(crate) const fn new(entity_id: Option<EntityId>, point: Point<Pixels>) -> Self {
        Self {
            entity_id,
            point,
            content_key: None,
        }
    }

    pub(crate) const fn with_content_key(mut self, content_key: TextSelectionContentKey) -> Self {
        self.content_key = Some(content_key);
        self
    }

    pub const fn entity_id(&self) -> Option<EntityId> {
        self.entity_id
    }

    pub const fn content_point(&self) -> Point<Pixels> {
        self.point
    }

    pub const fn content_key(&self) -> Option<TextSelectionContentKey> {
        self.content_key
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextSelectionWindowPoints {
    anchor: Point<Pixels>,
    cursor: Point<Pixels>,
}

impl TextSelectionWindowPoints {
    pub const fn anchor(&self) -> Point<Pixels> {
        self.anchor
    }

    pub const fn cursor(&self) -> Point<Pixels> {
        self.cursor
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextSelectionSnapshot {
    anchor: TextSelectionEndpoint,
    cursor: TextSelectionEndpoint,
    is_selecting: bool,
    window_points: Option<TextSelectionWindowPoints>,
    coverage: TextSelectionCoverage,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextSelectionCoverage {
    #[default]
    Bounded,

    FromStart,

    ToEnd,

    Full,
}

impl TextSelectionSnapshot {
    pub(crate) const fn new(anchor: TextSelectionEndpoint, cursor: TextSelectionEndpoint) -> Self {
        Self {
            anchor,
            cursor,
            is_selecting: false,
            window_points: None,
            coverage: TextSelectionCoverage::Bounded,
        }
    }

    pub(crate) const fn with_selecting(mut self, is_selecting: bool) -> Self {
        self.is_selecting = is_selecting;
        self
    }

    pub(crate) const fn with_window_points(
        mut self,
        window_points: Option<TextSelectionWindowPoints>,
    ) -> Self {
        self.window_points = window_points;
        self
    }

    #[cfg(test)]
    pub(crate) const fn with_coverage(mut self, coverage: TextSelectionCoverage) -> Self {
        self.coverage = coverage;
        self
    }

    pub const fn anchor(&self) -> TextSelectionEndpoint {
        self.anchor
    }

    pub const fn cursor(&self) -> TextSelectionEndpoint {
        self.cursor
    }

    pub const fn is_selecting(&self) -> bool {
        self.is_selecting
    }

    pub const fn window_points(&self) -> Option<TextSelectionWindowPoints> {
        self.window_points
    }

    pub const fn coverage(&self) -> TextSelectionCoverage {
        self.coverage
    }
}

struct RenderedMarker;

pub struct TextSelectionRegistration {
    hitbox: Hitbox,
    bounds: Bounds<Pixels>,
    scroll_offset: Point<Pixels>,
    scope: TextSelectionScopeId,
    document_order: u64,
    text_bounds: Vec<Bounds<Pixels>>,
    self_scroll: bool,
    selection_edges: Option<(Bounds<Pixels>, Bounds<Pixels>)>,
    rendered: Option<WeakEntity<RenderedMarker>>,
}

impl TextSelectionRegistration {
    pub fn new(hitbox: Hitbox, bounds: Bounds<Pixels>) -> Self {
        Self {
            hitbox,
            bounds,
            scroll_offset: Point::default(),
            scope: TextSelectionScopeId::default(),
            document_order: 0,
            text_bounds: Vec::new(),
            self_scroll: false,
            selection_edges: None,
            rendered: None,
        }
    }

    pub fn with_rendered_element(
        mut self,
        participant: &TextSelectionHandle,
        window: &mut Window,
        cx: &mut App,
    ) -> Self {
        self.rendered = Some(
            window
                .use_keyed_state(
                    ElementId::NamedInteger(
                        "text-selection-participant".into(),
                        participant.entity_id().as_u64(),
                    ),
                    cx,
                    |_, _| RenderedMarker,
                )
                .downgrade(),
        );
        self
    }

    fn is_rendered(&self) -> bool {
        self.rendered
            .as_ref()
            .is_some_and(|marker| marker.upgrade().is_some())
    }

    pub(crate) fn with_self_scroll(mut self, self_scroll: bool) -> Self {
        self.self_scroll = self_scroll;
        self
    }

    pub fn with_scroll_offset(mut self, scroll_offset: Point<Pixels>) -> Self {
        self.scroll_offset = scroll_offset;
        self
    }

    pub fn with_scope(mut self, scope: TextSelectionScopeId) -> Self {
        self.scope = scope;
        self
    }

    pub fn with_document_order(mut self, document_order: u64) -> Self {
        self.document_order = document_order;
        self
    }

    pub fn with_text_bounds(mut self, text_bounds: Vec<Bounds<Pixels>>) -> Self {
        self.text_bounds = text_bounds;
        self
    }

    pub fn with_selection_edges(mut self, start: Bounds<Pixels>, end: Bounds<Pixels>) -> Self {
        self.selection_edges = Some((start, end));
        self
    }

    pub fn hitbox(&self) -> &Hitbox {
        &self.hitbox
    }

    pub const fn bounds(&self) -> Bounds<Pixels> {
        self.bounds
    }

    pub const fn scroll_offset(&self) -> Point<Pixels> {
        self.scroll_offset
    }

    pub const fn scope(&self) -> TextSelectionScopeId {
        self.scope
    }

    pub const fn document_order(&self) -> u64 {
        self.document_order
    }

    pub fn text_bounds(&self) -> &[Bounds<Pixels>] {
        &self.text_bounds
    }

    pub const fn selection_edges(&self) -> Option<(Bounds<Pixels>, Bounds<Pixels>)> {
        self.selection_edges
    }
}

#[derive(Clone)]
pub struct TextSelectionRun {
    document_order: u64,

    text: SharedString,

    layout: TextLayout,

    bounds: Bounds<Pixels>,
}

impl TextSelectionRun {
    pub fn new(text: impl Into<SharedString>, layout: TextLayout, bounds: Bounds<Pixels>) -> Self {
        Self {
            document_order: 0,
            text: text.into(),
            layout,
            bounds,
        }
    }

    pub const fn with_document_order(mut self, document_order: u64) -> Self {
        self.document_order = document_order;
        self
    }

    pub const fn document_order(&self) -> u64 {
        self.document_order
    }

    pub fn text(&self) -> &SharedString {
        &self.text
    }

    pub fn layout(&self) -> &TextLayout {
        &self.layout
    }

    pub const fn bounds(&self) -> Bounds<Pixels> {
        self.bounds
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextSelectionProjection {
    ranges: Vec<Option<Range<usize>>>,

    is_active: bool,
}

impl TextSelectionProjection {
    pub fn ranges(&self) -> &[Option<Range<usize>>] {
        &self.ranges
    }

    pub const fn is_active(&self) -> bool {
        self.is_active
    }
}

fn project_ranges(
    snapshot: Option<TextSelectionSnapshot>,
    runs: &[TextSelectionRun],
) -> TextSelectionProjection {
    let Some(snapshot) = snapshot else {
        return TextSelectionProjection {
            ranges: vec![None; runs.len()],
            is_active: false,
        };
    };
    let Some(window_points) = snapshot.window_points() else {
        return TextSelectionProjection {
            ranges: vec![None; runs.len()],
            is_active: true,
        };
    };

    TextSelectionProjection {
        ranges: runs
            .iter()
            .map(|run| selection_range_for_run(run, window_points.anchor, window_points.cursor))
            .collect(),
        is_active: true,
    }
}

fn selection_range_for_run(
    run: &TextSelectionRun,
    selection_start: Point<Pixels>,
    selection_end: Point<Pixels>,
) -> Option<Range<usize>> {
    if run.text.len() != run.layout.len() {
        return None;
    }

    if run.text.is_empty() {
        return None;
    }

    let line_height = run.layout.line_height();
    // Each character is tested with its row's top and height, so a run whose
    // rows all miss the band, or all lie strictly inside it with no endpoint
    // on any row, has the same answer for every character. Decide those
    // without the walk below, which scans the layout twice per character.
    let (rows_top, rows_bottom) = text_rows_extent(&run.layout, line_height);
    let band_top = selection_start.y.min(selection_end.y);
    let band_bottom = selection_start.y.max(selection_end.y);
    if rows_bottom <= band_top || rows_top > band_bottom {
        return None;
    }
    if band_top < rows_top && band_bottom >= rows_bottom {
        return Some(0..run.text.len());
    }

    let mut range = None;
    for (offset, character) in run.text.char_indices() {
        let next_offset = offset + character.len_utf8();
        let Some(position) = run.layout.position_for_index(offset) else {
            continue;
        };

        let char_width = run
            .layout
            .position_for_index(next_offset)
            .filter(|next| next.y == position.y)
            .map_or_else(|| line_height.half(), |next| next.x - position.x);

        if point_in_selection_band(
            position,
            char_width,
            selection_start,
            selection_end,
            line_height,
        ) {
            range.get_or_insert(offset..offset).end = next_offset;
        }
    }
    range
}

pub(crate) fn text_rows_extent(text_layout: &TextLayout, line_height: Pixels) -> (Pixels, Pixels) {
    let top = text_layout.bounds().top();
    let layout_line_height = text_layout.line_height();
    let lines = text_layout.line_layouts();
    let mut last_line_top = top;
    for line in lines.iter().take(lines.len().saturating_sub(1)) {
        last_line_top += line.size(layout_line_height).height;
    }
    let last_row_top = lines.last().map_or(top, |line| {
        last_line_top + line.wrap_boundaries.len() as f32 * layout_line_height
    });
    (top, last_row_top + line_height)
}

fn points_for_multi_click(
    runs: &[TextSelectionRun],
    position: Point<Pixels>,
    click_count: usize,
) -> Option<(Point<Pixels>, Point<Pixels>)> {
    let run = runs.iter().find(|run| run.bounds.contains(&position))?;
    if run.text.len() != run.layout.len() {
        return None;
    }
    let offset = run.layout.index_for_position(position).ok()?;
    let range = match click_count {
        2 => word_range_at(&run.text, offset)?,
        3.. => line_range_at(&run.text, offset),
        _ => return None,
    };
    if range.is_empty() {
        return None;
    }
    Some((
        run.layout.position_for_index(range.start)?,
        run.layout.position_for_index(range.end)?,
    ))
}

fn point_in_selection_band(
    position: Point<Pixels>,
    char_width: Pixels,
    selection_start: Point<Pixels>,
    selection_end: Point<Pixels>,
    line_height: Pixels,
) -> bool {
    let point_in_line =
        |point: Point<Pixels>| point.y >= position.y && point.y < position.y + line_height;
    let top = selection_start.y.min(selection_end.y);
    let bottom = selection_start.y.max(selection_end.y);
    let x = position.x + char_width.half();

    if position.y + line_height <= top || position.y > bottom {
        return false;
    }

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
    if point_in_line(top_point) {
        x >= top_point.x
    } else if point_in_line(bottom_point) {
        x <= bottom_point.x
    } else {
        true
    }
}

type FocusCallback = Rc<dyn Fn(&mut Window, &mut App)>;
type ClearHandler = Rc<dyn Fn(&mut App)>;
type CopyCallback = Rc<dyn Fn(&mut App) -> String>;
type ContentKeyResolver = Rc<dyn Fn(Point<Pixels>, &App) -> Option<TextSelectionContentKey>>;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TextSelectionEvent {
    SelectionChanged(Option<TextSelectionSnapshot>),

    AutoScroll(Option<Pixels>),

    Cleared,

    TouchSelectionChanged,
}

struct CopyItem {
    document_order: u64,
    callback: Option<CopyCallback>,
    fallback: String,
}

fn resolve_copy_items(mut items: Vec<CopyItem>, cx: &mut App) -> String {
    items.sort_by_key(|item| item.document_order);
    items
        .into_iter()
        .map(|item| {
            item.callback
                .map(|callback| callback(cx))
                .unwrap_or(item.fallback)
        })
        .filter(|text| !text.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn dispatch_clear_handlers(handlers: Vec<ClearHandler>, cx: &mut App) {
    for handler in handlers {
        handler(cx);
    }
}

struct SelectableTextState {
    fallback_copy_text: String,

    projected_copy_text: RefCell<Option<String>>,

    runs: RefCell<Vec<TextSelectionRun>>,
    local_selection: bool,
    snapshot: Option<TextSelectionSnapshot>,
    on_focus: Option<FocusCallback>,
    clear: Option<ClearHandler>,
    copy: Option<CopyCallback>,
    content_key_resolver: Option<ContentKeyResolver>,
}

impl EventEmitter<TextSelectionEvent> for SelectableTextState {}

impl SelectableTextState {
    fn new(fallback_copy_text: impl Into<String>) -> Self {
        Self {
            fallback_copy_text: fallback_copy_text.into(),
            projected_copy_text: RefCell::new(None),
            runs: RefCell::new(Vec::new()),
            local_selection: false,
            snapshot: None,
            on_focus: None,
            clear: None,
            copy: None,
            content_key_resolver: None,
        }
    }

    fn snapshot(&self) -> Option<TextSelectionSnapshot> {
        self.snapshot
    }

    fn set_fallback_copy_text(&mut self, text: impl Into<String>) {
        self.fallback_copy_text = text.into();
        *self.projected_copy_text.get_mut() = None;
    }

    fn set_local_selection(&mut self, active: bool) {
        self.local_selection = active;
    }

    fn update_runs(&self, runs: &[TextSelectionRun]) -> TextSelectionProjection {
        *self.runs.borrow_mut() = runs.to_vec();
        let states = project_ranges(self.snapshot, runs);
        let mut selected_runs = runs
            .iter()
            .zip(states.ranges())
            .enumerate()
            .filter_map(|(index, (run, state))| {
                state.as_ref().map(|range| {
                    debug_assert!(run.text.is_char_boundary(range.start));
                    debug_assert!(run.text.is_char_boundary(range.end));
                    (
                        run.document_order,
                        index,
                        run.text[range.clone()].to_string(),
                    )
                })
            })
            .collect::<Vec<_>>();
        selected_runs.sort_by_key(|(order, index, _)| (*order, *index));
        *self.projected_copy_text.borrow_mut() =
            Some(selected_runs.into_iter().map(|(_, _, text)| text).collect());
        states
    }

    fn set_focus_handler(&mut self, callback: impl Fn(&mut Window, &mut App) + 'static) {
        self.on_focus = Some(Rc::new(callback));
    }

    fn clear_with(&mut self, callback: impl Fn(&mut App) + 'static) {
        self.clear = Some(Rc::new(callback));
    }

    fn copy_with(&mut self, callback: impl Fn(&mut App) -> String + 'static) {
        self.copy = Some(Rc::new(callback));
    }

    fn resolve_content_key_with(
        &mut self,
        callback: impl Fn(Point<Pixels>, &App) -> Option<TextSelectionContentKey> + 'static,
    ) {
        self.content_key_resolver = Some(Rc::new(callback));
    }

    fn set_snapshot(&mut self, snapshot: Option<TextSelectionSnapshot>, cx: &mut Context<Self>) {
        if self.snapshot == snapshot {
            return;
        }
        self.snapshot = snapshot;
        *self.projected_copy_text.get_mut() = None;
        cx.emit(TextSelectionEvent::SelectionChanged(snapshot));
    }

    fn has_selection_to_clear(&self) -> bool {
        self.snapshot.is_some() || self.local_selection
    }

    fn clear_state(&mut self, cx: &mut Context<Self>) -> Option<ClearHandler> {
        self.snapshot = None;
        *self.projected_copy_text.get_mut() = None;
        self.local_selection = false;
        cx.emit(TextSelectionEvent::Cleared);
        cx.emit(TextSelectionEvent::SelectionChanged(None));
        self.clear.clone()
    }

    fn set_auto_scroll(&self, delta: Option<Pixels>, cx: &mut Context<Self>) {
        cx.emit(TextSelectionEvent::AutoScroll(delta));
    }

    fn focus(&self, window: &mut Window, cx: &mut App) {
        if let Some(callback) = self.on_focus.clone() {
            window.defer(cx, move |window, cx| callback(window, cx));
        }
    }

    fn copy_item(&self, document_order: u64) -> Option<CopyItem> {
        (self.snapshot.is_some() || self.local_selection).then(|| CopyItem {
            document_order,
            callback: self.copy.clone(),
            fallback: self
                .projected_copy_text
                .borrow()
                .clone()
                .unwrap_or_else(|| self.fallback_copy_text.clone()),
        })
    }
}

#[derive(Default)]
pub struct TouchHandleLayout {
    hitboxes: Vec<(SelectionEdge, Hitbox)>,
}

#[derive(Clone)]
pub struct TextSelectionHandle(Entity<SelectableTextState>);

impl TextSelectionHandle {
    pub fn new(fallback_copy_text: impl Into<String>, cx: &mut App) -> Self {
        Self(cx.new(|_| SelectableTextState::new(fallback_copy_text)))
    }

    pub fn entity_id(&self) -> EntityId {
        self.0.entity_id()
    }

    pub fn snapshot(&self, cx: &App) -> Option<TextSelectionSnapshot> {
        self.0.read(cx).snapshot()
    }

    pub fn set_fallback_copy_text(&self, text: impl Into<String>, cx: &mut App) {
        self.0
            .update(cx, |state, _| state.set_fallback_copy_text(text));
    }

    pub fn set_local_selection(&self, active: bool, cx: &mut App) {
        self.0
            .update(cx, |state, _| state.set_local_selection(active));
    }

    pub fn has_local_selection(&self, cx: &App) -> bool {
        self.0.read(cx).local_selection
    }

    pub fn register(
        &self,
        mut registration: TextSelectionRegistration,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(scope) = current_text_selection_scope(window.window_handle().window_id(), cx) {
            registration.scope = scope;
        }
        let Some(state) = WindowSelectionState::existing(window, cx) else {
            return;
        };
        let Some(registration) = state.read(cx).register_quietly(self, registration) else {
            return;
        };
        state.update(cx, |state, cx| {
            state.register_participant(self.clone(), registration, cx)
        });
    }

    pub fn update_runs(&self, runs: &[TextSelectionRun], cx: &mut App) -> TextSelectionProjection {
        self.0.read(cx).update_runs(runs)
    }

    // Rich text renders its own selection; retain geometry only for word hit testing.
    pub(crate) fn set_hit_test_runs(&self, runs: &[TextSelectionRun], cx: &mut App) {
        *self.0.read(cx).runs.borrow_mut() = runs.to_vec();
    }

    pub fn subscribe(
        &self,
        mut callback: impl FnMut(&TextSelectionEvent, &mut App) + 'static,
        cx: &mut App,
    ) -> Subscription {
        cx.subscribe(&self.0, move |_, event, cx| callback(event, cx))
    }

    #[must_use = "retain the subscription or explicitly detach it"]
    pub fn refresh_window_on_change(&self, window: &Window, cx: &mut App) -> Subscription {
        let window = window.window_handle();
        self.subscribe(
            move |event, cx| {
                if matches!(event, TextSelectionEvent::SelectionChanged(_)) {
                    _ = window.update(cx, |_, window, _| window.refresh());
                }
            },
            cx,
        )
    }

    pub fn focus_with(&self, callback: impl Fn(&mut Window, &mut App) + 'static, cx: &mut App) {
        self.0
            .update(cx, |state, _| state.set_focus_handler(callback));
    }

    pub fn clear_with(&self, callback: impl Fn(&mut App) + 'static, cx: &mut App) {
        self.0.update(cx, |state, _| state.clear_with(callback));
    }

    pub fn copy_with(&self, callback: impl Fn(&mut App) -> String + 'static, cx: &mut App) {
        self.0.update(cx, |state, _| state.copy_with(callback));
    }

    pub fn prepaint_touch_handles(&self, window: &mut Window, cx: &App) -> TouchHandleLayout {
        let Some(state) = WindowSelectionState::existing(window, cx) else {
            return TouchHandleLayout::default();
        };
        let hitboxes = state
            .read(cx)
            .touch_handles_of(self.entity_id())
            .into_iter()
            .map(|(edge, caret)| {
                let hitbox = window.insert_hitbox(
                    TouchHandle::hit_bounds(edge, caret),
                    HitboxBehavior::BlockMouse,
                );
                (edge, hitbox)
            })
            .collect();
        TouchHandleLayout { hitboxes }
    }

    pub fn paint_touch_handles(
        &self,
        layout: &TouchHandleLayout,
        color: Hsla,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some(state) = WindowSelectionState::existing(window, cx) else {
            return;
        };
        for (edge, caret) in state.read(cx).touch_handles_of(self.entity_id()) {
            TouchHandle::paint(edge, caret, color, window);
        }
        let bounds: Vec<Bounds<Pixels>> = layout
            .hitboxes
            .iter()
            .map(|(_, hitbox)| hitbox.bounds)
            .collect();
        if !state.read(cx).handles_painted_at(self.entity_id(), &bounds) {
            state.update(cx, |state, _| {
                state.register_handles(self.entity_id(), bounds)
            });
        }
        for (edge, hitbox) in &layout.hitboxes {
            let edge = *edge;
            // Touch: the drag is offered on the first touch, before it can
            // become a tap, a long press or a pan.
            window.on_mouse_event({
                let hitbox = hitbox.clone();
                let state = state.downgrade();
                move |event: &TouchDragEvent, phase, window, cx| {
                    if !phase.bubble()
                        || event.phase != TouchPhase::Started
                        || window.default_prevented()
                        || !hitbox.is_hovered(window)
                    {
                        return;
                    }
                    let Some(state) = state.upgrade() else {
                        return;
                    };
                    window.prevent_default();
                    cx.stop_propagation();
                    state.update(cx, |state, cx| {
                        state.begin_edge_drag(edge, event.position, window, cx)
                    });
                    WindowSelectionState::resolve_content_keys(&state, cx);
                }
            });
            // Mouse: the same drag for a pointer.
            window.on_mouse_event({
                let hitbox = hitbox.clone();
                let state = state.downgrade();
                move |event: &MouseDownEvent, phase, window, cx| {
                    if !phase.bubble()
                        || event.button != MouseButton::Left
                        || !hitbox.is_hovered(window)
                    {
                        return;
                    }
                    let Some(state) = state.upgrade() else {
                        return;
                    };
                    cx.stop_propagation();
                    state.update(cx, |state, cx| {
                        state.begin_edge_drag(edge, event.position, window, cx)
                    });
                    WindowSelectionState::resolve_content_keys(&state, cx);
                }
            });
        }
    }

    pub fn resolve_content_key_with(
        &self,
        callback: impl Fn(Point<Pixels>, &App) -> Option<TextSelectionContentKey> + 'static,
        cx: &mut App,
    ) {
        self.0
            .update(cx, |state, _| state.resolve_content_key_with(callback));
    }

    fn downgrade(&self) -> WeakEntity<SelectableTextState> {
        self.0.downgrade()
    }
}

#[derive(Clone)]
struct ParticipantRegistration {
    participant: WeakEntity<SelectableTextState>,
    registration: Rc<TextSelectionRegistration>,
    generation: u64,
}

#[derive(Clone)]
struct SelectionEndpoint {
    participant: Option<WeakEntity<SelectableTextState>>,
    point: Point<Pixels>,
    inside: bool,
    inside_text: bool,
    content_key: Option<TextSelectionContentKey>,
    content_key_resolver: Option<(ContentKeyResolver, Point<Pixels>)>,
}

impl SelectionEndpoint {
    fn snapshot(&self) -> TextSelectionEndpoint {
        let snapshot = TextSelectionEndpoint::new(self.entity_id(), self.point);
        if let Some(content_key) = self.content_key {
            snapshot.with_content_key(content_key)
        } else {
            snapshot
        }
    }

    fn resolve(
        &self,
        participants: &HashMap<EntityId, ParticipantRegistration>,
    ) -> Option<Point<Pixels>> {
        let participant = self.participant.as_ref()?;
        let registration = participants.get(&participant.entity_id())?;
        participant.upgrade()?;
        Some(
            self.point
                + registration.registration.scroll_offset
                + registration.registration.bounds.origin,
        )
    }

    fn entity_id(&self) -> Option<EntityId> {
        self.participant
            .as_ref()
            .map(|participant| participant.entity_id())
    }
}

#[derive(Default)]
struct TouchSelection {
    active: bool,
    menu_open: bool,
    drag: Option<EdgeDrag>,

    ui_bounds: Vec<Bounds<Pixels>>,
    previous_ui_bounds: Vec<Bounds<Pixels>>,

    handle_bounds: HashMap<EntityId, Vec<Bounds<Pixels>>>,

    last_edges: std::cell::Cell<Option<(Bounds<Pixels>, Bounds<Pixels>, bool, bool)>>,
}

impl TouchSelection {
    fn covers(&self, position: Point<Pixels>) -> bool {
        self.active
            && self
                .ui_bounds
                .iter()
                .chain(&self.previous_ui_bounds)
                .chain(self.handle_bounds.values().flatten())
                .any(|bounds| bounds.contains(&position))
    }

    fn painted_any(&self) -> bool {
        !self.ui_bounds.is_empty() || !self.previous_ui_bounds.is_empty()
    }

    fn begin_frame(&mut self) {
        self.previous_ui_bounds = std::mem::take(&mut self.ui_bounds);
    }

    fn reset(&mut self) -> bool {
        let had = self.active;
        self.handle_bounds.clear();
        self.active = false;
        self.menu_open = false;
        self.drag = None;
        self.last_edges.set(None);
        had
    }
}

#[derive(Default)]
struct WindowSelectionState {
    participants: RefCell<HashMap<EntityId, ParticipantRegistration>>,
    active_scope: TextSelectionScopeId,
    anchor: Option<SelectionEndpoint>,
    cursor: Option<SelectionEndpoint>,
    pending_extension_anchor: Option<SelectionEndpoint>,
    is_selecting: bool,
    did_hit_text: bool,
    frame_generation: Cell<u64>,
    finish_frame_scheduled: Cell<bool>,
    refresh_held_cursor: bool,
    mouse_down_prepared: bool,
    auto_scroll: AutoScroll,

    auto_scroll_stall: (Option<(Bounds<Pixels>, Point<Pixels>)>, u8),
    touch: TouchSelection,

    entity_id: Option<EntityId>,
}

impl WindowSelectionState {
    fn resolve_content_keys(state: &Entity<Self>, cx: &mut App) {
        let pending = state.read_with(cx, |state, _| {
            [
                state
                    .anchor
                    .as_ref()
                    .and_then(|endpoint| endpoint.content_key_resolver.clone()),
                state
                    .cursor
                    .as_ref()
                    .and_then(|endpoint| endpoint.content_key_resolver.clone()),
            ]
        });
        // Nothing to resolve and no selection to publish: an event that
        // changed nothing does not change the state either.
        if pending.iter().all(Option::is_none) && !state.read(cx).is_active() {
            return;
        }
        let resolved =
            pending.map(|pending| pending.and_then(|(callback, point)| callback(point, cx)));
        state.update(cx, |state, cx| {
            if let (Some(endpoint), Some(key)) = (state.anchor.as_mut(), resolved[0]) {
                endpoint.content_key = Some(key);
                endpoint.content_key_resolver = None;
            }
            if let (Some(endpoint), Some(key)) = (state.cursor.as_mut(), resolved[1]) {
                endpoint.content_key = Some(key);
                endpoint.content_key_resolver = None;
            }
            state.publish_snapshots(cx);
        });
    }
    fn acquire(window_id: gpui::WindowId, cx: &mut App) -> Entity<Self> {
        if !cx.has_global::<SelectionStateRegistry>() {
            cx.set_global(SelectionStateRegistry::default());
        }
        if let Some(state) = cx
            .global::<SelectionStateRegistry>()
            .0
            .get(&window_id)
            .and_then(WeakEntity::upgrade)
        {
            return state;
        }

        let active_scope = if cx.has_global::<PendingTextSelectionScopes>() {
            cx.global_mut::<PendingTextSelectionScopes>()
                .0
                .remove(&window_id)
                .unwrap_or_default()
        } else {
            TextSelectionScopeId::default()
        };

        let state = cx.new(move |cx| {
            let entity_id = cx.entity_id();
            cx.on_release(move |state: &mut WindowSelectionState, cx| {
                let handlers = state.clear_state(cx);
                if cx.has_global::<SelectionStateRegistry>() {
                    let registry = &mut cx.global_mut::<SelectionStateRegistry>().0;
                    if registry
                        .get(&window_id)
                        .is_some_and(|state| state.entity_id() == entity_id)
                    {
                        registry.remove(&window_id);
                    }
                }
                if !handlers.is_empty() {
                    cx.defer(move |cx| dispatch_clear_handlers(handlers, cx));
                }
            })
            .detach();
            Self {
                active_scope,
                entity_id: Some(entity_id),
                ..Self::default()
            }
        });
        cx.global_mut::<SelectionStateRegistry>()
            .0
            .insert(window_id, state.downgrade());
        state
    }

    #[cfg(test)]
    fn ensure(window: &Window, cx: &mut App) -> Entity<Self> {
        Self::acquire(window.window_handle().window_id(), cx)
    }

    fn existing(window: &Window, cx: &App) -> Option<Entity<Self>> {
        if !cx.has_global::<SelectionStateRegistry>() {
            return None;
        }
        cx.global::<SelectionStateRegistry>()
            .0
            .get(&window.window_handle().window_id())
            .and_then(WeakEntity::upgrade)
    }

    #[cfg(test)]
    fn set_active_scope(&mut self, scope: TextSelectionScopeId, cx: &mut App) {
        let handlers = self.set_active_scope_state(scope, cx);
        dispatch_clear_handlers(handlers, cx);
    }

    fn set_active_scope_state(
        &mut self,
        scope: TextSelectionScopeId,
        cx: &mut App,
    ) -> Vec<ClearHandler> {
        if self.active_scope == scope {
            return Vec::new();
        }
        let handlers = self.clear_state(cx);
        self.active_scope = scope;
        self.publish_snapshots(cx);
        handlers
    }

    pub fn finish_frame(&mut self, cx: &mut App) -> Vec<ClearHandler> {
        self.finish_frame_scheduled.set(false);
        let generation = self.frame_generation.get();
        let stale = self
            .participants
            .get_mut()
            .iter()
            .filter_map(|(id, registration)| {
                (registration.generation != generation && !registration.registration.is_rendered())
                    .then(|| (*id, registration.participant.clone()))
            })
            .collect::<Vec<_>>();
        let mut handlers = Vec::new();
        for (id, participant) in stale {
            self.participants.get_mut().remove(&id);
            if let Some(participant) = participant.upgrade()
                && participant.read(cx).has_selection_to_clear()
            {
                if let Some(handler) = participant.update(cx, |state, cx| state.clear_state(cx)) {
                    handlers.push(handler);
                }
            }
        }
        self.publish_snapshots(cx);
        self.frame_generation.set(generation.wrapping_add(1));
        handlers
    }

    fn schedule_finish_frame(&self) -> bool {
        !self.finish_frame_scheduled.replace(true)
    }

    fn is_active(&self) -> bool {
        self.anchor.is_some()
            || self.cursor.is_some()
            || self.pending_extension_anchor.is_some()
            || self.is_selecting
            || self.refresh_held_cursor
            || self.touch.active
    }

    fn register_quietly(
        &self,
        selection: &TextSelectionHandle,
        registration: TextSelectionRegistration,
    ) -> Option<TextSelectionRegistration> {
        if self.is_active() {
            return Some(registration);
        }
        self.participants.borrow_mut().insert(
            selection.entity_id(),
            ParticipantRegistration {
                participant: selection.downgrade(),
                registration: Rc::new(registration),
                generation: self.frame_generation.get(),
            },
        );
        None
    }

    fn finish_frame_quietly(&self, cx: &App) -> bool {
        if self.is_active() {
            return false;
        }
        let generation = self.frame_generation.get();
        let mut participants = self.participants.borrow_mut();
        let gone = |registration: &ParticipantRegistration| {
            registration.generation != generation && !registration.registration.is_rendered()
        };
        if participants.values().any(|registration| {
            gone(registration)
                && registration
                    .participant
                    .upgrade()
                    .is_some_and(|participant| participant.read(cx).has_selection_to_clear())
        }) {
            return false;
        }
        participants.retain(|_, registration| {
            !gone(registration) && registration.participant.upgrade().is_some()
        });
        drop(participants);
        self.finish_frame_scheduled.set(false);
        self.frame_generation.set(generation.wrapping_add(1));
        true
    }

    pub fn register_participant(
        &mut self,
        selection: TextSelectionHandle,
        registration: TextSelectionRegistration,
        cx: &mut App,
    ) {
        self.prune_dead_participants();
        if self.is_selecting
            && registration.self_scroll
            && self.anchor.as_ref().and_then(SelectionEndpoint::entity_id)
                == Some(selection.entity_id())
            && self
                .participants
                .get_mut()
                .get(&selection.entity_id())
                .is_some_and(|previous| {
                    previous.registration.scroll_offset != registration.scroll_offset
                        || previous.registration.bounds != registration.bounds
                })
        {
            self.refresh_held_cursor = true;
        }
        // The handles sit on the painted ends; when those moved — a scroll, a
        // reflow, a select-all — whoever draws the handles needs to know.
        let edges_moved = self.touch.active
            && self
                .participants
                .get_mut()
                .get(&selection.entity_id())
                .is_none_or(|previous| {
                    previous.registration.selection_edges != registration.selection_edges
                });
        self.participants.get_mut().insert(
            selection.entity_id(),
            ParticipantRegistration {
                participant: selection.downgrade(),
                registration: Rc::new(registration),
                generation: self.frame_generation.get(),
            },
        );
        self.publish_snapshots(cx);
        if edges_moved {
            self.touch_changed(cx);
        }
    }

    #[cfg(test)]
    fn begin(&mut self, position: Point<Pixels>, extend: bool, cx: &mut App) {
        self.begin_impl(position, extend, false, None, cx);
    }

    #[cfg(test)]
    fn update(&mut self, position: Point<Pixels>, cx: &mut App) {
        self.update_impl(position, None, cx);
    }

    pub fn end(&mut self, cx: &mut App) {
        self.pending_extension_anchor = None;
        if !self.is_selecting {
            return;
        }
        self.is_selecting = false;
        if !self.did_hit_text {
            self.anchor = None;
            self.cursor = None;
        }
        self.stop_anchor_auto_scroll(cx);
        self.publish_snapshots(cx);
    }

    pub fn clear(&mut self, cx: &mut App) {
        let handlers = self.clear_state(cx);
        dispatch_clear_handlers(handlers, cx);
    }

    fn clear_state(&mut self, cx: &mut App) -> Vec<ClearHandler> {
        self.stop_anchor_auto_scroll(cx);
        self.anchor = None;
        self.cursor = None;
        self.pending_extension_anchor = None;
        self.is_selecting = false;
        self.did_hit_text = false;
        if self.touch.reset() {
            self.touch_changed(cx);
        }
        self.prune_dead_participants();
        self.participants
            .borrow()
            .values()
            .filter_map(|registration| registration.participant.upgrade())
            .filter_map(|participant| participant.update(cx, |state, cx| state.clear_state(cx)))
            .collect()
    }

    fn touch_changed(&self, cx: &mut App) {
        let participants = self
            .participants
            .borrow()
            .values()
            .filter_map(|registration| registration.participant.upgrade())
            .filter(|participant| participant.read(cx).snapshot.is_some())
            .collect::<Vec<_>>();
        let entity_id = self.entity_id;
        cx.defer(move |cx| {
            for participant in participants {
                participant.update(cx, |_, cx| {
                    cx.emit(TextSelectionEvent::TouchSelectionChanged)
                });
            }
            if let Some(entity_id) = entity_id {
                cx.notify(entity_id);
            }
        });
    }

    fn copy_items(&self, cx: &App) -> Vec<CopyItem> {
        self.participants
            .borrow()
            .values()
            .filter_map(|registration| {
                let participant = registration.participant.upgrade()?;
                participant
                    .read(cx)
                    .copy_item(registration.registration.document_order)
            })
            .collect()
    }

    #[cfg(test)]
    fn selected_text(&self, cx: &mut App) -> String {
        resolve_copy_items(self.copy_items(cx), cx)
    }

    fn selects_text(&self, cx: &App) -> bool {
        self.snapshot().is_some()
            && self.participants.borrow().values().any(|registration| {
                registration
                    .participant
                    .upgrade()
                    .is_some_and(|participant| {
                        let participant = participant.read(cx);
                        project_ranges(participant.snapshot, &participant.runs.borrow())
                            .ranges()
                            .iter()
                            .any(|range| range.as_ref().is_some_and(|range| !range.is_empty()))
                    })
            })
    }

    pub fn has_selection(&self, cx: &App) -> bool {
        self.snapshot().is_some()
            || self.participants.borrow().values().any(|registration| {
                registration
                    .participant
                    .upgrade()
                    .is_some_and(|participant| participant.read(cx).local_selection)
            })
    }

    pub fn snapshot(&self) -> Option<TextSelectionSnapshot> {
        if !self.did_hit_text {
            return None;
        }
        let anchor_endpoint = self.anchor.as_ref()?;
        let cursor_endpoint = self.cursor.as_ref()?;
        let anchor = anchor_endpoint.resolve(&self.participants.borrow())?;
        let cursor = cursor_endpoint.resolve(&self.participants.borrow())?;
        (anchor != cursor).then(|| {
            TextSelectionSnapshot::new(anchor_endpoint.snapshot(), cursor_endpoint.snapshot())
                .with_selecting(self.is_selecting)
                .with_window_points(Some(TextSelectionWindowPoints { anchor, cursor }))
        })
    }

    #[cfg(test)]
    fn is_selecting(&self) -> bool {
        self.is_selecting
    }

    fn prepare_for_mouse_down(&mut self, extend: bool, cx: &mut App) -> Vec<ClearHandler> {
        let pending_extension_anchor = extend.then(|| self.anchor.clone()).flatten();
        self.stop_anchor_auto_scroll(cx);
        self.anchor = None;
        self.cursor = None;
        self.pending_extension_anchor = None;
        self.is_selecting = false;
        self.did_hit_text = false;
        if self.touch.reset() {
            self.touch_changed(cx);
        }
        self.prune_dead_participants();
        let handlers = self
            .participants
            .borrow()
            .values()
            .filter_map(|registration| registration.participant.upgrade())
            .filter_map(|participant| participant.update(cx, |state, cx| state.clear_state(cx)))
            .collect();
        self.pending_extension_anchor = pending_extension_anchor;
        handlers
    }

    fn touch_selection(&self) -> Option<TouchSelectionSnapshot> {
        if !self.touch.active {
            return None;
        }
        // Each end with its owner's viewport: an end scrolled out of its
        // participant gets no handle.
        let mut start: Option<(u64, Bounds<Pixels>, bool)> = None;
        let mut end: Option<(u64, Bounds<Pixels>, bool)> = None;
        for registration in self.participants.borrow().values() {
            let geometry = &registration.registration;
            if geometry.scope != self.active_scope || registration.participant.upgrade().is_none() {
                continue;
            }
            let Some((edge_start, edge_end)) = geometry.selection_edges else {
                continue;
            };
            let order = geometry.document_order;
            let viewport = geometry.hitbox.bounds;
            if start.is_none_or(|(best, ..)| order < best) {
                start = Some((order, edge_start, caret_in_view(edge_start, viewport)));
            }
            if end.is_none_or(|(best, ..)| order >= best) {
                end = Some((order, edge_end, caret_in_view(edge_end, viewport)));
            }
        }
        let edges = match (start, end) {
            (Some((_, start, start_visible)), Some((_, end, end_visible))) => {
                let edges = (start, end, start_visible, end_visible);
                self.touch.last_edges.set(Some(edges));
                edges
            }
            _ if self.touch.drag.is_some() => self.touch.last_edges.get()?,
            _ => return None,
        };
        let (start, end, start_visible, end_visible) = edges;
        Some(
            TouchSelectionSnapshot::new(start, end)
                .with_edge_visible(SelectionEdge::Start, start_visible)
                .with_edge_visible(SelectionEdge::End, end_visible)
                .with_menu_open(self.touch.menu_open)
                .with_dragging(self.touch.drag.map(|drag| drag.edge())),
        )
    }

    fn touch_handles_of(&self, participant: EntityId) -> Vec<(SelectionEdge, Bounds<Pixels>)> {
        if !self.touch.active {
            return Vec::new();
        }
        let Some(own) = self.participants.borrow().get(&participant).cloned() else {
            return Vec::new();
        };
        let Some((start, end)) = own.registration.selection_edges else {
            return Vec::new();
        };
        if start == end {
            return Vec::new();
        }
        let order = own.registration.document_order;
        let participants = self.participants.borrow();
        let others = participants.iter().filter(|(id, registration)| {
            **id != participant
                && registration.registration.scope == self.active_scope
                && registration.registration.selection_edges.is_some()
                && registration.participant.upgrade().is_some()
        });
        let (mut earlier, mut later) = (false, false);
        for (_, registration) in others {
            let other = registration.registration.document_order;
            earlier |= other < order;
            later |= other > order;
        }
        let viewport = own.registration.hitbox.bounds;
        let mut handles = Vec::with_capacity(2);
        if !earlier && caret_in_view(start, viewport) {
            handles.push((SelectionEdge::Start, start));
        }
        if !later && caret_in_view(end, viewport) {
            handles.push((SelectionEdge::End, end));
        }
        handles
    }

    fn keep_touch_selection(&mut self, cx: &mut App) {
        self.touch.active = self.snapshot().is_some();
        self.touch.menu_open = self.touch.active;
        self.touch.drag = None;
        self.touch_changed(cx);
    }

    fn register_touch_ui(&mut self, bounds: Bounds<Pixels>) {
        self.touch.ui_bounds.push(bounds);
    }

    fn handles_painted_at(&self, participant: EntityId, bounds: &[Bounds<Pixels>]) -> bool {
        self.touch
            .handle_bounds
            .get(&participant)
            .map_or(bounds.is_empty(), |painted| painted == bounds)
    }

    fn register_handles(&mut self, participant: EntityId, bounds: Vec<Bounds<Pixels>>) {
        self.touch.handle_bounds.insert(participant, bounds);
    }

    fn close_edit_menu(&mut self, cx: &mut App) {
        if !self.touch.menu_open {
            return;
        }
        self.touch.menu_open = false;
        self.touch_changed(cx);
    }

    fn edit_menu_on_scroll(&mut self, phase: TouchPhase, cx: &mut App) {
        if !self.touch.active || self.touch.drag.is_some() {
            return;
        }
        match phase {
            TouchPhase::Ended | TouchPhase::Cancelled => {
                if !self.touch.menu_open {
                    self.touch.menu_open = true;
                    self.touch_changed(cx);
                }
            }
            _ => self.close_edit_menu(cx),
        }
    }

    fn select_all_touched(&mut self, cx: &mut App) {
        if !self.touch.active {
            return;
        }
        let Some((participant, registration)) = self.anchor_registration() else {
            return;
        };
        // From the participant's text runs, not its painted line bounds:
        // those are clipped to the viewport, and a message taller than the
        // screen would only select what is on it.
        let (anchor, cursor) = {
            let runs = &participant.read(cx).runs.borrow();
            let (Some(first), Some(last)) = (
                runs.iter().min_by_key(|run| run.document_order),
                runs.iter().max_by_key(|run| run.document_order),
            ) else {
                return;
            };
            let (Some(start), Some(end)) = (
                first.layout.position_for_index(0),
                last.layout.position_for_index(last.text.len()),
            ) else {
                return;
            };
            // Just inside the first and the last glyph, mid-line, so both
            // land on text.
            let inset = px(1.);
            (
                point(start.x + inset, start.y + first.layout.line_height() / 2.),
                point(end.x - inset, end.y + last.layout.line_height() / 2.),
            )
        };
        let content_key_resolver = participant.read(cx).content_key_resolver.clone();
        let to_endpoint = |window_point: Point<Pixels>| {
            let content_point =
                window_point - registration.bounds.origin - registration.scroll_offset;
            SelectionEndpoint {
                participant: Some(participant.downgrade()),
                point: content_point,
                inside: true,
                inside_text: true,
                content_key: None,
                content_key_resolver: content_key_resolver
                    .clone()
                    .map(|resolver| (resolver, content_point)),
            }
        };
        self.anchor = Some(to_endpoint(anchor));
        self.cursor = Some(to_endpoint(cursor));
        self.did_hit_text = true;
        self.is_selecting = false;
        self.touch.menu_open = true;
        self.publish_snapshots(cx);
        self.touch_changed(cx);
    }

    fn begin_edge_drag(
        &mut self,
        edge: SelectionEdge,
        finger: Point<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some(snapshot) = self.touch_selection() else {
            return;
        };
        let handlers = self.prepare_for_mouse_down(false, cx);
        dispatch_clear_handlers(handlers, cx);
        let held = snapshot.edge(edge.opposite());
        // Just inside the held end, so the anchor lands on the character it
        // marks rather than on the boundary between two participants.
        let nudge = px(1.);
        let anchor_point = match edge {
            SelectionEdge::Start => point(held.left() - nudge, held.center().y),
            SelectionEdge::End => point(held.left() + nudge, held.center().y),
        };
        let anchor = self.endpoint(anchor_point, None, cx);
        let drag = EdgeDrag::begin(edge, snapshot.edge(edge), finger);
        let cursor = self.endpoint(drag.text_position(finger), Some(window), cx);
        self.did_hit_text = anchor.inside_text || cursor.inside_text;
        self.anchor = Some(anchor);
        self.cursor = Some(cursor);
        self.is_selecting = true;
        self.touch.active = true;
        self.touch.menu_open = false;
        self.touch.drag = Some(drag);
        self.publish_snapshots(cx);
        self.touch_changed(cx);
    }

    fn update_edge_drag(&mut self, finger: Point<Pixels>, window: &Window, cx: &mut Context<Self>) {
        let Some(drag) = self.touch.drag else {
            return;
        };
        let before = self.cursor.clone();
        self.update_in_window(drag.text_position(finger), window, cx);
        // A handle never collapses the selection: at the other end it stops,
        // and the finger has to pass that end, onto text, to swap the two.
        if !self.selects_text(cx) {
            self.cursor = before;
            self.publish_snapshots(cx);
            return;
        }
        // Dragging one end past the other swaps them: the cursor now lies
        // before the anchor, so the finger holds what became the start.
        if let Some(points) = self
            .snapshot()
            .and_then(|snapshot| snapshot.window_points())
        {
            let (anchor, cursor) = (points.anchor(), points.cursor());
            let cursor_first = cursor.y < anchor.y || (cursor.y == anchor.y && cursor.x < anchor.x);
            let edge = if cursor_first {
                SelectionEdge::Start
            } else {
                SelectionEdge::End
            };
            if let Some(drag) = self.touch.drag.as_mut() {
                drag.set_edge(edge);
            }
        }
        self.touch_changed(cx);
    }

    fn end_edge_drag(&mut self, cx: &mut App) {
        if self.touch.drag.take().is_none() {
            return;
        }
        self.end(cx);
        self.keep_touch_selection(cx);
    }

    fn begin_in_window(
        &mut self,
        position: Point<Pixels>,
        extend: bool,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.begin_impl(position, extend, true, Some(window), cx);
    }

    fn update_in_window(
        &mut self,
        position: Point<Pixels>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        if !cx.has_active_drag() {
            self.update_impl(position, Some(window), cx);
            self.update_auto_scroll(position, window, cx);
        }
    }

    fn select_at(
        &mut self,
        position: Point<Pixels>,
        click_count: usize,
        window: &mut Window,
        cx: &mut App,
    ) {
        GlobalState::init(cx);
        if GlobalState::is_text_selection_suppressed(cx) {
            return;
        }
        let hit = self.endpoint(position, Some(window), cx);
        if !hit.inside_text {
            return;
        }
        let Some(participant) = hit
            .participant
            .and_then(|participant| participant.upgrade())
        else {
            return;
        };
        let points =
            points_for_multi_click(&participant.read(cx).runs.borrow(), position, click_count);
        let Some((anchor, cursor)) = points else {
            return;
        };
        let Some(registration) = self
            .participants
            .borrow()
            .get(&participant.entity_id())
            .cloned()
        else {
            return;
        };
        let content_key_resolver = participant.read(cx).content_key_resolver.clone();
        let to_endpoint = |point: Point<Pixels>| {
            let content_point = point
                - registration.registration.bounds.origin
                - registration.registration.scroll_offset;
            SelectionEndpoint {
                participant: Some(participant.downgrade()),
                point: content_point,
                inside: true,
                inside_text: true,
                content_key: None,
                content_key_resolver: content_key_resolver
                    .clone()
                    .map(|resolver| (resolver, content_point)),
            }
        };
        self.anchor = Some(to_endpoint(anchor));
        self.cursor = Some(to_endpoint(cursor));
        self.did_hit_text = true;
        self.is_selecting = false;
        participant.update(cx, |state, cx| state.focus(window, cx));
        self.publish_snapshots(cx);
    }

    #[cfg(test)]
    fn update_in_window_with_active_drag(
        &mut self,
        position: Point<Pixels>,
        active_drag: bool,
        window: &Window,
        cx: &mut App,
    ) {
        if !active_drag {
            self.update_impl(position, Some(window), cx);
        }
    }

    fn begin_impl(
        &mut self,
        position: Point<Pixels>,
        extend: bool,
        already_prepared: bool,
        window: Option<&mut Window>,
        cx: &mut App,
    ) {
        GlobalState::init(cx);
        if GlobalState::is_text_selection_suppressed(cx) {
            self.pending_extension_anchor = None;
            return;
        }
        let previous_anchor = extend
            .then(|| {
                self.pending_extension_anchor
                    .take()
                    .or_else(|| self.anchor.clone())
            })
            .flatten()
            .filter(|anchor| anchor.resolve(&self.participants.borrow()).is_some());
        if !extend && !already_prepared {
            self.clear(cx);
        }
        let endpoint = self.endpoint(position, window.as_deref(), cx);
        let focus_participant = endpoint
            .inside
            .then(|| endpoint.participant.clone())
            .flatten();
        let anchor = previous_anchor.unwrap_or_else(|| endpoint.clone());
        self.anchor = Some(anchor.clone());
        self.cursor = Some(endpoint.clone());
        self.did_hit_text = anchor.inside_text || endpoint.inside_text;
        self.is_selecting = true;
        if let Some(participant) = focus_participant.and_then(|participant| participant.upgrade()) {
            if let Some(window) = window {
                participant.update(cx, |state, cx| state.focus(window, cx));
            }
        }
        self.publish_snapshots(cx);
    }

    fn update_impl(&mut self, position: Point<Pixels>, window: Option<&Window>, cx: &mut App) {
        if !self.is_selecting {
            return;
        }
        let endpoint = self.endpoint(position, window, cx);
        self.did_hit_text |= endpoint.inside_text;
        self.cursor = Some(endpoint);
        if window.is_none() {
            self.update_participant_auto_scroll(position, cx);
        }
        self.publish_snapshots(cx);
    }

    fn endpoint(
        &mut self,
        position: Point<Pixels>,
        window: Option<&Window>,
        cx: &App,
    ) -> SelectionEndpoint {
        self.prune_dead_participants();
        let mut hit: Option<(
            WeakEntity<SelectableTextState>,
            Rc<TextSelectionRegistration>,
            f32,
        )> = None;
        let mut predecessor: Option<(
            WeakEntity<SelectableTextState>,
            Rc<TextSelectionRegistration>,
        )> = None;
        let mut first: Option<(
            WeakEntity<SelectableTextState>,
            Rc<TextSelectionRegistration>,
        )> = None;

        for registration in self.participants.borrow().values() {
            if registration.registration.scope != self.active_scope
                || registration.participant.upgrade().is_none()
            {
                continue;
            }
            let participant_geometry = &registration.registration;
            let hovered = window.map_or_else(
                || participant_geometry.bounds.contains(&position),
                |window| participant_geometry.hitbox.is_hovered(window),
            );
            if hovered {
                let area = f32::from(participant_geometry.bounds.size.width)
                    * f32::from(participant_geometry.bounds.size.height);
                if hit.as_ref().is_none_or(|(_, best, best_area)| {
                    area < *best_area
                        || (area == *best_area
                            && participant_geometry.document_order < best.document_order)
                }) {
                    hit = Some((
                        registration.participant.clone(),
                        participant_geometry.clone(),
                        area,
                    ));
                }
            }
            if participant_geometry.bounds.top() <= position.y
                && predecessor.as_ref().is_none_or(|(_, best)| {
                    participant_geometry.bounds.top() > best.bounds.top()
                        || (participant_geometry.bounds.top() == best.bounds.top()
                            && participant_geometry.document_order < best.document_order)
                })
            {
                predecessor = Some((
                    registration.participant.clone(),
                    participant_geometry.clone(),
                ));
            }
            if first.as_ref().is_none_or(|(_, best)| {
                participant_geometry.bounds.top() < best.bounds.top()
                    || (participant_geometry.bounds.top() == best.bounds.top()
                        && participant_geometry.document_order < best.document_order)
            }) {
                first = Some((
                    registration.participant.clone(),
                    participant_geometry.clone(),
                ));
            }
        }

        let selection = hit
            .map(|(participant, registration, _)| (participant, registration, true))
            .or_else(|| {
                predecessor
                    .or(first)
                    .map(|(participant, registration)| (participant, registration, false))
            });
        match selection {
            Some((participant, registration, inside)) => {
                let point = position - registration.bounds.origin - registration.scroll_offset;
                let content_key_resolver = participant.upgrade().and_then(|participant| {
                    participant
                        .read(cx)
                        .content_key_resolver
                        .clone()
                        .map(|callback| (callback, point))
                });
                SelectionEndpoint {
                    point,
                    participant: Some(participant),
                    inside,
                    inside_text: inside
                        && registration
                            .text_bounds
                            .iter()
                            .any(|bounds| bounds.contains(&position)),
                    content_key: None,
                    content_key_resolver,
                }
            }
            None => SelectionEndpoint {
                participant: None,
                point: position,
                inside: false,
                inside_text: false,
                content_key: None,
                content_key_resolver: None,
            },
        }
    }

    fn publish_snapshots(&mut self, cx: &mut App) {
        self.prune_dead_participants();
        let snapshot = self.snapshot();
        let single_participant = self.single_participant();
        for (id, registration) in self.participants.borrow().iter() {
            let Some(participant) = registration.participant.upgrade() else {
                continue;
            };
            let participant_snapshot = (registration.registration.scope == self.active_scope
                && self.participates(*id, registration)
                && single_participant.is_none_or(|single| single == *id))
            .then_some(snapshot)
            .flatten()
            .map(|mut snapshot| {
                snapshot.coverage = self.coverage_for(*id);
                snapshot
            });
            if participant.read(cx).snapshot == participant_snapshot {
                continue;
            }
            participant.update(cx, |state, cx| state.set_snapshot(participant_snapshot, cx));
        }
    }

    fn coverage_for(&self, id: EntityId) -> TextSelectionCoverage {
        let Some(anchor) = self.anchor.as_ref().and_then(SelectionEndpoint::entity_id) else {
            return TextSelectionCoverage::Bounded;
        };
        let Some(cursor) = self.cursor.as_ref().and_then(SelectionEndpoint::entity_id) else {
            return TextSelectionCoverage::Bounded;
        };
        if anchor == cursor {
            return TextSelectionCoverage::Bounded;
        }
        let anchor_order = self.participants.borrow()[&anchor]
            .registration
            .document_order;
        let cursor_order = self.participants.borrow()[&cursor]
            .registration
            .document_order;
        if id != anchor && id != cursor {
            TextSelectionCoverage::Full
        } else if (id == anchor) == (anchor_order < cursor_order) {
            TextSelectionCoverage::ToEnd
        } else {
            TextSelectionCoverage::FromStart
        }
    }

    fn single_participant(&self) -> Option<EntityId> {
        let anchor = self.anchor.as_ref()?.entity_id()?;
        let cursor = self.cursor.as_ref()?.entity_id()?;
        (anchor == cursor).then_some(anchor)
    }

    fn participates(&self, id: EntityId, registration: &ParticipantRegistration) -> bool {
        let Some(anchor) = self.anchor.as_ref().and_then(SelectionEndpoint::entity_id) else {
            return false;
        };
        let Some(cursor) = self.cursor.as_ref().and_then(SelectionEndpoint::entity_id) else {
            return false;
        };
        let Some(anchor_registration) = self.participants.borrow().get(&anchor).cloned() else {
            return false;
        };
        let Some(cursor_registration) = self.participants.borrow().get(&cursor).cloned() else {
            return false;
        };
        let start = anchor_registration
            .registration
            .document_order
            .min(cursor_registration.registration.document_order);
        let end = anchor_registration
            .registration
            .document_order
            .max(cursor_registration.registration.document_order);
        (start..=end).contains(&registration.registration.document_order)
            || id == anchor
            || id == cursor
    }

    fn update_auto_scroll(
        &mut self,
        position: Point<Pixels>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        // A finished gesture keeps its anchor for shift-click extension; only
        // a live drag may scroll.
        if !self.is_selecting {
            return;
        }
        let Some((_, registration)) = self.anchor_registration() else {
            return;
        };
        // Exactly one writer per drag: a participant that scrolls its own
        // content is notified directly; anything else gets a synthetic wheel.
        if registration.self_scroll {
            self.auto_scroll.stop();
            self.update_participant_auto_scroll(position, cx);
            return;
        }
        // The content mask is the nearest clipping viewport established by a
        // scrollable ancestor. It remains stable as the participant itself
        // moves, so selection keeps scrolling the same related region even
        // after the anchor text has moved out of view.
        let visible_bounds = registration.hitbox.content_mask.bounds;
        // Keeps the synthesized wheel event hit-testing inside the mask.
        const HIT_TEST_INSET: Pixels = px(1.);
        // A collapsed mask leaves an empty clamp range below — stop.
        if visible_bounds.size.width < HIT_TEST_INSET * 2.
            || visible_bounds.size.height < HIT_TEST_INSET * 2.
        {
            self.stop_anchor_auto_scroll(cx);
            return;
        }
        let delta = AutoScroll::compute_delta(position.y, visible_bounds);
        let event_position = point(
            position.x.clamp(
                visible_bounds.left() + HIT_TEST_INSET,
                visible_bounds.right() - HIT_TEST_INSET,
            ),
            position.y.clamp(
                visible_bounds.top() + HIT_TEST_INSET,
                visible_bounds.bottom() - HIT_TEST_INSET,
            ),
        );
        self.auto_scroll.last_drag_position = Some(event_position);
        self.auto_scroll_stall = (None, 0);
        let window = window.window_handle();
        self.auto_scroll.set(delta, cx, move |delta, state, cx| {
            let Some(position) = state.auto_scroll.last_drag_position else {
                return;
            };
            // Hold off once the container has shown, over a few ticks, that
            // it has nowhere left to go; the next move of the finger asks
            // again.
            const STALLED_TICKS: u8 = 3;
            let geometry = state
                .anchor_registration()
                .map(|(_, registration)| (registration.bounds, registration.scroll_offset));
            let (last, stalled) = &mut state.auto_scroll_stall;
            if *last == geometry {
                *stalled = stalled.saturating_add(1);
                if *stalled >= STALLED_TICKS {
                    return;
                }
            } else {
                *last = geometry;
                *stalled = 0;
            }
            let window = window;
            cx.defer(move |cx| {
                _ = window.update(cx, |_, window, cx| {
                    window.dispatch_event(
                        ScrollWheelEvent {
                            position,
                            delta: ScrollDelta::Pixels(point(px(0.), -delta)),
                            ..Default::default()
                        }
                        .to_platform_input(),
                        cx,
                    );
                });
            });
        });
    }

    fn update_participant_auto_scroll(&self, position: Point<Pixels>, cx: &mut App) {
        let Some((participant, registration)) = self.anchor_registration() else {
            return;
        };
        let visible_bounds = registration
            .bounds
            .intersect(&registration.hitbox.content_mask.bounds);
        let delta = if visible_bounds.size.width > px(0.) && visible_bounds.size.height > px(0.) {
            AutoScroll::compute_delta(position.y, visible_bounds)
        } else {
            None
        };
        participant.update(cx, |state, cx| state.set_auto_scroll(delta, cx));
    }

    fn stop_anchor_auto_scroll(&mut self, cx: &mut App) {
        self.auto_scroll.stop();
        let Some(participant) = self.anchor_participant() else {
            return;
        };
        participant.update(cx, |state, cx| state.set_auto_scroll(None, cx));
    }

    fn anchor_participant(&self) -> Option<Entity<SelectableTextState>> {
        self.anchor
            .as_ref()
            .filter(|anchor| anchor.inside)?
            .participant
            .as_ref()?
            .upgrade()
    }

    fn anchor_registration(
        &self,
    ) -> Option<(Entity<SelectableTextState>, Rc<TextSelectionRegistration>)> {
        let participant = self.anchor_participant()?;
        let registration = self
            .participants
            .borrow()
            .get(&participant.entity_id())?
            .clone();
        Some((participant, registration.registration.clone()))
    }

    fn prune_dead_participants(&mut self) {
        self.participants
            .get_mut()
            .retain(|_, registration| registration.participant.upgrade().is_some());
    }
}

#[derive(Default)]

struct SelectionStateRegistry(HashMap<gpui::WindowId, WeakEntity<WindowSelectionState>>);

impl Global for SelectionStateRegistry {}

#[derive(Default)]
struct PendingTextSelectionScopes(HashMap<gpui::WindowId, TextSelectionScopeId>);

impl Global for PendingTextSelectionScopes {}

#[derive(Default)]
struct TextSelectionScopeStacks(HashMap<gpui::WindowId, Vec<TextSelectionScopeId>>);

impl Global for TextSelectionScopeStacks {}

fn push_text_selection_scope(window_id: gpui::WindowId, scope: TextSelectionScopeId, cx: &mut App) {
    if !cx.has_global::<TextSelectionScopeStacks>() {
        cx.set_global(TextSelectionScopeStacks::default());
    }
    cx.global_mut::<TextSelectionScopeStacks>()
        .0
        .entry(window_id)
        .or_default()
        .push(scope);
}

fn pop_text_selection_scope(window_id: gpui::WindowId, cx: &mut App) {
    let stacks = &mut cx.global_mut::<TextSelectionScopeStacks>().0;
    let remove_stack = stacks.get_mut(&window_id).is_some_and(|stack| {
        stack.pop();
        stack.is_empty()
    });
    if remove_stack {
        stacks.remove(&window_id);
    }
}

fn current_text_selection_scope(
    window_id: gpui::WindowId,
    cx: &App,
) -> Option<TextSelectionScopeId> {
    cx.has_global::<TextSelectionScopeStacks>()
        .then(|| {
            cx.global::<TextSelectionScopeStacks>()
                .0
                .get(&window_id)
                .and_then(|stack| stack.last().copied())
        })
        .flatten()
}

fn with_text_selection_scope<T>(
    window_id: gpui::WindowId,
    scope: TextSelectionScopeId,
    cx: &mut App,
    callback: impl FnOnce(&mut App) -> T,
) -> T {
    push_text_selection_scope(window_id, scope, cx);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback(cx)));
    pop_text_selection_scope(window_id, cx);
    match result {
        Ok(result) => result,
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

pub struct TextSelection;

impl TextSelection {
    pub fn is_selectable_at(position: Point<Pixels>, window: &Window, cx: &mut App) -> bool {
        live_text_selection_state(window, cx).is_some_and(|state| {
            state.update(cx, |state, cx| {
                !state.touch.covers(position)
                    && state.endpoint(position, Some(window), cx).inside_text
            })
        })
    }

    pub fn selected_text(window: &mut Window, cx: &mut App) -> String {
        let Some(state) = live_text_selection_state(window, cx) else {
            return String::new();
        };
        let items = state.read(cx).copy_items(cx);
        resolve_copy_items(items, cx)
    }

    pub fn has_selection(window: &mut Window, cx: &mut App) -> bool {
        live_text_selection_state(window, cx).is_some_and(|state| state.read(cx).has_selection(cx))
    }

    pub fn clear(window: &mut Window, cx: &mut App) {
        if let Some(state) = live_text_selection_state(window, cx) {
            let handlers = state.update(cx, |state, cx| state.clear_state(cx));
            dispatch_clear_handlers(handlers, cx);
        }
    }

    pub fn clear_for_window(window_id: gpui::WindowId, cx: &mut App) {
        clear_window_text_selection(window_id, cx);
    }

    pub fn end(window: &mut Window, cx: &mut App) {
        if let Some(state) = live_text_selection_state(window, cx) {
            state.update(cx, |state, cx| state.end(cx));
        }
    }

    pub fn observe_touch_selection(
        window: &Window,
        cx: &mut App,
        callback: impl Fn(&mut App) + 'static,
    ) -> Subscription {
        let state = WindowSelectionState::acquire(window.window_handle().window_id(), cx);
        cx.observe(&state, move |_, cx| callback(cx))
    }

    pub fn touch_selection(window: &Window, cx: &App) -> Option<TouchSelectionSnapshot> {
        WindowSelectionState::existing(window, cx)?
            .read(cx)
            .touch_selection()
    }

    pub fn register_touch_ui(bounds: Bounds<Pixels>, window: &Window, cx: &mut App) {
        if let Some(state) = WindowSelectionState::existing(window, cx) {
            state.update(cx, |state, _| state.register_touch_ui(bounds));
        }
    }

    pub fn close_edit_menu(window: &mut Window, cx: &mut App) {
        if let Some(state) = live_text_selection_state(window, cx) {
            state.update(cx, |state, cx| state.close_edit_menu(cx));
        }
    }

    pub fn select_all(window: &mut Window, cx: &mut App) {
        if let Some(state) = live_text_selection_state(window, cx) {
            state.update(cx, |state, cx| state.select_all_touched(cx));
        }
    }

    pub fn begin_edge_drag(
        edge: SelectionEdge,
        finger: Point<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(state) = live_text_selection_state(window, cx) {
            state.update(cx, |state, cx| {
                state.begin_edge_drag(edge, finger, window, cx)
            });
            WindowSelectionState::resolve_content_keys(&state, cx);
        }
    }

    pub fn update_edge_drag(finger: Point<Pixels>, window: &mut Window, cx: &mut App) {
        if let Some(state) = live_text_selection_state(window, cx) {
            state.update(cx, |state, cx| state.update_edge_drag(finger, window, cx));
            WindowSelectionState::resolve_content_keys(&state, cx);
        }
    }

    pub fn end_edge_drag(window: &mut Window, cx: &mut App) {
        if let Some(state) = live_text_selection_state(window, cx) {
            state.update(cx, |state, cx| state.end_edge_drag(cx));
        }
    }

    pub fn activate_scope(scope: TextSelectionScopeId, window: &mut Window, cx: &mut App) {
        let Some(state) = WindowSelectionState::existing(window, cx) else {
            if !cx.has_global::<PendingTextSelectionScopes>() {
                cx.set_global(PendingTextSelectionScopes::default());
            }
            cx.global_mut::<PendingTextSelectionScopes>()
                .0
                .insert(window.window_handle().window_id(), scope);
            return;
        };
        if state.read(cx).active_scope == scope {
            return;
        }
        let handlers = state.update(cx, |state, cx| state.set_active_scope_state(scope, cx));
        dispatch_clear_handlers(handlers, cx);
    }
}

pub struct TextSelectionLayer;

pub(crate) fn text_selection_scope(
    scope: TextSelectionScopeId,
    element: impl IntoElement,
) -> impl IntoElement {
    TextSelectionScopeMarker {
        scope,
        element: element.into_element(),
    }
}

struct TextSelectionScopeMarker<E> {
    scope: TextSelectionScopeId,
    element: E,
}

impl<E: Element> IntoElement for TextSelectionScopeMarker<E> {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl<E: Element> Element for TextSelectionScopeMarker<E> {
    type RequestLayoutState = E::RequestLayoutState;
    type PrepaintState = E::PrepaintState;

    fn id(&self) -> Option<ElementId> {
        self.element.id()
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        self.element.source_location()
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let window_id = window.window_handle().window_id();
        with_text_selection_scope(window_id, self.scope, cx, |cx| {
            self.element.request_layout(id, inspector_id, window, cx)
        })
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let window_id = window.window_handle().window_id();
        with_text_selection_scope(window_id, self.scope, cx, |cx| {
            self.element
                .prepaint(id, inspector_id, bounds, request_layout, window, cx)
        })
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let window_id = window.window_handle().window_id();
        with_text_selection_scope(window_id, self.scope, cx, |cx| {
            self.element.paint(
                id,
                inspector_id,
                bounds,
                request_layout,
                prepaint,
                window,
                cx,
            );
        });
    }
}

#[doc(hidden)]
pub struct TextSelectionLayerPrepaintState(Entity<WindowSelectionState>);

impl IntoElement for TextSelectionLayer {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextSelectionLayer {
    type RequestLayoutState = ();
    type PrepaintState = TextSelectionLayerPrepaintState;

    fn id(&self) -> Option<ElementId> {
        Some("window-text-selection".into())
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
        (window.request_layout(Style::default(), [], cx), ())
    }

    fn prepaint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        // Automatic participant order is paint order within this frame. Keep
        // this lifecycle in base so base-only applications do not need a
        // separate root component to reset it. Otherwise, registering the
        // first of two selected TextViews temporarily reverses their order
        // against the previous frame and alternates coverage forever.
        GlobalState::init(cx);
        GlobalState::global(cx).begin_selection_frame();
        let state = retain_text_selection_state(global_id, window, cx);
        // The handles and the menu register again as they paint this frame.
        if state.read(cx).touch.painted_any() {
            state.update(cx, |state, _| state.touch.begin_frame());
        }
        TextSelectionLayerPrepaintState(state)
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        state: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        paint_text_selection(&state.0, window, cx);
    }
}

fn retain_text_selection_state(
    global_id: Option<&GlobalElementId>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<WindowSelectionState> {
    let window_id = window.window_handle().window_id();
    let state = window.with_element_state::<Entity<WindowSelectionState>, _>(
        global_id.expect("TextSelection has a stable element id"),
        |retained, _| {
            let state = retained.unwrap_or_else(|| WindowSelectionState::acquire(window_id, cx));
            (state.clone(), state)
        },
    );
    // Registered once: writing the global again every frame would change it,
    // and every view that reads it, on every frame.
    let registered = cx.has_global::<SelectionStateRegistry>()
        && cx
            .global::<SelectionStateRegistry>()
            .0
            .get(&window_id)
            .is_some_and(|registered| registered.entity_id() == state.entity_id());
    if !registered {
        if !cx.has_global::<SelectionStateRegistry>() {
            cx.set_global(SelectionStateRegistry::default());
        }
        cx.global_mut::<SelectionStateRegistry>()
            .0
            .insert(window_id, state.downgrade());
    }
    state
}

fn paint_text_selection(state: &Entity<WindowSelectionState>, window: &mut Window, cx: &mut App) {
    if state.read(cx).schedule_finish_frame() {
        let state = state.downgrade();
        window.defer(cx, move |window, cx| {
            let Some(state) = state.upgrade() else {
                return;
            };
            if state.read(cx).finish_frame_quietly(cx) {
                return;
            }
            let handlers = state.update(cx, |state, cx| state.finish_frame(cx));
            dispatch_clear_handlers(handlers, cx);
            // Direct participant scrolling produces no wheel event. Refresh
            // the held cursor after paint registers the new scroll geometry.
            let refresh_cursor = state.update(cx, |state, cx| {
                if std::mem::take(&mut state.refresh_held_cursor) && state.is_selecting {
                    // A handle drag holds the finger off the text; keep the
                    // same offset, or the cursor would hop between the two.
                    let position = window.mouse_position();
                    let position = state
                        .touch
                        .drag
                        .map_or(position, |drag| drag.text_position(position));
                    state.update_in_window(position, window, cx);
                    true
                } else {
                    false
                }
            });
            if refresh_cursor {
                WindowSelectionState::resolve_content_keys(&state, cx);
            }
        });
    }

    let mouse_down_state = state.downgrade();
    window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
        if event.button != MouseButton::Left {
            return;
        }
        let Some(state) = mouse_down_state.upgrade() else {
            return;
        };
        // A press on a handle or on the edit menu acts on the selection; it
        // must not clear it.
        if state.read(cx).touch.covers(event.position) {
            return;
        }
        if phase.capture() {
            GlobalState::init(cx);
            GlobalState::reset_text_selection_suppression(cx);
            let handlers = state.update(cx, |state, cx| {
                if state.mouse_down_prepared {
                    return Vec::new();
                }
                state.mouse_down_prepared = true;
                state.prepare_for_mouse_down(event.click_count == 1 && event.modifiers.shift, cx)
            });
            dispatch_clear_handlers(handlers, cx);
        } else if event.click_count == 1 {
            if GlobalState::is_text_selection_suppressed(cx) {
                state.update(cx, |state, _| state.pending_extension_anchor = None);
                return;
            }
            state.update(cx, |state, cx| {
                if !state.is_selecting {
                    state.begin_in_window(event.position, event.modifiers.shift, window, cx)
                }
            });
            WindowSelectionState::resolve_content_keys(&state, cx);
        } else if event.click_count >= 2 {
            if GlobalState::is_text_selection_suppressed(cx) {
                return;
            }
            let touch = GlobalState::is_touch_press(cx);
            state.update(cx, |state, cx| {
                state.select_at(event.position, event.click_count, window, cx);
                // A double tap is touch's other way to select a word, and it
                // gets the handles and the menu like a long press does.
                if touch {
                    state.keep_touch_selection(cx);
                }
            });
            WindowSelectionState::resolve_content_keys(&state, cx);
        }
    });

    // Every touch is offered as a drag first; that is how a tap's mouse
    // events are later told apart from a mouse's.
    window.on_mouse_event(move |event: &TouchDragEvent, phase, _, cx| {
        if phase.capture() && event.phase == TouchPhase::Started {
            GlobalState::note_touch(cx);
        }
    });

    // Touch panning remains scrolling until a long press actually hits text.
    // Claiming the gesture keeps subsequent moves out of the pan recognizer.
    let long_press_state = state.downgrade();
    window.on_mouse_event(move |event: &LongPressEvent, phase, window, cx| {
        if !phase.bubble() {
            return;
        }
        let Some(state) = long_press_state.upgrade() else {
            return;
        };
        if event.phase == TouchPhase::Started {
            if window.default_prevented()
                || state.read(cx).touch.covers(event.start_position)
                || !state.update(cx, |state, cx| {
                    state
                        .endpoint(event.start_position, Some(window), cx)
                        .inside_text
                })
            {
                return;
            }
            GlobalState::init(cx);
            GlobalState::reset_text_selection_suppression(cx);
            let handlers = state.update(cx, |state, cx| state.prepare_for_mouse_down(false, cx));
            dispatch_clear_handlers(handlers, cx);
            let selected = state.update(cx, |state, cx| {
                state.select_at(event.start_position, 2, window, cx);
                state.anchor.is_some()
            });
            if !selected {
                return;
            }
            window.capture_long_press(&state);
        } else if !window.has_long_press_capture(&state) {
            return;
        } else {
            state.update(cx, |state, cx| match event.phase {
                TouchPhase::Moved => {
                    state.is_selecting = true;
                    state.update_in_window(event.position, window, cx);
                }
                TouchPhase::Ended | TouchPhase::Cancelled => {
                    state.end(cx);
                    // The finger is up; the selection it made gets its
                    // handles and the edit menu.
                    state.keep_touch_selection(cx);
                }
                _ => {}
            });
        }
        window.prevent_default();
        cx.stop_propagation();
        WindowSelectionState::resolve_content_keys(&state, cx);
    });

    // A handle drag in progress follows the finger or the pointer wherever
    // it goes, whether or not the handle it took is laid out this frame.
    let drag_state = state.downgrade();
    window.on_mouse_event(move |event: &TouchDragEvent, phase, window, cx| {
        if !phase.bubble() || event.phase == TouchPhase::Started {
            return;
        }
        let Some(state) = drag_state.upgrade() else {
            return;
        };
        if state.read(cx).touch.drag.is_none() {
            return;
        }
        cx.stop_propagation();
        state.update(cx, |state, cx| match event.phase {
            TouchPhase::Moved => state.update_edge_drag(event.position, window, cx),
            _ => state.end_edge_drag(cx),
        });
        WindowSelectionState::resolve_content_keys(&state, cx);
    });
    let drag_state = state.downgrade();
    window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
        if phase.bubble()
            && event.pressed_button == Some(MouseButton::Left)
            && let Some(state) = drag_state.upgrade()
            && state.read(cx).touch.drag.is_some()
        {
            state.update(cx, |state, cx| {
                state.update_edge_drag(event.position, window, cx)
            });
            WindowSelectionState::resolve_content_keys(&state, cx);
        }
    });
    let drag_state = state.downgrade();
    window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
        if phase.bubble()
            && event.button == MouseButton::Left
            && let Some(state) = drag_state.upgrade()
        {
            state.update(cx, |state, cx| state.end_edge_drag(cx));
        }
    });

    let mouse_move_state = state.downgrade();
    window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
        if phase.bubble()
            && let Some(state) = mouse_move_state.upgrade()
            // Moving the pointer only moves a selection being made.
            && state.read(cx).is_selecting
        {
            state.update(cx, |state, cx| {
                // A handle drag maps the pointer through the handle's offset;
                // the raw pointer must not fight it.
                if state.touch.drag.is_some() {
                    return;
                }
                state.update_in_window(event.position, window, cx)
            });
            WindowSelectionState::resolve_content_keys(&state, cx);
        }
    });

    let mouse_up_state = state.downgrade();
    window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
        if phase.bubble()
            && let Some(state) = mouse_up_state.upgrade()
        {
            state.update(cx, |state, cx| {
                state.mouse_down_prepared = false;
                state.end(cx)
            });
        }
    });

    let scroll_state = state.downgrade();
    window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, cx| {
        let Some(state) = scroll_state.upgrade() else {
            return;
        };
        // On capture, before a scroll container can stop the event: one
        // stretched past its end swallows the whole stream, the lift
        // included, and the menu would never come back.
        if phase.capture() {
            if state.read(cx).touch.active {
                state.update(cx, |state, cx| {
                    state.edit_menu_on_scroll(event.touch_phase, cx)
                });
            }
            return;
        }
        // A scroll only moves a selection being made under the pointer.
        if phase.bubble() && state.read(cx).is_selecting {
            let position = window.mouse_position();
            state.update(cx, |state, cx| {
                // A handle drag holds the finger off the text; keep its offset.
                let position = state
                    .touch
                    .drag
                    .map_or(position, |drag| drag.text_position(position));
                state.update_in_window(position, window, cx)
            });
            WindowSelectionState::resolve_content_keys(&state, cx);
        }
    });
}

fn live_text_selection_state(
    window: &Window,
    cx: &mut App,
) -> Option<Entity<WindowSelectionState>> {
    WindowSelectionState::existing(window, cx)
}

pub(crate) fn clear_window_text_selection(window_id: gpui::WindowId, cx: &mut App) {
    if !cx.has_global::<SelectionStateRegistry>() {
        return;
    }
    let Some(state) = cx
        .global::<SelectionStateRegistry>()
        .0
        .get(&window_id)
        .and_then(WeakEntity::upgrade)
    else {
        return;
    };
    let handlers = state.update(cx, |state, cx| state.clear_state(cx));
    dispatch_clear_handlers(handlers, cx);
}
