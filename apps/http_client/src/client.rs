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
    toolbar::Toolbar,
};
use gpui_kit::{
    Anchor, AnyElement, App, AppContext as _, Context, Div, Entity, Focusable as _,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window, accesskit::Role, div,
    prelude::FluentBuilder as _, px, relative, rems,
};
use js_sys::Function;
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsValue;

use crate::{
    palette,
    tree_sitter_highlighter::{ParseSnapshot, SyntaxEditHandler, TreeSitterHighlighter},
    workspace_tab_background, workspace_tab_foreground,
};

const METHODS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];
const REQUEST_TABS: &[&str] = &["JSON", "Params", "Headers", "Auth", "Settings", "Info"];
const RESPONSE_TABS: &[&str] = &["Response", "Request", "Headers", "Cookies", "Timeline"];

#[derive(Default, Deserialize)]
pub(super) struct Preview {
    url: String,
    request: String,
    headers: String,
    response: Option<Response>,
}

pub(super) struct ClientCallbacks {
    pub(super) send: Function,
    pub(super) queue_edit: Function,
    pub(super) load_request: Function,
}

#[derive(Serialize)]
struct Request {
    method: &'static str,
    url: String,
    params: String,
    headers: String,
    #[serde(skip)]
    body: String,
}

#[derive(Deserialize)]
struct Response {
    status: u16,
    status_text: String,
    elapsed_ms: u32,
    bytes: usize,
    headers: Vec<(String, String)>,
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
        let editor = cx.new(|cx| {
            let mut state = EditorState::new(window, cx)
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
    params: Entity<TextareaState>,
    headers: Entity<TextareaState>,
    response_body: JsonBody,
    response: Option<Response>,
    response_loading: bool,
    request_loading: bool,
    sent: Option<Request>,
    request_tab: usize,
    response_tab: usize,
    send: Function,
    tab_id: u32,
    pending: bool,
    error: Option<String>,
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
        let params = cx.new(|cx| TextareaState::new(window, cx).placeholder("name=value"));
        let headers = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("Name: value")
                .default_value(preview.headers.clone())
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
        let sent = preview.response.as_ref().map(|_| Request {
            method: "POST",
            url: preview.url,
            params: String::new(),
            headers: preview.headers,
            body: preview.request,
        });
        Self {
            method: if sent.is_some() { "POST" } else { "GET" },
            url,
            request,
            params,
            headers,
            response_body,
            response: preview.response,
            response_loading: false,
            request_loading: true,
            sent,
            request_tab: 0,
            response_tab: 0,
            send: callbacks.send.clone(),
            tab_id,
            pending: false,
            error: None,
        }
    }

    fn send_request(&mut self, cx: &mut Context<Self>) {
        if self.pending || self.request_loading {
            return;
        }
        let request = Request {
            method: self.method,
            url: self.url.read(cx).value().trim().to_owned(),
            params: self.params.read(cx).value().to_string(),
            headers: self.headers.read(cx).value().to_string(),
            body: {
                let mut chars = self.request.editor.read(cx).text().chars();
                let mut preview: String = chars.by_ref().take(4096).collect();
                if chars.next().is_some() {
                    preview.push_str("\n…");
                }
                preview
            },
        };
        self.error = None;
        self.pending = true;
        let payload = serde_json::to_string(&request).expect("Serialize HTTP request");
        self.sent = Some(request);
        if self
            .send
            .call2(
                &JsValue::UNDEFINED,
                &JsValue::from(self.tab_id),
                &JsValue::from_str(&payload),
            )
            .is_err()
        {
            self.pending = false;
            self.error = Some("Could not send request".into());
        }
        cx.notify();
    }

    pub(super) fn begin_response(&mut self, cx: &mut Context<Self>) {
        self.pending = true;
        self.response_loading = true;
        self.response_tab = 0;
        cx.notify();
    }

    pub(super) fn finish_request_error(&mut self, error: String, cx: &mut Context<Self>) {
        self.pending = false;
        self.response_loading = false;
        self.error = Some(error);
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
            self.response = Some(response);
            self.pending = false;
            self.response_loading = false;
            self.response_tab = 0;
            cx.notify();
        }
        installed
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
        let send_disabled = self.pending || self.request_loading;
        let palette = palette::get(cx);
        let send_color = if send_disabled {
            cx.theme().muted_foreground.opacity(0.5)
        } else {
            palette.foreground
        };
        let send_style = ButtonCustomVariant::new(cx)
            .foreground(palette.foreground)
            .hover(palette.hover)
            .active(palette.active);
        let owner = cx.weak_entity();
        exchange_header()
            .bg(palette.background)
            .border_1()
            .rounded_sm()
            .border_color(cx.theme().border.opacity(0.65))
            .capture_action(cx.listener(|this, _: &Enter, _, cx| {
                cx.stop_propagation();
                this.send_request(cx);
            }))
            .child(
                Button::new("http-method")
                    .ghost()
                    .with_size(Size::XSmall)
                    .px_3()
                    .label(self.method)
                    .accessibility_label(format!("HTTP method, {}", self.method))
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_color(cx.theme().green)
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
                Button::new("send-request")
                    .custom(send_style)
                    .with_size(Size::Small)
                    .w_auto()
                    .px_3()
                    .disabled(send_disabled)
                    .when(!send_disabled, |button| button.cursor_pointer())
                    .icon(
                        Icon::default()
                            .data(include_bytes!("../../../assets/icons/run.svg"))
                            .text_color(send_color),
                    )
                    .tooltip("Send request")
                    .accessibility_label("Send request")
                    .on_click(cx.listener(|this, _, _, cx| this.send_request(cx))),
            )
    }

    fn response_status(&self, cx: &App) -> impl IntoElement {
        let text = if self.request_loading {
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
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("Preview"),
            )
    }

    fn tabs(&self, response: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let (labels, active, id) = if response {
            (RESPONSE_TABS, self.response_tab, "response-tabs")
        } else {
            (REQUEST_TABS, self.request_tab, "request-tabs")
        };
        let palette = palette::get(cx);
        let segment_style = ButtonCustomVariant::new(cx)
            .foreground(palette.ui_foreground)
            .hover(palette.hover)
            .active(palette.active);
        let selected_style = segment_style
            .active(cx.theme().tab_active)
            .foreground(cx.theme().tab_active_foreground);
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
                    .children(labels.iter().enumerate().map(|(index, &label)| {
                        Button::new((id, index))
                            .custom(if active == index {
                                selected_style
                            } else {
                                segment_style
                            })
                            .with_size(Size::Small)
                            .cursor_pointer()
                            .rounded(ButtonRounded::Small)
                            .role(Role::Tab)
                            .selected(active == index)
                            .label(label)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if response {
                                    this.response_tab = index;
                                } else {
                                    this.request_tab = index;
                                }
                                cx.notify();
                            }))
                    })),
            )
    }

    fn request_content(&self, cx: &App) -> AnyElement {
        match self.request_tab {
            0 => self
                .request
                .render(false, self.request_loading, cx)
                .into_any_element(),
            1 => Textarea::new(&self.params)
                .appearance(false)
                .size_full()
                .aria_label("Query parameters, one name=value per line")
                .into_any_element(),
            2 => Textarea::new(&self.headers)
                .appearance(false)
                .size_full()
                .aria_label("Request headers, one Name: value per line")
                .into_any_element(),
            3 => detail("Authentication", "No authentication configured").into_any_element(),
            4 => detail("Transport", "Preview").into_any_element(),
            _ => detail("Request body", "JSON5").into_any_element(),
        }
    }

    fn response_content(&self, cx: &App) -> AnyElement {
        let Some(sent) = &self.sent else {
            return div().into_any_element();
        };
        match self.response_tab {
            0 => self
                .response_body
                .render(true, false, cx)
                .into_any_element(),
            1 => div()
                .id("sent-request")
                .size_full()
                .overflow_y_scroll()
                .px_3()
                .py_2()
                .font_family(cx.theme().mono_font_family.clone())
                .text_sm()
                .child(format!("{} {}", sent.method, sent.url))
                .when(!sent.params.is_empty(), |view| {
                    view.child(div().mt_2().child(sent.params.clone()))
                })
                .when(!sent.headers.is_empty(), |view| {
                    view.child(div().mt_2().child(sent.headers.clone()))
                })
                .when(!sent.body.is_empty(), |view| {
                    view.child(div().mt_2().child(sent.body.clone()))
                })
                .into_any_element(),
            2 => div()
                .id("response-headers")
                .size_full()
                .overflow_y_scroll()
                .children(
                    self.response
                        .iter()
                        .flat_map(|response| &response.headers)
                        .map(|(name, value)| detail(name, value)),
                )
                .into_any_element(),
            3 => detail("Cookies", "No response cookies").into_any_element(),
            _ => div()
                .children(
                    self.response
                        .iter()
                        .map(|response| detail("Total", &format!("{} ms", response.elapsed_ms))),
                )
                .into_any_element(),
        }
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
}

impl Render for HttpClient {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = palette::get(cx);
        let tab_background = workspace_tab_background(self.tab_id as usize);
        let request_background = palette.background;
        let response_background = palette.toolbar;
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
            .gap_2()
            .bg(tab_background)
            .role(Role::TabPanel)
            .aria_label("Request")
            .child(
                div()
                    .w(relative(0.515))
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

pub(super) struct Workspace {
    tabs: Vec<Entity<HttpClient>>,
    url_subscriptions: Vec<Subscription>,
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
            url_subscriptions: vec![Self::subscribe_to_url(&first_tab, cx)],
            tabs: vec![first_tab],
            active_tab: 0,
            tab_scroll: ScrollHandle::new(),
            callbacks,
        }
    }

    pub(super) fn tab(&self, id: u32) -> Option<Entity<HttpClient>> {
        self.tabs.get(id as usize).cloned()
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
        self.active_tab = id;
        self.tab_scroll.scroll_to_item(id as usize);
        cx.notify();
    }

    fn add_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.tabs.len() as u32;
        let tab = cx.new(|cx| HttpClient::new(window, cx, id, &self.callbacks, Preview::default()));
        tab.read(cx).url.read(cx).focus_handle(cx).focus(window, cx);
        self.url_subscriptions
            .push(Self::subscribe_to_url(&tab, cx));
        self.tabs.push(tab);
        self.select_tab(id, cx);
        if let Err(error) = self.callbacks.load_request.call2(
            &JsValue::UNDEFINED,
            &JsValue::from(id),
            &JsValue::from_str(""),
        ) {
            self.tabs[id as usize].update(cx, |tab, cx| {
                tab.set_request_loading(false, cx);
                tab.finish_request_error(format!("Could not initialize request: {error:?}"), cx);
            });
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
                        Button::new("http-client-overflow")
                            .ghost()
                            .with_size(Size::Small)
                            .cursor_pointer()
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
                        Button::new("new-request-tab")
                            .ghost()
                            .with_size(Size::Small)
                            .mb_1()
                            .cursor_pointer()
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
                            .children(self.tabs.iter().enumerate().map(|(index, tab)| {
                                let id = index as u32;
                                let selected = self.active_tab == id;
                                let background = workspace_tab_background(index);
                                let url = tab.read(cx).url.read(cx).value();
                                let name = if id == 0 {
                                    "Request".to_owned()
                                } else {
                                    format!("Request {}", id + 1)
                                };
                                Button::new(("request-workspace-tab", index))
                                    .custom(
                                        tab_style.hover(background.opacity(0.8)).active(background),
                                    )
                                    .bg(background)
                                    .rounded(ButtonRounded::None)
                                    .rounded_tl(radius)
                                    .rounded_tr(radius)
                                    .h_8()
                                    .w_40()
                                    .px_2()
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
                                    )
                            })),
                    ),
            )
            .child(self.tabs[self.active_tab as usize].clone())
    }
}

fn exchange_header() -> Div {
    div().flex_none().flex().items_center().h(px(30.)).mb_2()
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
