use std::{cell::RefCell, rc::Rc};

use gpui_kit::base::input::{Enter, InputPreparation};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _, Size,
    button::{Button, ButtonVariants as _},
    input::{Editor, EditorState, Input, InputState, Textarea, TextareaState},
    menu::{DropdownMenu as _, PopupMenuItem},
};
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _, Window,
    div, prelude::FluentBuilder as _, px, relative, rems,
};
use js_sys::Function;
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsValue;

use crate::{
    palette,
    tree_sitter_highlighter::{ParseSnapshot, SyntaxEditHandler, TreeSitterHighlighter},
};

const METHODS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];
const REQUEST_TABS: &[&str] = &["JSON", "Params", "Headers", "Auth", "Settings", "Info"];
const RESPONSE_TABS: &[&str] = &["Response", "Request", "Headers", "Cookies", "Timeline"];

#[derive(Deserialize)]
pub(super) struct Preview {
    url: String,
    request: String,
    headers: String,
    response: Response,
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
    response: Response,
    response_loading: bool,
    request_loading: bool,
    sent: Request,
    request_tab: usize,
    response_tab: usize,
    send: Function,
    pending: bool,
    error: Option<String>,
}

impl HttpClient {
    pub(super) fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        send: Function,
        queue_edit: Function,
        load_request: Function,
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
            let _ = queue_edit.call1(
                &JsValue::UNDEFINED,
                &JsValue::from_str(&payload.to_string()),
            );
        });
        let request = JsonBody::new(Some(on_edit), window, cx);
        let response_body = JsonBody::new(None, window, cx);
        request.editor.update(cx, |editor, cx| {
            editor.set_readonly(true, cx);
            editor.set_clipboard_paste_handler(move |editor, text| {
                editor.caret_count() == 1
                    && editor.selected_range() == (0..editor.text().len())
                    && text.trim_start().starts_with(['{', '['])
                    && load_request
                        .call1(&JsValue::UNDEFINED, &JsValue::from_str(text))
                        .is_ok()
            });
        });
        Self {
            method: "POST",
            url,
            request,
            params,
            headers,
            response_body,
            response: preview.response,
            response_loading: false,
            request_loading: true,
            sent: Request {
                method: "POST",
                url: preview.url,
                params: String::new(),
                headers: preview.headers,
                body: preview.request,
            },
            request_tab: 0,
            response_tab: 0,
            send,
            pending: false,
            error: None,
        }
    }

    fn send_request(&mut self, cx: &mut Context<Self>) {
        if self.pending || self.request_loading {
            return;
        }
        self.sent = Request {
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
        let payload = serde_json::to_string(&self.sent).expect("Serialize HTTP request");
        if self
            .send
            .call1(&JsValue::UNDEFINED, &JsValue::from_str(&payload))
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
            self.response = response;
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
        let owner = cx.weak_entity();
        div()
            .h(px(30.))
            .flex()
            .items_center()
            .mx_2()
            .mt(rems(0.5625))
            .mb(rems(0.3125))
            .flex_none()
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
                    .ghost()
                    .with_size(Size::Small)
                    .w_auto()
                    .px_3()
                    .disabled(self.pending || self.request_loading)
                    .icon(Icon::default().data(include_bytes!("send.svg")))
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
        } else {
            format!(
                "{} {}  •  {} ms  •  {} B",
                self.response.status,
                self.response.status_text,
                self.response.elapsed_ms,
                self.response.bytes
            )
        };
        div()
            .flex()
            .items_center()
            .gap_3()
            .px_3()
            .mt_1()
            .h(px(40.))
            .flex_none()
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
        let foreground = palette.ui_foreground;
        let underline = palette.link;
        div()
            .id(id)
            .flex()
            .items_center()
            .h(px(36.))
            .flex_none()
            .px_1()
            .overflow_x_scroll()
            .children(labels.iter().enumerate().map(|(index, &label)| {
                let selected = active == index;
                div()
                    .flex_none()
                    .h_full()
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(if selected {
                        underline
                    } else {
                        gpui_kit::transparent_black()
                    })
                    .child(
                        Button::new((id, index))
                            .ghost()
                            .with_size(Size::Small)
                            .label(label)
                            .text_color(foreground)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if response {
                                    this.response_tab = index;
                                } else {
                                    this.request_tab = index;
                                }
                                cx.notify();
                            })),
                    )
            }))
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
                .child(format!("{} {}", self.sent.method, self.sent.url))
                .when(!self.sent.params.is_empty(), |view| {
                    view.child(div().mt_2().child(self.sent.params.clone()))
                })
                .when(!self.sent.headers.is_empty(), |view| {
                    view.child(div().mt_2().child(self.sent.headers.clone()))
                })
                .when(!self.sent.body.is_empty(), |view| {
                    view.child(div().mt_2().child(self.sent.body.clone()))
                })
                .into_any_element(),
            2 => div()
                .id("response-headers")
                .size_full()
                .overflow_y_scroll()
                .children(
                    self.response
                        .headers
                        .iter()
                        .map(|(name, value)| detail(name, value)),
                )
                .into_any_element(),
            3 => detail("Cookies", "No response cookies").into_any_element(),
            _ => detail("Total", &format!("{} ms", self.response.elapsed_ms)).into_any_element(),
        }
    }
}

impl Render for HttpClient {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = palette::get(cx);
        let request_background = palette.background;
        let response_background = palette.toolbar;
        let separator = palette.separator;
        div()
            .size_full()
            .flex()
            .overflow_hidden()
            .font_family(cx.theme().font_family.clone())
            .text_color(palette.foreground)
            .child(
                div()
                    .w(relative(0.515))
                    .min_w_0()
                    .h_full()
                    .flex_none()
                    .flex()
                    .flex_col()
                    .bg(request_background)
                    .child(self.request_bar(cx))
                    .child(self.tabs(false, cx))
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .min_w_0()
                            .overflow_hidden()
                            .child(self.request_content(cx)),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .border_l_1()
                    .border_color(separator)
                    .bg(response_background)
                    .child(self.response_status(cx))
                    .child(self.tabs(true, cx))
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .min_w_0()
                            .overflow_hidden()
                            .child(self.response_content(cx)),
                    ),
            )
    }
}

fn detail(name: &str, value: &str) -> impl IntoElement {
    div()
        .flex()
        .gap_4()
        .px_3()
        .py_2()
        .text_sm()
        .child(div().flex_none().w_32().child(name.to_owned()))
        .child(div().min_w_0().flex_1().child(value.to_owned()))
}
