use std::{
    cell::{Cell, RefCell},
    ops::Range,
    rc::Rc,
    time::Duration,
};

use gpui_kit::base::{
    MotionStatus, StyledExt as _, TextSelection, TextSelectionHandle, TextSelectionRegistration,
    TextSelectionRun, Transition,
    input::{Copy, Enter, InputEvent, InputPreparation},
    transition_with_status,
};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Selectable as _, Sizable as _, Size,
    button::{Button, ButtonCustomVariant, ButtonRounded, ButtonVariants as _},
    input::{Editor, EditorState, Input, InputState, Textarea, TextareaState},
    menu::{DropdownMenu as _, PopupMenuItem},
    scroll::ScrollableElement as _,
    toolbar::Toolbar,
    tooltip::Tooltip,
};
use gpui_kit::{
    Anchor, AnyElement, App, AppContext as _, BorderStyle, Bounds, ClipboardItem, Context, Corners,
    CursorStyle, DispatchPhase, Div, DragMoveEvent, Edges, Element, ElementId, EmptyView, Entity,
    FocusHandle, Focusable as _, GlobalElementId, Hitbox, HitboxBehavior, Hsla, InspectorElementId,
    InteractiveElement as _, IntoElement, KeyBinding, KeyDownEvent, LayoutId, MouseButton,
    MouseMoveEvent, MouseUpEvent, PaintQuad, ParentElement as _, Pixels, Point, Render,
    ScrollHandle, SharedString, StatefulInteractiveElement as _, Styled as _, StyledText,
    Subscription, TextLayout, TextStyleRefinement, Window, accesskit::Role, canvas, deferred, div,
    prelude::FluentBuilder as _, px, relative, rems, transparent_black,
};
use js_sys::Function;
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsValue;

use crate::{
    DETAIL_PANE_MAX_WIDTH_REMS,
    buttons::{
        action_button, checked_menu_item, field_button, field_button_icon, field_button_label,
        small_icon,
    },
    palette,
    query::url_with_default_scheme,
    request_defaults::{
        RequestDefaults, RequestDefaultsDialog, SettingInheritance, request_settings_controls,
        set_timeout_input, timeout_input, timeout_seconds,
    },
    request_fields::RequestFields,
    response_headers::ResponseHeaders,
    timeline_foregrounds,
    tree_sitter_highlighter::{ParseSnapshot, SyntaxEditHandler, TreeSitterHighlighter},
    workspace_tab_background, workspace_tab_foreground,
};

const METHODS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];
const RESPONSE_TABS: &[&str] = &["Response", "Headers", "Cookies", "Timeline"];
const DEFAULT_REQUEST_FRACTION: f32 = 0.515;
const MIN_PANE_FRACTION: f32 = 0.2;
const REQUEST_PANE_CONTEXT: &str = "HttpRequestPane";
const TIMELINE_CONTEXT: &str = "ResponseTimeline";

gpui_kit::actions!(http_client, [SendRequest]);

pub(super) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-enter", SendRequest, Some(REQUEST_PANE_CONTEXT)),
        // Match at input depth too so the editor's secondary Enter binding cannot win.
        KeyBinding::new("ctrl-enter", SendRequest, Some("HttpRequestPane > Input")),
        KeyBinding::new("ctrl-c", Copy, Some(TIMELINE_CONTEXT)),
        KeyBinding::new("cmd-c", Copy, Some(TIMELINE_CONTEXT)),
    ]);
}

struct PaneResize;

#[derive(Clone, Copy, PartialEq, Eq)]
enum RequestTab {
    Params,
    Body,
    Headers,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum BodyFormat {
    Json,
    UrlEncoded,
    MultiPart,
    Text,
    Binary,
    NoBody,
}

impl BodyFormat {
    const OPTIONS: [Self; 6] = [
        Self::Json,
        Self::UrlEncoded,
        Self::MultiPart,
        Self::Text,
        Self::Binary,
        Self::NoBody,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Json => "JSON",
            Self::UrlEncoded => "Url Encoded",
            Self::MultiPart => "Multi-Part",
            Self::Text => "Text",
            Self::Binary => "Binary",
            Self::NoBody => "No Body",
        }
    }

    fn content_type(self) -> Option<&'static str> {
        match self {
            Self::Json => Some("application/json"),
            Self::UrlEncoded => Some("application/x-www-form-urlencoded"),
            Self::MultiPart => Some("multipart/form-data"),
            Self::Text => Some("text/plain"),
            Self::Binary => Some("application/octet-stream"),
            Self::NoBody => None,
        }
    }
}

#[derive(Default, Deserialize)]
pub(super) struct Preview {
    url: String,
    headers: String,
    response: Option<Response>,
}

pub(super) struct ClientCallbacks {
    pub(super) send: Function,
    pub(super) queue_edit: Function,
    pub(super) load_request: Function,
    pub(super) pick_binary: Function,
    pub(super) clear_binary: Function,
    pub(super) download_response: Function,
    pub(super) close_tab: Function,
    pub(super) archive_tab: Function,
}

#[derive(Serialize)]
struct Request {
    method: &'static str,
    url: String,
    headers: String,
    body_format: BodyFormat,
    timeout_ms: u64,
    follow_redirects: bool,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ResponseFormat {
    #[default]
    Json,
    Text,
    Binary,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TimelineSide {
    Request,
    Response,
}

#[derive(Deserialize)]
pub(super) struct TimelineEvent {
    label: String,
    elapsed_ms: f64,
    side: TimelineSide,
}

#[derive(Clone)]
struct TimelineCellSelection {
    handle: TextSelectionHandle,
    fallback: SharedString,
    last_run: Rc<RefCell<Option<TextSelectionRun>>>,
}

impl TimelineCellSelection {
    fn new(text: impl Into<SharedString>, focus: &FocusHandle, cx: &mut App) -> Self {
        let fallback: SharedString = text.into();
        let handle = TextSelectionHandle::new(fallback.to_string(), cx);
        let focus = focus.clone();
        handle.focus_with(move |window, cx| focus.focus(window, cx), cx);
        Self {
            handle,
            fallback,
            last_run: Rc::new(RefCell::new(None)),
        }
    }

    fn selected_text(&self, cx: &mut App) -> Option<String> {
        self.handle.snapshot(cx)?;
        let text = if let Some(run) = self.last_run.borrow().clone() {
            let projection = self.handle.update_runs(std::slice::from_ref(&run), cx);
            let range = projection.ranges().first()?.as_ref()?;
            run.text().get(range.clone())?.to_owned()
        } else {
            self.fallback.to_string()
        };
        (!text.is_empty()).then_some(text)
    }
}

fn paint_selection_quads(
    layout: &TextLayout,
    range: Range<usize>,
    color: Hsla,
    window: &mut Window,
) {
    let (Some(start), Some(end)) = (
        layout.position_for_index(range.start),
        layout.position_for_index(range.end),
    ) else {
        return;
    };
    let bounds = layout.bounds();
    let line_height = layout.line_height();
    let quads = if start.y == end.y {
        vec![Bounds::from_corners(
            start,
            Point::new(end.x, end.y + line_height),
        )]
    } else {
        let mut quads = vec![Bounds::from_corners(
            start,
            Point::new(bounds.right(), start.y + line_height),
        )];
        if end.y > start.y + line_height {
            quads.push(Bounds::from_corners(
                Point::new(bounds.left(), start.y + line_height),
                Point::new(bounds.right(), end.y),
            ));
        }
        quads.push(Bounds::from_corners(
            Point::new(bounds.left(), end.y),
            Point::new(end.x, end.y + line_height),
        ));
        quads
    };
    for bounds in quads {
        window.paint_quad(PaintQuad {
            bounds,
            background: color.into(),
            corner_radii: Corners::default(),
            border_widths: Edges::default(),
            border_color: transparent_black(),
            border_style: BorderStyle::default(),
        });
    }
}

struct TimelineCell {
    id: ElementId,
    cell: TimelineCellSelection,
    styled_text: StyledText,
    document_order: u64,
    selection_color: Hsla,
}

impl IntoElement for TimelineCell {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TimelineCell {
    type RequestLayoutState = ();
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.styled_text
            .request_layout(global_id, inspector_id, window, cx)
    }

    fn prepaint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.styled_text
            .prepaint(global_id, inspector_id, bounds, &mut (), window, cx);
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        let registration = TextSelectionRegistration::new(hitbox.clone(), bounds)
            .with_document_order(self.document_order)
            .with_text_bounds(vec![bounds])
            .with_rendered_element(&self.cell.handle, window, cx);
        self.cell.handle.register(registration, window, cx);
        hitbox
    }

    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let layout = self.styled_text.layout().clone();
        let run = TextSelectionRun::new(self.cell.fallback.clone(), layout.clone(), bounds)
            .with_document_order(self.document_order);
        *self.cell.last_run.borrow_mut() = Some(run.clone());
        let selected_text_before = TextSelection::selected_text(window, cx);
        let projection = self.cell.handle.update_runs(&[run], cx);
        if selected_text_before != TextSelection::selected_text(window, cx) {
            window.refresh();
        }
        for range in projection.ranges().iter().flatten().cloned() {
            paint_selection_quads(&layout, range, self.selection_color, window);
        }
        self.styled_text.paint(
            global_id,
            inspector_id,
            bounds,
            &mut (),
            &mut (),
            window,
            cx,
        );
    }
}

struct TimelineRowHandles {
    label: TimelineCellSelection,
    time: TimelineCellSelection,
    cumulative: TimelineCellSelection,
}

impl TimelineRowHandles {
    fn new(label: &str, time: &str, cumulative: &str, focus: &FocusHandle, cx: &mut App) -> Self {
        let [label, time, cumulative] = [label, time, cumulative]
            .map(|text| TimelineCellSelection::new(text.to_owned(), focus, cx));
        Self {
            label,
            time,
            cumulative,
        }
    }

    fn cell(
        id: impl Into<ElementId>,
        cell: &TimelineCellSelection,
        order: u64,
        cx: &App,
    ) -> TimelineCell {
        TimelineCell {
            id: id.into(),
            styled_text: StyledText::new(cell.fallback.clone()),
            cell: cell.clone(),
            document_order: order,
            selection_color: cx.theme().selection,
        }
    }

    fn selected_row_text(&self, cx: &mut App) -> Option<String> {
        let cells = [&self.label, &self.time, &self.cumulative]
            .into_iter()
            .filter_map(|cell| cell.selected_text(cx))
            .collect::<Vec<_>>();
        (!cells.is_empty()).then(|| cells.join("\t"))
    }
}

struct TimelineRowData {
    side: TimelineSide,
    handles: TimelineRowHandles,
}

struct ResponseTimeline {
    focus: FocusHandle,
    scroll: ScrollHandle,
    header: TimelineRowHandles,
    rows: Vec<TimelineRowData>,
    total: Option<TimelineRowHandles>,
}

impl ResponseTimeline {
    fn new(cx: &mut App) -> Self {
        let focus = cx.focus_handle();
        let header =
            TimelineRowHandles::new("Event", "Time (ms)", "Cumulative time (ms)", &focus, cx);
        Self {
            focus,
            scroll: ScrollHandle::new(),
            header,
            rows: Vec::new(),
            total: None,
        }
    }

    fn set_events(
        &mut self,
        events: &[TimelineEvent],
        reset_scroll: bool,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.clear_selection_if_focused(window, cx);
        let mut previous = 0.0;
        let mut total = 0.0;
        self.rows = events
            .iter()
            .map(|event| {
                let delta = (event.elapsed_ms - previous).max(0.0);
                previous = event.elapsed_ms;
                total += delta;
                let time = format!("{delta:.2}");
                let cumulative = format!("{:.2}", event.elapsed_ms);
                let handles =
                    TimelineRowHandles::new(&event.label, &time, &cumulative, &self.focus, cx);
                TimelineRowData {
                    side: event.side,
                    handles,
                }
            })
            .collect();
        self.total = (!events.is_empty()).then(|| {
            let time = format!("{total:.2}");
            let cumulative = format!("{previous:.2}");
            TimelineRowHandles::new("Total", &time, &cumulative, &self.focus, cx)
        });
        if reset_scroll {
            self.scroll.set_offset(Point::default());
        }
    }

    fn selected_text(&self, window: &mut Window, cx: &mut App) -> String {
        if !self.focus.is_focused(window) {
            return String::new();
        }
        std::iter::once(&self.header)
            .chain(self.rows.iter().map(|row| &row.handles))
            .chain(self.total.as_ref())
            .filter_map(|row| row.selected_row_text(cx))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn clear_selection_if_focused(&self, window: &mut Window, cx: &mut App) {
        if self.focus.is_focused(window) {
            TextSelection::clear(window, cx);
        }
    }
}

#[derive(Deserialize)]
struct Response {
    #[serde(default)]
    preview: bool,
    status: u16,
    status_text: String,
    https_upgrade_url: Option<String>,
    elapsed_ms: u32,
    bytes: usize,
    headers: Vec<(String, String)>,
    #[serde(default)]
    format: ResponseFormat,
    #[serde(default)]
    timeline: Vec<TimelineEvent>,
}

struct JsonBody {
    editor: Entity<EditorState>,
    syntax: Rc<RefCell<ParseSnapshot>>,
    preparation: Option<(u32, InputPreparation)>,
}

impl JsonBody {
    fn new(on_edit: Option<SyntaxEditHandler>, window: &mut Window, cx: &mut App) -> Self {
        let syntax = Rc::new(RefCell::new(ParseSnapshot::default()));
        syntax.borrow_mut().prepare_open();
        let snapshot = syntax.clone();
        let font = TextStyleRefinement {
            font_family: Some(cx.theme().mono_font_family.clone()),
            ..Default::default()
        };
        let editor = cx.new(|cx| {
            // A body may be prepared before its tab has painted for the first time.
            let mut state = window
                .with_text_style(Some(font), |window| EditorState::new(window, cx))
                .language("json5")
                .searchable(false)
                .soft_wrap(true)
                .indent_guides(false);
            state.set_highlighter_factory(
                Rc::new(move |language| {
                    TreeSitterHighlighter::new(language, snapshot.clone(), on_edit.clone())
                        .map(|highlighter| Box::new(highlighter) as Box<_>)
                }),
                cx,
            );
            state.set_highlighter("json5", cx);
            state.set_gutter_left_padding_em(0.25, cx);
            state.set_gutter_reserved_digits(Some(2), cx);
            state.set_gutter_line_number_font_size(Some(px(11.)), cx);
            state
        });
        Self {
            editor,
            syntax,
            preparation: None,
        }
    }

    fn install_syntax(
        &self,
        editor: &mut EditorState,
        highlights: &[u32],
        folds: &[u32],
        brackets: &[u32],
        diagnostics: &str,
        cx: &mut Context<EditorState>,
    ) -> Result<(), String> {
        let mut syntax = self.syntax.borrow_mut();
        syntax.install(highlights, folds, brackets, diagnostics, editor.text())?;
        let text = editor.text().clone();
        if let Some(current) = editor.diagnostics_mut() {
            current.reset(&text);
            current.extend(syntax.diagnostics.clone());
        }
        editor.apply_highlighter_fold_candidates(syntax.folds.clone(), cx);
        Ok(())
    }

    fn render(&self, response: bool, loading: bool, cx: &App) -> Editor {
        Editor::new(&self.editor)
            .readonly(response || loading)
            .aria_label(if response {
                "Response body"
            } else {
                "Request JSON body"
            })
            .highlight_style_resolver(palette::styles(cx))
            .appearance(false)
            .bordered(false)
            .bg(if response {
                palette::get(cx).toolbar
            } else {
                palette::get(cx).background
            })
            .size_full()
    }
}

pub(super) struct HttpClient {
    method: &'static str,
    url: Entity<InputState>,
    request: JsonBody,
    body_format: BodyFormat,
    url_encoded_body: Entity<TextareaState>,
    multipart_body: Entity<TextareaState>,
    text_body: Entity<TextareaState>,
    binary_file: Option<(SharedString, SharedString)>,
    binary_focus: FocusHandle,
    binary_drop_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    binary_drag_over: bool,
    pick_binary: Function,
    clear_binary: Function,
    params: Entity<RequestFields>,
    headers: Entity<RequestFields>,
    defaults: RequestDefaults,
    timeout: Entity<InputState>,
    timeout_enabled: bool,
    timeout_inherited: bool,
    follow_redirects: bool,
    follow_redirects_inherited: bool,
    response_body: JsonBody,
    response_headers: Entity<ResponseHeaders>,
    response_text: Entity<TextareaState>,
    download_response: Function,
    response: Option<Response>,
    error_timeline: Vec<TimelineEvent>,
    timeline: ResponseTimeline,
    response_loading: bool,
    request_loading: bool,
    request_tab: RequestTab,
    response_tab: usize,
    request_fraction: f32,
    send: Function,
    tab_id: u32,
    pending: bool,
    sent_http_url: Option<String>,
    archiving: bool,
    error: Option<String>,
    _url_subscription: Subscription,
    _timeout_subscription: Subscription,
}

impl HttpClient {
    fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        tab_id: u32,
        callbacks: &ClientCallbacks,
        preview: Preview,
        defaults: &RequestDefaults,
    ) -> Self {
        let url = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("https://")
                .default_value(preview.url.clone())
        });
        let params = cx.new(|cx| RequestFields::query(url.clone(), window, cx));
        let headers =
            cx.new(|cx| RequestFields::request_headers(defaults, &preview.headers, window, cx));
        let timeout = cx.new(|cx| timeout_input(defaults.timeout_seconds, window, cx));
        let timeout_subscription = cx.subscribe(&timeout, |this, _, event, cx| {
            if matches!(event, InputEvent::Change) && this.timeout_inherited {
                this.timeout_inherited = false;
                cx.notify();
            }
        });
        let response_headers = cx.new(|cx| ResponseHeaders::new(&[], window, cx));
        let url_subscription = cx.subscribe_in(&url, window, |this, _, event, window, cx| {
            if matches!(event, InputEvent::Blur) {
                this.normalize_url(window, cx);
            }
        });
        let queue_edit = callbacks.queue_edit.clone();
        let tab = JsValue::from(tab_id);
        let on_edit: SyntaxEditHandler = Rc::new(move |revision, edit, text| {
            let payload = serde_json::json!({
                "revision": revision,
                "edit": {
                    "start": edit.start_byte, "old_end": edit.old_end_byte,
                    "replacement": text.slice(edit.start_byte..edit.new_end_byte).to_string(),
                    "start_row": edit.start_position.row, "start_column": edit.start_position.column,
                    "old_end_row": edit.old_end_position.row, "old_end_column": edit.old_end_position.column,
                    "new_end_row": edit.new_end_position.row, "new_end_column": edit.new_end_position.column,
                },
            });
            let _ = queue_edit.call2(
                &JsValue::UNDEFINED,
                &tab,
                &JsValue::from_str(&payload.to_string()),
            );
        });
        let request = JsonBody::new(Some(on_edit), window, cx);
        let response_body = JsonBody::new(None, window, cx);
        let load_request = callbacks.load_request.clone();
        request.editor.update(cx, |editor, cx| {
            editor.set_readonly(true, cx);
            editor.set_clipboard_paste_handler(move |editor, text| {
                editor.caret_count() == 1
                    && editor.selected_range() == (0..editor.text().len())
                    && text.trim_start().starts_with(['{', '['])
                    && load_request
                        .call2(
                            &JsValue::UNDEFINED,
                            &JsValue::from(tab_id),
                            &JsValue::from_str(text),
                        )
                        .is_ok()
            });
        });
        let has_preview = preview.response.is_some();
        let response_font = TextStyleRefinement {
            font_family: Some(cx.theme().font_family.clone()),
            ..Default::default()
        };
        Self {
            method: if has_preview { "POST" } else { "GET" },
            url,
            request,
            body_format: if has_preview {
                BodyFormat::Json
            } else {
                BodyFormat::NoBody
            },
            url_encoded_body: cx.new(|cx| TextareaState::new(window, cx).placeholder("name=value")),
            multipart_body: cx.new(|cx| TextareaState::new(window, cx).placeholder("name=value")),
            text_body: cx.new(|cx| TextareaState::new(window, cx)),
            binary_file: None,
            binary_focus: cx.focus_handle(),
            binary_drop_bounds: Rc::default(),
            binary_drag_over: false,
            pick_binary: callbacks.pick_binary.clone(),
            clear_binary: callbacks.clear_binary.clone(),
            params,
            headers,
            defaults: defaults.clone(),
            timeout,
            timeout_enabled: defaults.timeout_seconds.is_some(),
            timeout_inherited: true,
            follow_redirects: defaults.follow_redirects,
            follow_redirects_inherited: true,
            response_body,
            response_headers,
            response_text: cx.new(|cx| {
                window.with_text_style(Some(response_font), |window| TextareaState::new(window, cx))
            }),
            download_response: callbacks.download_response.clone(),
            response: None,
            error_timeline: Vec::new(),
            timeline: ResponseTimeline::new(cx),
            response_loading: false,
            request_loading: true,
            request_tab: RequestTab::Body,
            response_tab: 0,
            request_fraction: DEFAULT_REQUEST_FRACTION,
            send: callbacks.send.clone(),
            tab_id,
            pending: false,
            sent_http_url: None,
            archiving: false,
            error: None,
            _url_subscription: url_subscription,
            _timeout_subscription: timeout_subscription,
        }
    }

    pub(super) fn apply_defaults(
        &mut self,
        defaults: &RequestDefaults,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.defaults = defaults.clone();
        if self.timeout_inherited {
            self.timeout_enabled = defaults.timeout_seconds.is_some();
            set_timeout_input(&self.timeout, defaults.timeout_seconds, window, cx);
        }
        if self.follow_redirects_inherited {
            self.follow_redirects = defaults.follow_redirects;
        }
        self.headers.update(cx, |headers, cx| {
            headers.sync_defaults(defaults, window, cx)
        });
        cx.notify();
    }

    fn toggle_timeout_inheritance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.timeout_inherited {
            self.timeout_inherited = false;
        } else {
            self.timeout_enabled = self.defaults.timeout_seconds.is_some();
            set_timeout_input(&self.timeout, self.defaults.timeout_seconds, window, cx);
            self.timeout_inherited = true;
        }
        cx.notify();
    }

    fn toggle_redirect_inheritance(&mut self, cx: &mut Context<Self>) {
        if self.follow_redirects_inherited {
            self.follow_redirects_inherited = false;
        } else {
            self.follow_redirects = self.defaults.follow_redirects;
            self.follow_redirects_inherited = true;
        }
        cx.notify();
    }

    fn normalize_url(&self, window: &mut Window, cx: &mut Context<Self>) {
        let value = self.url.read(cx).value();
        if let Some(url) = url_with_default_scheme(value.trim()) {
            self.url
                .update(cx, |input, cx| input.set_value(url, window, cx));
        }
    }

    fn send_request(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.send_disabled() {
            return;
        }
        self.normalize_url(window, cx);
        let text_body = self
            .body_textarea()
            .map(|field| field.read(cx).value())
            .unwrap_or_default();
        let request = self.request_snapshot(cx);
        self.sent_http_url = request
            .url
            .get(..7)
            .filter(|scheme| scheme.eq_ignore_ascii_case("http://"))
            .map(|_| request.url.clone());
        let had_error = self.error.is_some();
        self.error = None;
        self.error_timeline.clear();
        self.pending = true;
        let payload = serde_json::to_string(&request).expect("Serialize HTTP request");
        if self
            .send
            .call3(
                &JsValue::UNDEFINED,
                &JsValue::from(self.tab_id),
                &JsValue::from_str(&payload),
                &JsValue::from_str(&text_body),
            )
            .is_err()
        {
            self.pending = false;
            self.sent_http_url = None;
            self.error = Some("Could not send request".into());
        }
        if had_error || self.error.is_some() {
            self.sync_timeline(true, window, cx);
        }
        cx.notify();
    }

    fn request_snapshot(&self, cx: &App) -> Request {
        Request {
            method: self.method,
            url: self.url.read(cx).value().trim().to_owned(),
            headers: self.headers.read(cx).header_text(cx),
            body_format: self.body_format,
            timeout_ms: u64::from(
                timeout_seconds(&self.timeout, self.timeout_enabled, cx).unwrap_or(0),
            ) * 1000,
            follow_redirects: self.follow_redirects,
        }
    }

    fn send_disabled(&self) -> bool {
        self.pending
            || self.archiving
            || (self.body_format == BodyFormat::Json && self.request_loading)
    }

    fn body_textarea(&self) -> Option<&Entity<TextareaState>> {
        match self.body_format {
            BodyFormat::UrlEncoded => Some(&self.url_encoded_body),
            BodyFormat::MultiPart => Some(&self.multipart_body),
            BodyFormat::Text => Some(&self.text_body),
            _ => None,
        }
    }

    pub(super) fn set_binary_file(&mut self, name: String, size: u64, cx: &mut Context<Self>) {
        self.binary_file = Some((name.into(), format_byte_size(size).into()));
        self.binary_drag_over = false;
        cx.notify();
    }

    fn set_binary_drag_over(&mut self, active: bool, cx: &mut Context<Self>) {
        if self.binary_drag_over != active {
            self.binary_drag_over = active;
            cx.notify();
        }
    }

    fn accepts_binary_drop(&self, position: Point<Pixels>, cx: &App) -> bool {
        self.request_tab == RequestTab::Body
            && self.body_format == BodyFormat::Binary
            && !self.response_headers.read(cx).has_modal_open()
            && self
                .binary_drop_bounds
                .get()
                .is_some_and(|bounds| bounds.contains(&position))
    }

    fn accepts_binary_paste(&self, window: &Window, cx: &App) -> bool {
        self.request_tab == RequestTab::Body
            && self.body_format == BodyFormat::Binary
            && !self.response_headers.read(cx).has_modal_open()
            && self.binary_focus.contains_focused(window, cx)
    }

    fn clear_binary_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.binary_file = None;
        self.binary_focus.focus(window, cx);
        let _ = self
            .clear_binary
            .call1(&JsValue::UNDEFINED, &JsValue::from(self.tab_id));
        cx.notify();
    }

    pub(super) fn begin_response(&mut self, cx: &mut Context<Self>) {
        self.pending = true;
        self.response_loading = true;
        cx.notify();
    }

    fn sync_timeline(&mut self, reset_scroll: bool, window: &mut Window, cx: &mut Context<Self>) {
        let events = if self.error.is_some() {
            &self.error_timeline[..]
        } else {
            self.response
                .as_ref()
                .map_or(&[][..], |response| &response.timeline)
        };
        self.timeline.set_events(events, reset_scroll, window, cx);
    }

    pub(super) fn finish_request_error(
        &mut self,
        error: String,
        timeline: Vec<TimelineEvent>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pending = false;
        self.response_loading = false;
        self.sent_http_url = None;
        self.error = Some(error);
        self.error_timeline = timeline;
        self.sync_timeline(true, window, cx);
        cx.notify();
    }

    pub(super) fn set_request_loading(&mut self, loading: bool, cx: &mut Context<Self>) {
        if self.request_loading == loading {
            return;
        }
        self.request_loading = loading;
        self.request
            .editor
            .update(cx, |editor, cx| editor.set_readonly(loading, cx));
        cx.notify();
    }

    fn body_mut(&mut self, response: bool) -> &mut JsonBody {
        if response {
            &mut self.response_body
        } else {
            &mut self.request
        }
    }

    pub(super) fn prepare_body(&mut self, response: bool, id: u32, view: String, cx: &App) {
        let body = self.body_mut(response);
        let preparation = body.editor.read(cx).prepare_value(SharedString::from(view));
        body.preparation = Some((id, preparation));
    }

    pub(super) fn advance_body_preparation(
        &mut self,
        response: bool,
        id: u32,
        cx: &mut App,
    ) -> i32 {
        let body = self.body_mut(response);
        let Some((_, preparation)) = body
            .preparation
            .as_mut()
            .filter(|(current, _)| *current == id)
        else {
            return -1;
        };
        i32::from(
            body.editor
                .update(cx, |editor, cx| editor.advance_preparation(preparation, cx)),
        )
    }

    pub(super) fn body_preparation_progress(&mut self, response: bool, id: u32) -> Vec<u32> {
        self.body_mut(response)
            .preparation
            .as_ref()
            .filter(|(current, _)| *current == id)
            .map(|(_, preparation)| {
                vec![
                    preparation.processed_bytes() as u32,
                    preparation.text().len() as u32,
                ]
            })
            .unwrap_or_default()
    }

    pub(super) fn cancel_body_preparation(&mut self, response: bool, id: u32) {
        let body = self.body_mut(response);
        if body
            .preparation
            .as_ref()
            .is_some_and(|(current, _)| *current == id)
        {
            body.preparation = None;
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn install_body(
        &mut self,
        response: bool,
        metadata: &str,
        highlights: &[u32],
        folds: &[u32],
        brackets: &[u32],
        diagnostics: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let response_metadata = if response {
            let Ok(metadata) = serde_json::from_str::<Response>(metadata) else {
                return false;
            };
            Some(metadata)
        } else {
            None
        };
        let body = self.body_mut(response);
        let Some((_, preparation)) = body.preparation.take() else {
            return false;
        };
        let installed = body.editor.update(cx, |editor, cx| {
            body.syntax.borrow_mut().installing = true;
            if !editor.set_prepared_value(preparation, window, cx) {
                body.syntax.borrow_mut().installing = false;
                return false;
            }
            body.syntax.borrow_mut().prepare_open();
            body.syntax.borrow_mut().installing = false;
            match body.install_syntax(editor, highlights, folds, brackets, diagnostics, cx) {
                Ok(()) => true,
                Err(error) => {
                    log::error!("Cannot install HTTP body syntax: {error}");
                    false
                }
            }
        });
        if installed && let Some(response) = response_metadata {
            self.finish_response(response, window, cx);
        }
        installed
    }

    pub(super) fn install_non_json_response(
        &mut self,
        metadata: &str,
        body: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Ok(response) = serde_json::from_str::<Response>(metadata) else {
            return false;
        };
        self.response_text
            .update(cx, |text, cx| text.set_value(body, window, cx));
        self.finish_response(response, window, cx);
        true
    }

    fn finish_response(&mut self, response: Response, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(sent_url) = self.sent_http_url.take()
            && response.https_upgrade_url.is_some()
            && self.url.read(cx).value().trim() == sent_url
            && let Some((_, remainder)) = sent_url.split_once("://")
        {
            self.url.update(cx, |input, cx| {
                input.replace_all(format!("https://{remainder}"), window, cx)
            });
        }
        self.response_headers.update(cx, |headers, cx| {
            headers.set_headers(&response.headers, window, cx)
        });
        self.response = Some(response);
        self.sync_timeline(true, window, cx);
        self.pending = false;
        self.response_loading = false;
        cx.notify();
    }

    pub(super) fn apply_syntax(
        &self,
        revision: u32,
        highlights: &[u32],
        folds: &[u32],
        brackets: &[u32],
        diagnostics: &str,
        cx: &mut Context<Self>,
    ) {
        let body = &self.request;
        if body.syntax.borrow().revision != revision {
            return;
        }
        body.editor.update(cx, |editor, cx| {
            if let Err(error) =
                body.install_syntax(editor, highlights, folds, brackets, diagnostics, cx)
            {
                log::error!("Cannot install JSON syntax: {error}");
            }
        });
    }

    fn request_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let send_disabled = self.send_disabled();
        let palette = palette::get(cx);
        let send_color = if send_disabled {
            cx.theme().muted_foreground.opacity(0.5)
        } else {
            palette.foreground
        };
        let owner = cx.weak_entity();
        exchange_header()
            .bg(palette.background)
            .border_1()
            .rounded_sm()
            .border_color(cx.theme().border.opacity(0.65))
            .capture_action(cx.listener(|this, _: &Enter, window, cx| {
                cx.stop_propagation();
                this.send_request(window, cx);
            }))
            .child(
                field_button("http-method", cx)
                    .with_size(Size::XSmall)
                    .px_3()
                    .cursor_pointer()
                    .child(field_button_label(self.method, cx.theme().green))
                    .accessibility_label(format!("HTTP method, {}", self.method))
                    .font_family(cx.theme().mono_font_family.clone())
                    .dropdown_menu(move |menu, _, _| {
                        METHODS.iter().fold(menu, |menu, &method| {
                            let owner = owner.clone();
                            menu.item(PopupMenuItem::new(method).on_click(move |_, _, cx| {
                                let _ = owner.update(cx, |this, cx| {
                                    this.method = method;
                                    cx.notify();
                                });
                            }))
                        })
                    }),
            )
            .child(
                div().flex_1().min_w_0().child(
                    Input::new(&self.url)
                        .with_size(Size::XSmall)
                        .appearance(false)
                        .aria_label("Request URL")
                        .font_family(cx.theme().mono_font_family.clone())
                        .w_full(),
                ),
            )
            .child(
                field_button("send-request", cx)
                    .with_size(Size::Small)
                    .w_auto()
                    .px_3()
                    .disabled(send_disabled)
                    .when(!send_disabled, |button| button.cursor_pointer())
                    .child(field_button_icon(
                        include_bytes!("../../../assets/icons/run.svg"),
                        rems(0.875),
                        send_color,
                        !send_disabled,
                    ))
                    .tooltip_with_action("Send request", &SendRequest, Some(REQUEST_PANE_CONTEXT))
                    .accessibility_label("Send request")
                    .on_click(cx.listener(|this, _, window, cx| this.send_request(window, cx))),
            )
    }

    fn response_status(&self, cx: &App) -> impl IntoElement {
        let text = if self.body_format == BodyFormat::Json && self.request_loading {
            "Preparing request body…".to_owned()
        } else if self.response_loading {
            "Preparing response…".to_owned()
        } else if self.pending {
            "Sending request…".to_owned()
        } else if let Some(error) = &self.error {
            error.clone()
        } else if let Some(response) = &self.response {
            format!(
                "{} {}  •  {} ms  •  {} B",
                response.status, response.status_text, response.elapsed_ms, response.bytes
            )
        } else {
            "No response yet".to_owned()
        };
        exchange_header()
            .gap_3()
            .px_3()
            .font_family(cx.theme().mono_font_family.clone())
            .text_sm()
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .when(self.error.is_some(), |element| {
                        element.text_color(cx.theme().danger)
                    })
                    .child(text),
            )
            .when(
                self.response
                    .as_ref()
                    .is_some_and(|response| response.preview),
                |header| {
                    header.child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("Preview"),
                    )
                },
            )
    }

    fn tabs(&self, response: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let id = if response {
            "response-tabs"
        } else {
            "request-tabs"
        };
        div()
            .id(id)
            .role(Role::TabList)
            .flex()
            .items_center()
            .h_9()
            .flex_none()
            .overflow_x_scroll()
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(rems(0.125))
                    .px_1()
                    .py(rems(0.125))
                    .map(|row| {
                        if response {
                            row.children(RESPONSE_TABS.iter().enumerate().map(|(index, &label)| {
                                segment_button((id, index), self.response_tab == index, cx)
                                    .label(label)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        if this.response_tab != index {
                                            this.timeline.clear_selection_if_focused(window, cx);
                                            this.response_tab = index;
                                            cx.notify();
                                        }
                                    }))
                            }))
                        } else {
                            let request_tab = |id, label, tab| {
                                segment_button(id, self.request_tab == tab, cx)
                                    .label(label)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.request_tab = tab;
                                        cx.notify();
                                    }))
                            };
                            row.min_w_full()
                                .justify_between()
                                .gap_4()
                                .child(
                                    div()
                                        .flex()
                                        .flex_none()
                                        .items_center()
                                        .gap(rems(0.125))
                                        .child(request_tab(
                                            "request-params",
                                            "Params",
                                            RequestTab::Params,
                                        ))
                                        .child(self.body_dropdown(cx)),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_none()
                                        .items_center()
                                        .gap(rems(0.125))
                                        .child(request_tab(
                                            "request-headers",
                                            "Headers",
                                            RequestTab::Headers,
                                        ))
                                        .child(request_tab(
                                            "request-settings",
                                            "Settings",
                                            RequestTab::Settings,
                                        )),
                                )
                        }
                    }),
            )
    }

    fn body_dropdown(&self, cx: &Context<Self>) -> AnyElement {
        let owner = cx.weak_entity();
        let format = self.body_format;
        let selected = self.request_tab == RequestTab::Body;
        let color = if selected {
            cx.theme().tab_active_foreground
        } else {
            palette::get(cx).ui_foreground
        };
        let button = segment_button("request-body", selected, cx)
            .label(format.label())
            .accessibility_label(format!("Request body format, {}", format.label()))
            .child(
                Icon::default()
                    .data(include_bytes!("../../../assets/icons/chevron-down.svg"))
                    .with_size(Size::XSmall)
                    .text_color(color),
            );
        if !selected {
            return button
                .on_click(cx.listener(|this, _, window, cx| {
                    this.request_tab = RequestTab::Body;
                    if this.body_format == BodyFormat::Binary {
                        this.binary_focus.focus(window, cx);
                    }
                    cx.notify();
                }))
                .into_any_element();
        }
        button
            .dropdown_menu(move |menu, _, _| {
                BodyFormat::OPTIONS
                    .into_iter()
                    .fold(menu.with_size(Size::Small), |menu, option| {
                        let owner = owner.clone();
                        let item = checked_menu_item(option.label(), option == format);
                        menu.item(item.on_click(move |_, window, cx| {
                            let _ = owner.update(cx, |this, cx| {
                                if this.body_format != option {
                                    this.body_format = option;
                                    this.binary_drag_over = false;
                                    this.headers.update(cx, |headers, cx| {
                                        headers.set_content_type(option.content_type(), window, cx);
                                    });
                                    if option == BodyFormat::Binary {
                                        let focus = this.binary_focus.clone();
                                        window.on_next_frame(move |window, cx| {
                                            focus.focus(window, cx)
                                        });
                                    }
                                    cx.notify();
                                }
                            });
                        }))
                    })
            })
            .into_any_element()
    }

    fn request_content(&self, cx: &mut Context<Self>) -> AnyElement {
        match self.request_tab {
            RequestTab::Params => self.params.clone().into_any_element(),
            RequestTab::Headers => div()
                .size_full()
                .max_w(rems(DETAIL_PANE_MAX_WIDTH_REMS))
                .mx_auto()
                .child(self.headers.clone())
                .into_any_element(),
            RequestTab::Settings => self.request_settings(cx).into_any_element(),
            RequestTab::Body => self.request_body_content(cx),
        }
    }

    fn request_settings(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let owner = cx.weak_entity();
        request_settings_controls(
            &self.timeout,
            self.timeout_enabled,
            self.follow_redirects,
            move |enabled, _, cx| {
                let _ = owner.update(cx, |this, cx| {
                    if this.timeout_enabled != enabled {
                        this.timeout_enabled = enabled;
                        this.timeout_inherited = false;
                        cx.notify();
                    }
                });
            },
            cx.listener(|this, _, _, cx| {
                this.follow_redirects = !this.follow_redirects;
                this.follow_redirects_inherited = false;
                cx.notify();
            }),
            Some(SettingInheritance {
                inherited: self.timeout_inherited,
                on_click: Box::new(cx.listener(|this, _, window, cx| {
                    this.toggle_timeout_inheritance(window, cx);
                })),
            }),
            Some(SettingInheritance {
                inherited: self.follow_redirects_inherited,
                on_click: Box::new(cx.listener(|this, _, _, cx| {
                    this.toggle_redirect_inheritance(cx);
                })),
            }),
            cx,
        )
        .max_w(rems(DETAIL_PANE_MAX_WIDTH_REMS))
        .mx_auto()
        .p_4()
    }

    fn request_body_content(&self, cx: &mut Context<Self>) -> AnyElement {
        if let Some(field) = self.body_textarea() {
            return Textarea::new(field)
                .appearance(false)
                .size_full()
                .font_family(cx.theme().mono_font_family.clone())
                .aria_label(if self.body_format == BodyFormat::Text {
                    "Text request body"
                } else {
                    "Request body fields, one name=value per line"
                })
                .into_any_element();
        }
        match self.body_format {
            BodyFormat::Json => self
                .request
                .render(false, self.request_loading, cx)
                .into_any_element(),
            BodyFormat::Binary => {
                let icon_color = cx.theme().muted_foreground;
                let highlight = palette::get(cx).selection;
                let drop_bounds = self.binary_drop_bounds.clone();
                let radius = cx.theme().radius;
                let content = div()
                    .id("binary-body")
                    .size_full()
                    .min_h_0()
                    .overflow_y_scroll()
                    .pt(rems(3.0))
                    .pb_2()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_1();
                let content = if let Some((name, size)) = self.binary_file.clone() {
                    let full_name = name.clone();
                    content
                        .child(
                            div()
                                .id("binary-file-name")
                                .max_w_full()
                                .truncate()
                                .text_sm()
                                .tooltip(move |window, cx| {
                                    Tooltip::new(full_name.clone()).build(window, cx)
                                })
                                .child(name),
                        )
                        .child(div().text_xs().text_color(icon_color).child(size))
                        .child(
                            action_button("remove-binary-file", cx)
                                .with_size(Size::Small)
                                .tooltip("Remove file")
                                .accessibility_label("Remove file")
                                .icon(small_icon(
                                    include_bytes!("../../../assets/icons/delete.svg"),
                                    icon_color,
                                ))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.clear_binary_file(window, cx)
                                })),
                        )
                } else {
                    let tab_id = self.tab_id;
                    let pick_binary = self.pick_binary.clone();
                    content.child(
                        action_button("request-binary-file", cx)
                            .size(rems(3.0))
                            .p_0()
                            .tooltip("Choose, drop, or paste file")
                            .accessibility_label("Choose file")
                            .child(
                                Icon::default()
                                    .data(include_bytes!("../../../assets/icons/open.svg"))
                                    .size(rems(2.0))
                                    .text_color(icon_color),
                            )
                            .on_click(move |_, _, _| {
                                let _ =
                                    pick_binary.call1(&JsValue::UNDEFINED, &JsValue::from(tab_id));
                            }),
                    )
                };
                div()
                    .id("binary-body-frame")
                    .relative()
                    .track_focus(&self.binary_focus)
                    .size_full()
                    .min_h_0()
                    .rounded(radius)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            this.binary_focus.focus(window, cx);
                        }),
                    )
                    .child(
                        canvas(
                            move |bounds, _, _| {
                                drop_bounds.set(Some(bounds));
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .inset_0(),
                    )
                    .when(self.binary_drag_over, |frame| {
                        frame.child(
                            div()
                                .absolute()
                                .inset_0()
                                .rounded(radius)
                                .border_1()
                                .border_dashed()
                                .border_color(highlight)
                                .bg(highlight.opacity(0.12)),
                        )
                    })
                    .child(content)
                    .into_any_element()
            }
            _ => div().into_any_element(),
        }
    }

    fn response_content(&self, cx: &mut Context<Self>) -> AnyElement {
        match self.response_tab {
            0 => match self
                .response
                .as_ref()
                .filter(|response| !self.response_loading && response.bytes > 0)
                .map(|response| &response.format)
            {
                Some(ResponseFormat::Json) => self
                    .response_body
                    .render(true, false, cx)
                    .into_any_element(),
                Some(ResponseFormat::Text) => Textarea::new(&self.response_text)
                    .appearance(false)
                    .readonly(true)
                    .size_full()
                    .aria_label("Response text")
                    .into_any_element(),
                Some(ResponseFormat::Binary) => {
                    let download = self.download_response.clone();
                    let id = self.tab_id;
                    div()
                        .p_3()
                        .child(
                            Button::new("download-response")
                                .with_size(Size::Small)
                                .cursor_pointer()
                                .icon(
                                    Icon::default()
                                        .data(include_bytes!("../../../assets/icons/download.svg")),
                                )
                                .label("Download response")
                                .on_click(move |_, _, _| {
                                    let _ = download.call1(&JsValue::UNDEFINED, &JsValue::from(id));
                                }),
                        )
                        .into_any_element()
                }
                None => div().into_any_element(),
            },
            1 => self.response_headers.clone().into_any_element(),
            2 => detail("Cookies", "No response cookies").into_any_element(),
            _ => self.response_timeline(cx).into_any_element(),
        }
    }

    fn response_timeline(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let mono = cx.theme().mono_font_family.clone();
        let label_font = cx.theme().font_family.clone();
        let (request_foreground, response_foreground) = timeline_foregrounds();
        let summary_foreground = cx.theme().foreground.opacity(0.5);
        let separator = cx.theme().border.opacity(0.5);
        div()
            .id("response-timeline-pane")
            .key_context(TIMELINE_CONTEXT)
            .track_focus(&self.timeline.focus)
            .on_action(cx.listener(|this, _: &Copy, window, cx| {
                let text = this.timeline.selected_text(window, cx);
                if text.is_empty() {
                    cx.propagate();
                    return;
                }
                cx.write_to_clipboard(ClipboardItem::new_string(text));
            }))
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .text_sm()
            .child(
                timeline_row(
                    div().child(TimelineRowHandles::cell(
                        "timeline-header-event",
                        &self.timeline.header.label,
                        0,
                        cx,
                    )),
                    TimelineRowHandles::cell(
                        "timeline-header-time",
                        &self.timeline.header.time,
                        1,
                        cx,
                    ),
                    TimelineRowHandles::cell(
                        "timeline-header-cumulative",
                        &self.timeline.header.cumulative,
                        2,
                        cx,
                    ),
                )
                .py_1()
                .text_xs()
                .border_b_1()
                .border_color(separator)
                .text_color(summary_foreground),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .id("response-timeline")
                            .flex_1()
                            .min_h_0()
                            .track_scroll(&self.timeline.scroll)
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .children(self.timeline.rows.iter().enumerate().map(|(index, row)| {
                                let order = (index as u64 + 1) * 3;
                                let foreground = match row.side {
                                    TimelineSide::Request => request_foreground,
                                    TimelineSide::Response => response_foreground,
                                };
                                timeline_row(
                                    div().font_family(label_font.clone()).italic().child(
                                        TimelineRowHandles::cell(
                                            ("timeline-event-label", index),
                                            &row.handles.label,
                                            order,
                                            cx,
                                        ),
                                    ),
                                    TimelineRowHandles::cell(
                                        ("timeline-event-time", index),
                                        &row.handles.time,
                                        order + 1,
                                        cx,
                                    ),
                                    TimelineRowHandles::cell(
                                        ("timeline-event-cumulative", index),
                                        &row.handles.cumulative,
                                        order + 2,
                                        cx,
                                    ),
                                )
                                .text_color(foreground)
                                .font_family(mono.clone())
                            }))
                            .when_some(self.timeline.total.as_ref(), |list, total| {
                                let order = (self.timeline.rows.len() as u64 + 1) * 3;
                                list.child(
                                    timeline_row(
                                        div().font_family(label_font.clone()).text_xs().child(
                                            TimelineRowHandles::cell(
                                                "timeline-total-label",
                                                &total.label,
                                                order,
                                                cx,
                                            ),
                                        ),
                                        TimelineRowHandles::cell(
                                            "timeline-total-time",
                                            &total.time,
                                            order + 1,
                                            cx,
                                        ),
                                        TimelineRowHandles::cell(
                                            "timeline-total-cumulative",
                                            &total.cumulative,
                                            order + 2,
                                            cx,
                                        ),
                                    )
                                    .mt_1()
                                    .py_1()
                                    .font_family(mono.clone())
                                    .border_t_1()
                                    .border_color(separator)
                                    .text_color(summary_foreground),
                                )
                            }),
                    )
                    .vertical_scrollbar(&self.timeline.scroll),
            )
    }

    fn editor_panel(&self, response: bool, cx: &mut Context<Self>) -> Div {
        let content = if response {
            self.response_content(cx)
        } else {
            self.request_content(cx)
        };
        div()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .pb_2()
            .child(self.tabs(response, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .overflow_hidden()
                    .px_2()
                    .child(content),
            )
    }

    fn resize_panes(&mut self, fraction: f32, cx: &mut Context<Self>) {
        let fraction = fraction.clamp(MIN_PANE_FRACTION, 1.0 - MIN_PANE_FRACTION);
        if self.request_fraction != fraction {
            self.request_fraction = fraction;
            cx.notify();
        }
    }
}

impl Render for HttpClient {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = palette::get(cx);
        let tab_background = workspace_tab_background(self.tab_id as usize);
        let request_background = palette.background;
        let response_background = palette.toolbar;
        let divider_highlight = palette.selection;
        let radius = cx.theme().radius;
        div()
            .id(("request-workspace", self.tab_id as usize))
            .size_full()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .flex()
            .overflow_hidden()
            .p_2()
            .bg(tab_background)
            .role(Role::TabPanel)
            .aria_label("Request")
            .on_drag_move(
                cx.listener(|this, event: &DragMoveEvent<PaneResize>, window, cx| {
                    let inset = rems(0.5).to_pixels(window.rem_size());
                    let width = event.bounds.size.width - inset * 2.0;
                    if width > px(0.0) {
                        let offset = event.event.position.x - event.bounds.left() - inset * 1.5;
                        this.resize_panes(offset / width, cx);
                    }
                }),
            )
            .child(
                div()
                    .id("request-pane")
                    .focusable()
                    .key_context(REQUEST_PANE_CONTEXT)
                    .capture_action(cx.listener(|this, _: &SendRequest, window, cx| {
                        cx.stop_propagation();
                        this.send_request(window, cx);
                    }))
                    .w(relative(self.request_fraction))
                    .min_w_0()
                    .h_full()
                    .flex_none()
                    .flex()
                    .flex_col()
                    .child(self.request_bar(cx))
                    .child(
                        self.editor_panel(false, cx)
                            .rounded(radius)
                            .bg(request_background),
                    ),
            )
            .child(
                div()
                    .id("request-response-divider")
                    .role(Role::Splitter)
                    .aria_label("Resize request and response panes")
                    .tab_index(0)
                    .w_2()
                    .h_full()
                    .flex_none()
                    .cursor(CursorStyle::ResizeLeftRight)
                    .hover(move |style| style.bg(divider_highlight))
                    .focus_visible(move |style| style.bg(divider_highlight))
                    .on_drag(PaneResize, |_, _, _, cx| cx.new(|_| EmptyView))
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                        let delta = match event.keystroke.key.as_str() {
                            "left" => -0.025,
                            "right" => 0.025,
                            _ => return,
                        };
                        cx.stop_propagation();
                        this.resize_panes(this.request_fraction + delta, cx);
                    })),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .rounded(radius)
                    .overflow_hidden()
                    .bg(response_background)
                    .child(self.response_status(cx))
                    .child(self.editor_panel(true, cx)),
            )
    }
}

struct WorkspaceTab {
    id: u32,
    client: Entity<HttpClient>,
    archiving: bool,
    displayed_left: Cell<Pixels>,
    _url_subscription: Subscription,
}

struct WorkspaceTabDrag {
    id: u32,
    start: Point<Pixels>,
    moved: bool,
    start_left: Pixels,
    start_scroll: Pixels,
    left: Pixels,
}

const WORKSPACE_TAB_WIDTH: f32 = 10.0;
const WORKSPACE_TAB_GAP: f32 = 0.25;
const WORKSPACE_TAB_SLIDE: Duration = Duration::from_millis(150);

pub(super) struct Workspace {
    tabs: Vec<WorkspaceTab>,
    next_tab_id: u32,
    active_tab: u32,
    tab_scroll: ScrollHandle,
    tab_drag: Option<WorkspaceTabDrag>,
    raised_tab: Option<u32>,
    callbacks: ClientCallbacks,
    defaults: RequestDefaults,
    persist_request_defaults: Function,
    settings_dialog: Option<Entity<RequestDefaultsDialog>>,
    settings_previous_focus: Option<FocusHandle>,
}

impl Workspace {
    pub(super) fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        callbacks: ClientCallbacks,
        preview: Preview,
        defaults: RequestDefaults,
        persist_request_defaults: Function,
    ) -> Self {
        let first_tab = cx.new(|cx| HttpClient::new(window, cx, 0, &callbacks, preview, &defaults));
        Self {
            tabs: vec![WorkspaceTab {
                id: 0,
                _url_subscription: Self::subscribe_to_url(&first_tab, cx),
                client: first_tab,
                archiving: false,
                displayed_left: Cell::new(px(0.)),
            }],
            next_tab_id: 1,
            active_tab: 0,
            tab_scroll: ScrollHandle::new(),
            tab_drag: None,
            raised_tab: None,
            callbacks,
            defaults,
            persist_request_defaults,
            settings_dialog: None,
            settings_previous_focus: None,
        }
    }

    pub(super) fn save_defaults(
        &mut self,
        defaults: RequestDefaults,
        persist_to_storage: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.defaults = defaults;
        self.apply_defaults_to_tabs(window, cx);
        if !persist_to_storage {
            return true;
        }
        serde_json::to_string(&self.defaults).is_ok_and(|value| {
            self.persist_request_defaults
                .call1(&JsValue::UNDEFINED, &JsValue::from_str(&value))
                .is_ok()
        })
    }

    fn apply_defaults_to_tabs(&self, window: &mut Window, cx: &mut Context<Self>) {
        for tab in &self.tabs {
            tab.client.update(cx, |client, cx| {
                client.apply_defaults(&self.defaults, window, cx)
            });
        }
    }

    fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(dialog) = &self.settings_dialog {
            let focus = dialog.read(cx).focus_handle(cx);
            window.on_next_frame(move |window, cx| focus.focus(window, cx));
            return;
        }
        let owner = cx.weak_entity();
        let dialog = cx.new(|cx| RequestDefaultsDialog::new(&self.defaults, owner, window, cx));
        self.settings_previous_focus = window.focused(cx);
        let focus = dialog.read(cx).focus_handle(cx);
        self.settings_dialog = Some(dialog);
        window.on_next_frame(move |window, cx| focus.focus(window, cx));
        cx.notify();
    }

    pub(super) fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_dialog = None;
        if self.defaults.clear_live_header_ids() {
            self.apply_defaults_to_tabs(window, cx);
        }
        if let Some(previous) = self.settings_previous_focus.take() {
            previous.focus(window, cx);
        }
        cx.notify();
    }

    pub(super) fn tab(&self, id: u32) -> Option<Entity<HttpClient>> {
        self.tabs
            .iter()
            .find(|tab| tab.id == id)
            .map(|tab| tab.client.clone())
    }

    pub(super) fn selected_response_text(&self, window: &mut Window, cx: &mut App) -> String {
        if self.settings_dialog.is_some() {
            return String::new();
        }
        let client = self.tab(self.active_tab).expect("Active request tab");
        let (response_tab, headers) = {
            let tab = client.read(cx);
            (tab.response_tab, tab.response_headers.clone())
        };
        match response_tab {
            1 => headers.update(cx, |headers, cx| headers.selected_text(window, cx)),
            3 => client.update(cx, |tab, cx| tab.timeline.selected_text(window, cx)),
            _ => String::new(),
        }
    }

    pub(super) fn binary_drop_tab(&self, position: Point<Pixels>, cx: &App) -> Option<u32> {
        if self.settings_dialog.is_some() {
            return None;
        }
        let client = self.tab(self.active_tab)?;
        client
            .read(cx)
            .accepts_binary_drop(position, cx)
            .then_some(self.active_tab)
    }

    pub(super) fn binary_paste_tab(&self, window: &Window, cx: &App) -> Option<u32> {
        if self.settings_dialog.is_some() {
            return None;
        }
        let client = self.tab(self.active_tab)?;
        client
            .read(cx)
            .accepts_binary_paste(window, cx)
            .then_some(self.active_tab)
    }

    pub(super) fn set_binary_drag_over(&mut self, target: Option<u32>, cx: &mut Context<Self>) {
        for tab in &self.tabs {
            let tab_active = target == Some(tab.id);
            tab.client
                .update(cx, |client, cx| client.set_binary_drag_over(tab_active, cx));
        }
    }

    fn subscribe_to_url(tab: &Entity<HttpClient>, cx: &mut Context<Self>) -> Subscription {
        let url = tab.read(cx).url.clone();
        cx.subscribe(&url, |_, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })
    }

    fn select_tab(&mut self, id: u32, cx: &mut Context<Self>) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        self.active_tab = id;
        self.tab_scroll.scroll_to_item(index);
        cx.notify();
    }

    fn reorder_tab(&mut self, source: u32, target: u32, cx: &mut Context<Self>) {
        let source = self.tabs.iter().position(|tab| tab.id == source);
        let target = self.tabs.iter().position(|tab| tab.id == target);
        if let (Some(source), Some(target)) = (source, target)
            && source != target
        {
            let tab = self.tabs.remove(source);
            self.tabs.insert(target, tab);
            cx.notify();
        }
    }

    fn start_tab_drag(&mut self, id: u32, position: Point<Pixels>) {
        let Some(tab) = self.tabs.iter().find(|tab| tab.id == id) else {
            return;
        };
        let left = tab.displayed_left.get();
        self.tab_drag = Some(WorkspaceTabDrag {
            id,
            start: position,
            moved: false,
            start_left: left,
            start_scroll: self.tab_scroll.offset().x,
            left,
        });
    }

    fn drag_tab_to(
        &mut self,
        position: Point<Pixels>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(drag) = &mut self.tab_drag else {
            return false;
        };
        let delta = position.x - drag.start.x;
        if !drag.moved && f32::from(delta).abs() <= 4.0 {
            return false;
        }
        drag.moved = true;
        let source = drag.id;
        let pitch = window.rem_size() * (WORKSPACE_TAB_WIDTH + WORKSPACE_TAB_GAP);
        let last = self.tabs.len().saturating_sub(1);
        let left = (drag.start_left + delta + drag.start_scroll - self.tab_scroll.offset().x)
            .clamp(px(0.), pitch * last as f32);
        drag.left = left;
        self.raised_tab = Some(source);
        let target = (f32::from(left) / f32::from(pitch)).round() as usize;
        if let Some(tab) = self.tabs.get(target) {
            self.reorder_tab(source, tab.id, cx);
        }
        cx.notify();
        true
    }

    fn finish_tab_drag(&mut self, cx: &mut Context<Self>) {
        if self.tab_drag.take().is_some_and(|drag| drag.moved) {
            cx.notify();
        }
    }

    fn tab_drag_events(&self, cx: &Context<Self>) -> impl IntoElement + use<> {
        let owner = cx.weak_entity();
        canvas(
            |_, _, _| {},
            move |_, _, window, _| {
                let moving = owner.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                    if phase == DispatchPhase::Capture {
                        let _ = moving.update(cx, |this, cx| {
                            if event.pressed_button == Some(MouseButton::Left) {
                                this.drag_tab_to(event.position, window, cx);
                            } else {
                                this.finish_tab_drag(cx);
                            }
                        });
                    }
                });
                window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                    if phase == DispatchPhase::Capture && event.button == MouseButton::Left {
                        let dragged = owner
                            .update(cx, |this, cx| {
                                let dragged = this.drag_tab_to(event.position, window, cx);
                                this.finish_tab_drag(cx);
                                dragged
                            })
                            .unwrap_or(false);
                        if dragged {
                            cx.stop_propagation();
                        }
                    }
                });
            },
        )
        .absolute()
        .inset_0()
    }

    fn add_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        let tab = cx.new(|cx| {
            HttpClient::new(
                window,
                cx,
                id,
                &self.callbacks,
                Preview::default(),
                &self.defaults,
            )
        });
        tab.read(cx).url.read(cx).focus_handle(cx).focus(window, cx);
        self.tabs.push(WorkspaceTab {
            id,
            _url_subscription: Self::subscribe_to_url(&tab, cx),
            client: tab,
            archiving: false,
            displayed_left: Cell::new(px(0.)),
        });
        self.select_tab(id, cx);
        if let Err(error) = self.callbacks.load_request.call2(
            &JsValue::UNDEFINED,
            &JsValue::from(id),
            &JsValue::from_str(""),
        ) {
            self.tab(id)
                .expect("New request tab")
                .update(cx, |tab, cx| {
                    tab.set_request_loading(false, cx);
                    tab.finish_request_error(
                        format!("Could not initialize request: {error:?}"),
                        Vec::new(),
                        window,
                        cx,
                    );
                });
        }
    }

    fn close_tab(&mut self, id: u32, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        self.tabs.remove(index);
        let _ = self
            .callbacks
            .close_tab
            .call1(&JsValue::UNDEFINED, &JsValue::from(id));
        if self.tabs.is_empty() {
            self.add_tab(window, cx);
        } else if self.active_tab == id {
            self.select_tab(self.tabs[index.saturating_sub(1)].id, cx);
        }
        cx.notify();
    }

    fn archive_tab(&mut self, id: u32, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        if tab.archiving || tab.client.read(cx).send_disabled() {
            return;
        }
        let client = tab.client.read(cx);
        let payload = serde_json::to_string(&client.request_snapshot(cx))
            .expect("Serialize archived request");
        let text = client
            .body_textarea()
            .map(|field| field.read(cx).value())
            .unwrap_or_default();
        tab.archiving = true;
        tab.client.update(cx, |client, cx| {
            client.archiving = true;
            cx.notify();
        });
        if let Err(error) = self.callbacks.archive_tab.call3(
            &JsValue::UNDEFINED,
            &JsValue::from(id),
            &JsValue::from_str(&payload),
            &JsValue::from_str(&text),
        ) {
            tab.archiving = false;
            tab.client.update(cx, |client, cx| {
                client.archiving = false;
                client.error = Some(format!("Could not archive request: {error:?}"));
                cx.notify();
            });
        }
        cx.notify();
    }

    pub(super) fn finish_archive(
        &mut self,
        id: u32,
        error: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if error.is_empty() {
            self.close_tab(id, window, cx);
        } else if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) {
            tab.archiving = false;
            tab.client.update(cx, |client, cx| {
                client.archiving = false;
                client.error = Some(format!("Could not archive request: {error}"));
                cx.notify();
            });
            cx.notify();
        }
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = palette::get(cx);
        let chrome = palette.toolbar;
        let border = palette.separator;
        let radius = cx.theme().radius;
        let foreground = workspace_tab_foreground();
        let tab_style = ButtonCustomVariant::new(cx).foreground(foreground);
        let url_font = cx.theme().mono_font_family.clone();
        let pitch = window.rem_size() * (WORKSPACE_TAB_WIDTH + WORKSPACE_TAB_GAP);
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .font_family(cx.theme().font_family.clone())
            .text_color(palette.foreground)
            .child(self.tab_drag_events(cx))
            .child(
                Toolbar::new("http-client-toolbar")
                    .justify_end()
                    .bg(chrome)
                    .border_b_1()
                    .border_color(border)
                    .content(
                        action_button("http-client-settings", cx)
                            .with_size(Size::Small)
                            .icon(Icon::default().data(include_bytes!("../../../assets/icons/settings.svg")))
                            .tooltip("Settings")
                            .accessibility_label("Settings")
                            .on_click(cx.listener(|this, _, window, cx| this.open_settings(window, cx))),
                    )
                    .content(
                        action_button("http-client-overflow", cx)
                            .with_size(Size::Small)
                            .icon(
                                Icon::default().data(include_bytes!(
                                    "../../../assets/icons/more-vertical.svg"
                                )),
                            )
                            .tooltip("More actions")
                            .accessibility_label("More actions")
                            .dropdown_menu_with_anchor(Anchor::TopRight, |menu, _, _| {
                                menu.with_size(Size::Small)
                                    .item(PopupMenuItem::new("No actions yet").disabled(true))
                            }),
                    ),
            )
            .child(
                div()
                    .id("http-client-tabs")
                    .flex()
                    .items_end()
                    .flex_none()
                    .h_9()
                    .px_2()
                    .gap_2()
                    .bg(chrome)
                    .child(
                        action_button("new-request-tab", cx)
                            .with_size(Size::Small)
                            .mb_1()
                            .icon(
                                Icon::default()
                                    .data(include_bytes!("../../../assets/icons/open-new-tab.svg"))
                                    .text_color(palette.icon),
                            )
                            .tooltip("New request tab")
                            .accessibility_label("New request tab")
                            .on_click(cx.listener(|this, _, window, cx| this.add_tab(window, cx))),
                    )
                    .child(
                        div()
                            .id("request-workspace-tabs")
                            .role(Role::TabList)
                            .min_w_0()
                            .flex_1()
                            .flex()
                            .gap(rems(WORKSPACE_TAB_GAP))
                            .overflow_x_scroll()
                            .track_scroll(&self.tab_scroll)
                            .children(self.tabs.iter().enumerate().map(|(slot, tab)| {
                                let id = tab.id;
                                let index = id as usize;
                                let slot_left = pitch * slot as f32;
                                let drag = self.tab_drag.as_ref().filter(|drag| drag.moved && drag.id == id);
                                let position = transition_with_status(
                                    ElementId::from(("workspace-tab-position", index)),
                                    drag.map_or(slot_left, |drag| drag.left),
                                    Transition::new(if drag.is_some() { Duration::ZERO } else { WORKSPACE_TAB_SLIDE }),
                                    window,
                                    cx,
                                );
                                tab.displayed_left.set(position.value);
                                let raised = drag.is_some()
                                    || (self.raised_tab == Some(id) && position.status == MotionStatus::Running);
                                let selected = self.active_tab == id;
                                let background = workspace_tab_background(index);
                                let url = tab.client.read(cx).url.read(cx).value();
                                let archiving = tab.archiving;
                                let archive_disabled = archiving || tab.client.read(cx).send_disabled();
                                let name = if id == 0 {
                                    "Request".to_owned()
                                } else {
                                    format!("Request {}", id + 1)
                                };
                                let header = div()
                                    .id(("request-workspace-tab-frame", index))
                                    .relative()
                                    .size_full()
                                    .left(position.value - slot_left)
                                    .pr_5()
                                    .rounded_tl(radius)
                                    .rounded_tr(radius)
                                    .bg(background)
                                    .child(Button::new(("request-workspace-tab", index))
                                    .custom(
                                        tab_style.hover(background.opacity(0.8)).active(background),
                                    )
                                    .rounded(ButtonRounded::None)
                                    .rounded_tl(radius)
                                    .rounded_tr(radius)
                                    .h_8()
                                    .w_full()
                                    .px_2()
                                    .pr_1()
                                    .cursor(if drag.is_some() { CursorStyle::ClosedHand } else { CursorStyle::OpenHand })
                                    .role(Role::Tab)
                                    .selected(selected)
                                    .accessibility_label(if url.is_empty() {
                                        name.clone()
                                    } else {
                                        format!("{name}, {url}")
                                    })
                                    .child(
                                        div()
                                            .w_full()
                                            .min_w_0()
                                            .flex()
                                            .flex_col()
                                            .line_height(rems(0.75))
                                            .child(
                                                div()
                                                    .h_3()
                                                    .text_size(rems(0.625))
                                                    .truncate()
                                                    .when(selected, |title| title.font_semibold())
                                                    .child(name),
                                            )
                                            .child(
                                                div()
                                                    .h_3()
                                                    .font_family(url_font.clone())
                                                    .text_size(rems(0.5625))
                                                    .whitespace_nowrap()
                                                    .overflow_hidden()
                                                    .text_ellipsis_start()
                                                    .child(url),
                                            ),
                                    )
                                    .on_click(
                                        cx.listener(move |this, _, _, cx| this.select_tab(id, cx)),
                                    )
                                    .on_mouse_down(MouseButton::Left, cx.listener(move |this, event: &gpui_kit::MouseDownEvent, _, _| {
                                        this.start_tab_drag(id, event.position);
                                    })))
                                    .child(
                                        Button::new(("close-request-tab", index))
                                            .custom(tab_style.hover(foreground.opacity(0.12)))
                                            .absolute().top_0().right_0()
                                            .h_4().w_5().p_0()
                                            .cursor_pointer()
                                            .disabled(archiving)
                                            .child(Icon::default().data(include_bytes!("../../../assets/icons/close.svg")).with_size(Size::Size(px(9.0))))
                                            .tooltip("Close tab")
                                            .accessibility_label("Close tab")
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                cx.stop_propagation();
                                                this.close_tab(id, window, cx);
                                            })),
                                    )
                                    .child(
                                        Button::new(("archive-request-tab", index))
                                            .custom(tab_style.hover(foreground.opacity(0.12)))
                                            .absolute().bottom_0().right_0()
                                            .h_4().w_5().p_0()
                                            .cursor_pointer()
                                            .disabled(archive_disabled)
                                            .child(Icon::default().data(include_bytes!("../../../assets/icons/move-to-another-changelog.svg")).with_size(Size::Size(px(9.0))))
                                            .tooltip("Archive request")
                                            .accessibility_label("Archive request")
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                cx.stop_propagation();
                                                this.archive_tab(id, cx);
                                            })),
                                    );
                                div()
                                    .relative()
                                    .flex_none()
                                    .w(rems(WORKSPACE_TAB_WIDTH))
                                    .h_8()
                                    .child(if raised {
                                        deferred(header).into_any_element()
                                    } else {
                                        header.into_any_element()
                                    })
                            })),
                    ),
            )
            .child(self.tab(self.active_tab).expect("Active request tab"))
            .children(self.settings_dialog.clone())
    }
}

fn format_byte_size(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let units = ["KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < units.len() {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", units[unit])
}

fn exchange_header() -> Div {
    div().flex_none().flex().items_center().h(px(30.)).mb_2()
}

fn segment_button(id: impl Into<ElementId>, selected: bool, cx: &App) -> Button {
    let palette = palette::get(cx);
    let style = ButtonCustomVariant::new(cx)
        .foreground(palette.ui_foreground)
        .hover(palette.hover)
        .active(palette.active);
    Button::new(id)
        .custom(if selected {
            style
                .active(cx.theme().tab_active)
                .foreground(cx.theme().tab_active_foreground)
        } else {
            style
        })
        .with_size(Size::Small)
        .cursor_pointer()
        .rounded(ButtonRounded::Small)
        .role(Role::Tab)
        .selected(selected)
}

fn detail(name: &str, value: &str) -> impl IntoElement + use<> {
    div()
        .flex()
        .gap_4()
        .px_3()
        .py_2()
        .text_sm()
        .child(div().flex_none().w_32().child(name.to_owned()))
        .child(div().min_w_0().flex_1().child(value.to_owned()))
}

fn timeline_row(label: Div, time: impl IntoElement, cumulative: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_none()
        .gap_3()
        .px_3()
        .py_0p5()
        .cursor(CursorStyle::IBeam)
        .child(label.flex_1().min_w_0())
        .child(
            div()
                .flex_none()
                .w_20()
                .max_w(relative(0.26))
                .flex()
                .justify_end()
                .text_left()
                .child(div().min_w_0().child(time)),
        )
        .child(
            div()
                .flex_none()
                .w(rems(9.0))
                .max_w(relative(0.30))
                .flex()
                .justify_end()
                .text_left()
                .child(div().min_w_0().child(cumulative)),
        )
}
