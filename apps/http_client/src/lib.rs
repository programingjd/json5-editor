//! HTTP client application.

use std::{cell::RefCell, sync::Arc};

use gpui_kit::component::theme::Theme;
use gpui_kit::{App, AppContext as _, ApplicationHandle, Entity, Global, Hsla, WindowOptions, rgb};
use js_sys::Function;
use wasm_bindgen::prelude::*;

mod client;
#[path = "../../editor/src/palette.rs"]
mod palette;
#[path = "../../editor/src/syntax_data.rs"]
mod syntax_data;
#[path = "../../editor/src/tree_sitter_highlighter.rs"]
mod tree_sitter_highlighter;
#[path = "../../editor/src/web_support.rs"]
mod web_support;

use client::{ClientCallbacks, HttpClient, Workspace};

thread_local! {
    static APPLICATION: RefCell<Option<ApplicationHandle>> = const { RefCell::new(None) };
}

struct ClientView(Entity<Workspace>);
impl Global for ClientView {}

#[wasm_bindgen]
pub fn run(
    dark: bool,
    inter: Vec<u8>,
    mono: Vec<u8>,
    inter_italic: Vec<u8>,
    send: Function,
    queue_edit: Function,
    load_request: Function,
    ready: Function,
    preview: String,
) -> Result<(), JsValue> {
    console_error_panic_hook::set_once();
    let fonts = vec![
        web_support::decode_font("Inter", &inter)?,
        web_support::decode_font("Inter Italic", &inter_italic)?,
        web_support::decode_font("CommitMono", &mono)?,
    ];
    let preview =
        serde_json::from_str(&preview).map_err(|error| JsValue::from_str(&error.to_string()))?;
    gpui_kit::web::init_logging();
    let app = web_support::web_application().run_embedded(move |cx| {
        gpui_kit::init(cx);
        cx.text_system()
            .add_fonts(fonts)
            .expect("Register HTTP client fonts");
        apply_theme(dark, cx);
        let (_, view) = gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
            let callbacks = ClientCallbacks {
                send,
                queue_edit,
                load_request,
            };
            cx.new(|cx| Workspace::new(window, cx, callbacks, preview))
        })
        .expect("Open HTTP client window");
        cx.set_global(ClientView(view));
        cx.activate(true);
        let _ = ready.call0(&JsValue::UNDEFINED);
    });
    APPLICATION.with(|application| *application.borrow_mut() = Some(app));
    Ok(())
}

const WORKSPACE_TAB_COLORS: [u32; 10] = [
    0x66c5cc, 0xf6cf71, 0xf89c74, 0xdcb0f2, 0x87c55f, 0x9eb9f3, 0xfe88b1, 0xc9db74, 0x8be0a4,
    0xb497e7,
];
const WORKSPACE_TAB_FOREGROUND: u32 = 0x2b2b2b;

fn workspace_tab_background(index: usize) -> Hsla {
    rgb(WORKSPACE_TAB_COLORS[index % WORKSPACE_TAB_COLORS.len()]).into()
}

fn workspace_tab_foreground() -> Hsla {
    rgb(WORKSPACE_TAB_FOREGROUND).into()
}

fn apply_theme(dark: bool, cx: &mut App) {
    palette::apply_theme(dark, cx);
    Theme::update(cx, |theme| {
        theme.colors.tab_active = rgb(0x553d66).into();
        theme.colors.tab_active_foreground = rgb(0xa9b7c6).into();
        if dark {
            let current_line = Hsla::from(rgb(0x524b5a)).opacity(0.5);
            Arc::make_mut(&mut theme.highlight_theme)
                .style
                .editor_active_line = Some(current_line);
        }
    });
}

fn update_app(update: impl FnOnce(&mut App)) {
    APPLICATION.with(|application| {
        if let Some(handle) = application.borrow().as_ref() {
            handle.update(update);
        }
    });
}

fn update_view<T>(
    tab_id: u32,
    update: impl FnOnce(&mut HttpClient, &mut gpui_kit::Context<HttpClient>) -> T,
) -> Option<T> {
    let mut value = None;
    update_app(|cx| {
        if let Some(view) = client_tab(tab_id, cx) {
            value = Some(view.update(cx, update));
        }
    });
    value
}

fn client_tab(tab_id: u32, cx: &App) -> Option<Entity<HttpClient>> {
    cx.try_global::<ClientView>()?.0.read(cx).tab(tab_id)
}

#[wasm_bindgen]
pub fn set_color_scheme(dark: bool) {
    update_app(|cx| {
        if cx.has_global::<ClientView>() {
            apply_theme(dark, cx);
            cx.refresh_windows();
        }
    });
}

#[wasm_bindgen]
pub fn begin_response(tab_id: u32) {
    update_view(tab_id, HttpClient::begin_response);
}

#[wasm_bindgen]
pub fn finish_request_error(tab_id: u32, error: String) {
    update_view(tab_id, |view, cx| view.finish_request_error(error, cx));
}

#[wasm_bindgen]
pub fn set_request_loading(tab_id: u32, loading: bool) {
    update_view(tab_id, |view, cx| view.set_request_loading(loading, cx));
}

#[wasm_bindgen]
pub fn prepare_body(tab_id: u32, response: bool, id: u32, contents: String) -> bool {
    update_view(tab_id, |view, cx| {
        view.prepare_body(response, id, contents, cx)
    })
    .is_some()
}

#[wasm_bindgen]
pub fn advance_body_preparation(tab_id: u32, response: bool, id: u32) -> i32 {
    update_view(tab_id, |view, cx| {
        view.advance_body_preparation(response, id, cx)
    })
    .unwrap_or(-1)
}

#[wasm_bindgen]
pub fn body_preparation_progress(tab_id: u32, response: bool, id: u32) -> Vec<u32> {
    update_view(tab_id, |view, _| {
        view.body_preparation_progress(response, id)
    })
    .unwrap_or_default()
}

#[wasm_bindgen]
pub fn cancel_body_preparation(tab_id: u32, response: bool, id: u32) {
    update_view(tab_id, |view, _| view.cancel_body_preparation(response, id));
}

#[wasm_bindgen]
pub fn install_body(
    tab_id: u32,
    response: bool,
    metadata: String,
    highlights: Vec<u32>,
    folds: Vec<u32>,
    brackets: Vec<u32>,
    diagnostics: String,
) -> bool {
    let mut installed = false;
    update_app(|cx| {
        let Some(view) = client_tab(tab_id, cx) else {
            return;
        };
        let Some(window) = cx.windows().into_iter().next() else {
            return;
        };
        let _ = cx.update_window(window, |_, window, cx| {
            view.update(cx, |view, cx| {
                installed = view.install_body(
                    response,
                    &metadata,
                    &highlights,
                    &folds,
                    &brackets,
                    &diagnostics,
                    window,
                    cx,
                );
            });
        });
    });
    installed
}

#[wasm_bindgen]
pub fn apply_syntax(
    tab_id: u32,
    revision: u32,
    highlights: Vec<u32>,
    folds: Vec<u32>,
    brackets: Vec<u32>,
    diagnostics: String,
) {
    update_view(tab_id, |view, cx| {
        view.apply_syntax(revision, &highlights, &folds, &brackets, &diagnostics, cx);
    });
}
