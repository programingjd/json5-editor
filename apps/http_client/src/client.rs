use std::{cell::RefCell, rc::Rc};

use gpui_kit::base::{
    StyledExt as _,
    input::{Enter, InputEvent, InputPreparation},
};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Selectable as _, Sizable as _, Size,
    button::{Button, ButtonCustomVariant, ButtonRounded, ButtonVariants as _},
    input::{Editor, EditorState, Input, InputState, Textarea, TextareaState},
    menu::{DropdownMenu as _, PopupMenuItem},
    scroll::ScrollableElement as _,
    toolbar::Toolbar,
};
use gpui_kit::{
    Anchor, AnyElement, App, AppContext as _, Context, CursorStyle, Div, DragMoveEvent, ElementId,
    EmptyView, Entity, Focusable as _, InteractiveElement as _, IntoElement, KeyBinding,
    KeyDownEvent, ParentElement as _, Point, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, TextStyleRefinement, Window,
    accesskit::Role, div, prelude::FluentBuilder as _, px, relative, rems,
};
use js_sys::Function;
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsValue;

use crate::{
    DETAIL_PANE_MAX_WIDTH_REMS,
    buttons::{action_button, field_button, field_button_icon, field_button_label},
    palette,
    query::localhost_url,
    request_fields::{RequestFields, param_field, param_placeholder_color},
    response_headers::ResponseHeaders,
    timeline_foregrounds,
    tree_sitter_highlighter::{ParseSnapshot, SyntaxEditHandler, TreeSitterHighlighter},
    workspace_tab_background, workspace_tab_foreground,
};

const METHODS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];
const RESPONSE_TABS: &[&str] = &["Response", "Headers", "Cookies", "Timeline"];
const DEFAULT_TIMEOUT_SECONDS: u32 = 30;
const DEFAULT_REQUEST_FRACTION: f32 = 0.515;
const MIN_PANE_FRACTION: f32 = 0.2;
const REQUEST_PANE_CONTEXT: &str = "HttpRequestPane";

gpui_kit::actions!(http_client, [SendRequest]);

pub(super) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-enter", SendRequest, Some(REQUEST_PANE_CONTEXT)),
        // Match at input depth too so the editor's secondary Enter binding cannot win.
        KeyBinding::new("ctrl-enter", SendRequest, Some("HttpRequestPane > Input")),
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

#[derive(Deserialize)]
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

#[derive(Deserialize)]
struct Response {
    #[serde(default)]
    preview: bool,
    status: u16,
    status_text: String,
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
    binary_name: Option<SharedString>,
    pick_binary: Function,
    params: Entity<RequestFields>,
    headers: Entity<RequestFields>,
    timeout: Entity<InputState>,
    timeout_enabled: bool,
    follow_redirects: bool,
    response_body: JsonBody,
    response_headers: Entity<ResponseHeaders>,
    response_text: Entity<TextareaState>,
    download_response: Function,
    response: Option<Response>,
    error_timeline: Vec<TimelineEvent>,
    timeline_scroll: ScrollHandle,
    response_loading: bool,
    request_loading: bool,
    request_tab: RequestTab,
    response_tab: usize,
    request_fraction: f32,
    send: Function,
    tab_id: u32,
    pending: bool,
    archiving: bool,
    error: Option<String>,
    _url_subscription: Subscription,
}

impl HttpClient {
    fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        tab_id: u32,
        callbacks: &ClientCallbacks,
        preview: Preview,
    ) -> Self {
        let url = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("https://")
                .default_value(preview.url.clone())
        });
        let params = cx.new(|cx| RequestFields::query(url.clone(), window, cx));
        let headers = cx.new(|cx| RequestFields::headers(&preview.headers, window, cx));
        let response_headers = cx.new(|cx| {
            ResponseHeaders::new(
                preview
                    .response
                    .as_ref()
                    .map_or(&[], |response| response.headers.as_slice()),
                window,
                cx,
            )
        });
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
            binary_name: None,
            pick_binary: callbacks.pick_binary.clone(),
            params,
            headers,
            timeout: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(DEFAULT_TIMEOUT_SECONDS.to_string())
                    .default_value(DEFAULT_TIMEOUT_SECONDS.to_string())
                    .validate(|text, _| {
                        text.is_empty()
                            || (text.len() <= 3
                                && text.bytes().all(|byte| byte.is_ascii_digit())
                                && text.parse::<u32>().is_ok_and(|seconds| seconds > 0))
                    })
            }),
            timeout_enabled: false,
            follow_redirects: true,
            response_body,
            response_headers,
            response_text: cx.new(|cx| {
                window.with_text_style(Some(response_font), |window| TextareaState::new(window, cx))
            }),
            download_response: callbacks.download_response.clone(),
            response: preview.response,
            error_timeline: Vec::new(),
            timeline_scroll: ScrollHandle::new(),
            response_loading: false,
            request_loading: true,
            request_tab: RequestTab::Body,
            response_tab: 0,
            request_fraction: DEFAULT_REQUEST_FRACTION,
            send: callbacks.send.clone(),
            tab_id,
            pending: false,
            archiving: false,
            error: None,
            _url_subscription: url_subscription,
        }
    }

    fn normalize_url(&self, window: &mut Window, cx: &mut Context<Self>) {
        let value = self.url.read(cx).value();
        if let Some(url) = localhost_url(value.trim()) {
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
            self.error = Some("Could not send request".into());
        }
        cx.notify();
    }

    fn request_snapshot(&self, cx: &App) -> Request {
        Request {
            method: self.method,
            url: self.url.read(cx).value().trim().to_owned(),
            headers: self.headers.read(cx).header_text(cx),
            body_format: self.body_format,
            timeout_ms: if self.timeout_enabled {
                u64::from(
                    self.timeout
                        .read(cx)
                        .value()
                        .parse()
                        .unwrap_or(DEFAULT_TIMEOUT_SECONDS),
                ) * 1000
            } else {
                0
            },
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

    pub(super) fn set_binary_name(&mut self, name: String, cx: &mut Context<Self>) {
        self.binary_name = Some(name.into());
        cx.notify();
    }

    pub(super) fn begin_response(&mut self, cx: &mut Context<Self>) {
        self.pending = true;
        self.response_loading = true;
        cx.notify();
    }

    pub(super) fn finish_request_error(
        &mut self,
        error: String,
        timeline: Vec<TimelineEvent>,
        cx: &mut Context<Self>,
    ) {
        self.pending = false;
        self.response_loading = false;
        self.error = Some(error);
        self.error_timeline = timeline;
        self.timeline_scroll.set_offset(Point::default());
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
        self.response_headers.update(cx, |headers, cx| {
            headers.set_headers(&response.headers, window, cx)
        });
        self.response = Some(response);
        self.timeline_scroll.set_offset(Point::default());
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
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.response_tab = index;
                                        cx.notify();
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
                .on_click(cx.listener(|this, _, _, cx| {
                    this.request_tab = RequestTab::Body;
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
                        let item = PopupMenuItem::new(option.label()).checked(option == format);
                        let item = if option == format {
                            item.icon(
                                Icon::default()
                                    .data(include_bytes!("../../../assets/icons/check.svg")),
                            )
                        } else {
                            item
                        };
                        menu.item(item.on_click(move |_, window, cx| {
                            let _ = owner.update(cx, |this, cx| {
                                if this.body_format != option {
                                    this.body_format = option;
                                    this.headers.update(cx, |headers, cx| {
                                        headers.set_content_type(option.content_type(), window, cx);
                                    });
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
        let palette = palette::get(cx);
        let thumb = palette.icon;
        let track = if self.follow_redirects {
            palette.selection
        } else {
            palette.hover
        };
        let switch_style = ButtonCustomVariant::new(cx)
            .color(track)
            .hover(track.opacity(0.8))
            .active(track.opacity(0.7));
        div()
            .w_full()
            .max_w(rems(DETAIL_PANE_MAX_WIDTH_REMS))
            .mx_auto()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .text_sm()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child("Timeout")
                    .child(self.timeout_field(cx)),
            )
            .child(div().w_full().h(px(1.0)).bg(palette.border.opacity(0.6)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child("Follow redirect")
                    .child(
                        Button::new("follow-redirects")
                            .custom(switch_style)
                            .role(Role::Switch)
                            .accessibility_label("Follow redirect")
                            .toggled(self.follow_redirects)
                            .cursor_pointer()
                            .w_8()
                            .h_4()
                            .p_0p5()
                            .rounded_full()
                            .child(
                                div()
                                    .w_full()
                                    .flex()
                                    .when(self.follow_redirects, |row| row.justify_end())
                                    .child(div().size_3().rounded_full().bg(thumb)),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.follow_redirects = !this.follow_redirects;
                                cx.notify();
                            })),
                    ),
            )
    }

    fn timeout_field(&self, cx: &Context<Self>) -> impl IntoElement + use<> {
        let enabled = self.timeout_enabled;
        let owner = cx.weak_entity();
        let muted = cx.theme().muted_foreground;
        let placeholder = param_placeholder_color(cx);
        let chevron = field_button_icon(
            include_bytes!("../../../assets/icons/chevron-down.svg"),
            rems(0.75),
            muted,
            true,
        );
        let menu = field_button("timeout-options", cx)
            .with_size(Size::Small)
            .cursor_pointer()
            .accessibility_label(if enabled {
                "Timeout options"
            } else {
                "No timeout"
            })
            .map(|button| {
                if enabled {
                    button.min_w_6().child(chevron)
                } else {
                    button.w_full().child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .justify_between()
                            .text_color(placeholder)
                            .child("No timeout")
                            .child(chevron),
                    )
                }
            })
            .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                [(true, "Edit"), (false, "No timeout")].into_iter().fold(
                    menu.with_size(Size::Small),
                    |menu, (edit, label)| {
                        let owner = owner.clone();
                        let checked = enabled == edit;
                        let item = PopupMenuItem::new(label).checked(checked);
                        let item = if checked {
                            item.icon(
                                Icon::default()
                                    .data(include_bytes!("../../../assets/icons/check.svg")),
                            )
                        } else {
                            item
                        };
                        menu.item(item.on_click(move |_, window, cx| {
                            let _ = owner.update(cx, |this, cx| {
                                this.timeout_enabled = edit;
                                if edit {
                                    let focus = this.timeout.read(cx).focus_handle(cx);
                                    window.on_next_frame(move |window, cx| focus.focus(window, cx));
                                }
                                cx.notify();
                            });
                        }))
                    },
                )
            });
        param_field(!enabled, cx)
            .flex_none()
            .w(rems(if enabled { 5.25 } else { 6.5 }))
            .when(enabled, |field| {
                field
                    .child(
                        div().flex_1().min_w_0().child(
                            Input::new(&self.timeout)
                                .with_size(Size::Small)
                                .appearance(false)
                                .bordered(false)
                                .aria_label("Timeout in seconds")
                                .px_2()
                                .w_full(),
                        ),
                    )
                    .child(div().text_color(muted).child("s"))
            })
            .when(!enabled, |field| {
                field.flex_col().items_stretch().justify_center()
            })
            .child(menu)
    }

    fn request_body_content(&self, cx: &App) -> AnyElement {
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
                let tab_id = self.tab_id;
                let pick_binary = self.pick_binary.clone();
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .p_2()
                    .child(
                        Button::new("request-binary-file")
                            .with_size(Size::Small)
                            .cursor_pointer()
                            .label("Choose file")
                            .on_click(move |_, _, _| {
                                let _ =
                                    pick_binary.call1(&JsValue::UNDEFINED, &JsValue::from(tab_id));
                            }),
                    )
                    .when_some(self.binary_name.clone(), |row, name| {
                        row.child(div().min_w_0().truncate().child(name))
                    })
                    .into_any_element()
            }
            _ => div().into_any_element(),
        }
    }

    fn response_content(&self, cx: &App) -> AnyElement {
        match self.response_tab {
            0 => match self.response.as_ref().map(|response| &response.format) {
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

    fn response_timeline(&self, cx: &App) -> impl IntoElement + use<> {
        let events = if self.error.is_some() {
            &self.error_timeline[..]
        } else {
            self.response
                .as_ref()
                .map_or(&[][..], |response| &response.timeline)
        };
        let mono = cx.theme().mono_font_family.clone();
        let label_font = cx.theme().font_family.clone();
        let (request_foreground, response_foreground) = timeline_foregrounds();
        let summary_foreground = cx.theme().foreground.opacity(0.5);
        let separator = cx.theme().border.opacity(0.5);
        let mut previous = 0.0;
        let mut total = 0.0;
        div()
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .text_sm()
            .child(
                timeline_row(div().child("Event"), "Time (ms)", "Cumulative time (ms)")
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
                            .track_scroll(&self.timeline_scroll)
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .children(events.iter().map(|event| {
                                let delta = (event.elapsed_ms - previous).max(0.0);
                                previous = event.elapsed_ms;
                                total += delta;
                                let foreground = match event.side {
                                    TimelineSide::Request => request_foreground,
                                    TimelineSide::Response => response_foreground,
                                };
                                timeline_row(
                                    div()
                                        .font_family(label_font.clone())
                                        .italic()
                                        .child(event.label.clone()),
                                    format!("{delta:.2}"),
                                    format!("{:.2}", event.elapsed_ms),
                                )
                                .text_color(foreground)
                                .font_family(mono.clone())
                            }))
                            .when(!events.is_empty(), |list| {
                                list.child(
                                    timeline_row(
                                        div()
                                            .font_family(label_font.clone())
                                            .text_xs()
                                            .child("Total"),
                                        format!("{total:.2}"),
                                        format!("{previous:.2}"),
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
                    .vertical_scrollbar(&self.timeline_scroll),
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
    _url_subscription: Subscription,
}

pub(super) struct Workspace {
    tabs: Vec<WorkspaceTab>,
    next_tab_id: u32,
    active_tab: u32,
    tab_scroll: ScrollHandle,
    callbacks: ClientCallbacks,
}

impl Workspace {
    pub(super) fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        callbacks: ClientCallbacks,
        preview: Preview,
    ) -> Self {
        let first_tab = cx.new(|cx| HttpClient::new(window, cx, 0, &callbacks, preview));
        Self {
            tabs: vec![WorkspaceTab {
                id: 0,
                _url_subscription: Self::subscribe_to_url(&first_tab, cx),
                client: first_tab,
                archiving: false,
            }],
            next_tab_id: 1,
            active_tab: 0,
            tab_scroll: ScrollHandle::new(),
            callbacks,
        }
    }

    pub(super) fn tab(&self, id: u32) -> Option<Entity<HttpClient>> {
        self.tabs
            .iter()
            .find(|tab| tab.id == id)
            .map(|tab| tab.client.clone())
    }

    pub(super) fn selected_response_header_text(
        &self,
        window: &mut Window,
        cx: &mut App,
    ) -> String {
        let client = self.tab(self.active_tab).expect("Active request tab");
        let tab = client.read(cx);
        if tab.response_tab == 1 {
            let headers = tab.response_headers.clone();
            headers.update(cx, |headers, cx| headers.selected_text(window, cx))
        } else {
            String::new()
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

    fn add_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        let tab = cx.new(|cx| HttpClient::new(window, cx, id, &self.callbacks, Preview::default()));
        tab.read(cx).url.read(cx).focus_handle(cx).focus(window, cx);
        self.tabs.push(WorkspaceTab {
            id,
            _url_subscription: Self::subscribe_to_url(&tab, cx),
            client: tab,
            archiving: false,
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
                client.error = Some(format!("Could not archive request: {error:?}").into());
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
                client.error = Some(format!("Could not archive request: {error}").into());
                cx.notify();
            });
            cx.notify();
        }
    }
}

impl Render for Workspace {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = palette::get(cx);
        let chrome = palette.toolbar;
        let border = palette.separator;
        let radius = cx.theme().radius;
        let foreground = workspace_tab_foreground();
        let tab_style = ButtonCustomVariant::new(cx).foreground(foreground);
        let url_font = cx.theme().mono_font_family.clone();
        div()
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .font_family(cx.theme().font_family.clone())
            .text_color(palette.foreground)
            .child(
                Toolbar::new("http-client-toolbar")
                    .justify_end()
                    .bg(chrome)
                    .border_b_1()
                    .border_color(border)
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
                            .gap_1()
                            .overflow_x_scroll()
                            .track_scroll(&self.tab_scroll)
                            .children(self.tabs.iter().map(|tab| {
                                let id = tab.id;
                                let index = id as usize;
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
                                div()
                                    .id(("request-workspace-tab-frame", index))
                                    .relative()
                                    .flex_none()
                                    .w_40()
                                    .h_8()
                                    .child(Button::new(("request-workspace-tab", index))
                                    .custom(
                                        tab_style.hover(background.opacity(0.8)).active(background),
                                    )
                                    .bg(background)
                                    .rounded(ButtonRounded::None)
                                    .rounded_tl(radius)
                                    .rounded_tr(radius)
                                    .h_8()
                                    .w_full()
                                    .px_2()
                                    .pr_6()
                                    .cursor_pointer()
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
                                    ))
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
                                    )
                            })),
                    ),
            )
            .child(self.tab(self.active_tab).expect("Active request tab"))
    }
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

fn timeline_row(
    label: Div,
    time: impl Into<SharedString>,
    cumulative: impl Into<SharedString>,
) -> Div {
    div()
        .flex()
        .flex_none()
        .gap_3()
        .px_3()
        .py_0p5()
        .child(label.flex_1().min_w_0())
        .child(
            div()
                .flex_none()
                .w_20()
                .max_w(relative(0.26))
                .text_right()
                .child(time.into()),
        )
        .child(
            div()
                .flex_none()
                .w(rems(9.0))
                .max_w(relative(0.30))
                .text_right()
                .child(cumulative.into()),
        )
}
