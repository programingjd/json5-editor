use gpui_kit::{AppContext as _, Application, ApplicationHandle};
use std::{borrow::Cow, rc::Rc};
use wasm_bindgen::JsValue;

pub(super) fn consume_custom_context_menu(handle: &ApplicationHandle) -> bool {
    handle.update(|cx| {
        let Some(window) = cx.windows().into_iter().next() else {
            return false;
        };
        cx.update_window(window, |_, window, cx| {
            gpui_kit::component::context_menu::consume_custom_context_menu(window, cx)
        })
        .unwrap_or(false)
    })
}

pub(super) fn web_application() -> Application {
    use gpui_kit::web::{CanvasFontFallback, WebBackendPreference, WebPlatform};
    use std::sync::Arc;

    let platform = Rc::new(WebPlatform::new_with_backend_and_font_fallback(
        false,
        WebBackendPreference::Auto,
        CanvasFontFallback::EmojiAndCjk,
    ));
    let http_client = Arc::new(platform.fetch_http_client());
    Application::with_platform(platform)
        .with_http_client(http_client)
        .with_assets(gpui_kit::assets::Assets::new(""))
}

pub(super) fn decode_font(family: &str, woff2: &[u8]) -> Result<Cow<'static, [u8]>, JsValue> {
    wuff::decompress_woff2(woff2)
        .map(Cow::Owned)
        .map_err(|error| {
            JsValue::from_str(&format!("Failed to decode {family} WOFF2 font: {error:?}"))
        })
}
