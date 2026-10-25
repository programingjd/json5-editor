//! The editor's local facade over its retained GPUI Kit components and GPUI.
#[doc(hidden)]
pub use crate as gpui;
pub use ::gpui::*;
pub use ::gpui_base as base;
pub use ::gpui_component as component;
pub use ::gpui_kit_assets as assets;
pub use ::gpui_platform as platform;
#[cfg(target_family = "wasm")]
pub use ::gpui_web as web;

pub fn init(cx: &mut App) {
    component::init(cx);
}

pub fn open_window<V: Render>(
    options: WindowOptions,
    cx: &mut App,
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
) -> Result<(AnyWindowHandle, Entity<V>)> {
    let mut built = None;
    let window = cx.open_window(options, |window, cx| {
        let view = build(window, cx);
        built = Some(view.clone());
        cx.new(|cx| base::Root::new(view, window, cx))
    })?;
    Ok((window.into(), built.expect("window builder ran")))
}

#[macro_export]
macro_rules! actions {
    ($namespace:path, [ $( $(#[$attr:meta])* $name:ident),* $(,)? ]) => {
        $(#[derive(::std::clone::Clone, ::std::cmp::PartialEq, ::std::default::Default,
            ::std::fmt::Debug, $crate::Action)]
        #[action(namespace = $namespace)] $(#[$attr])* pub struct $name;)*
    };
}
