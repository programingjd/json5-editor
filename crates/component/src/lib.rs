#[cfg(feature = "gpui-fast")]
extern crate gpui_fast as gpui;

use gpui::App;
use std::ops::Deref;

mod component_traits;
mod element_ext;
pub mod global_state;
mod icon;
mod index_path;
mod root;
mod sizing;
mod styled;
mod touch_selection;
mod virtual_list;
mod window_ext;

pub(crate) mod actions {
    pub use gpui_base::actions::*;
}

pub mod button;
pub mod checkbox;
pub mod empty;
pub mod highlighter;
pub mod history;
pub mod input;
pub mod kbd;
pub mod label;
pub mod link;
pub mod list;
pub mod menu;
pub mod context_menu;
pub mod popover;
pub mod scroll;
pub mod separator;
pub mod skeleton;
pub mod spinner;
pub mod status_bar;
pub mod text;
pub mod theme;
pub mod toolbar;
pub mod tooltip;

pub use crate::Disableable;
pub use element_ext::*;
pub use global_state::GlobalState;
pub use gpui_base::Root;
pub use gpui_base::animation;
pub use gpui_base::{
    AxisExt, Edges, FocusTrapElement, InteractiveElementExt, LengthExt, Measure, OngoingScrollExt,
    Placement, Side, measure, measure_if,
};
pub use icon::*;
pub use index_path::IndexPath;
pub use input::{Rope, RopeExt, RopeLines};
pub use styled::*;
pub use theme::*;
pub use virtual_list::{VirtualList, VirtualListScrollHandle, h_virtual_list, v_virtual_list};
pub use window_ext::WindowExt;

rust_i18n::i18n!("locales", fallback = "en");

pub fn init(cx: &mut App) {
    theme::init(cx);
    global_state::init(cx);
    root::init(cx);
    gpui_base::init(cx);
    input::init(cx);
    list::init(cx);
    popover::init(cx);
    menu::init(cx);
    tooltip::init(cx);
}
pub fn locale() -> impl Deref<Target = str> {
    rust_i18n::locale()
}
pub fn set_locale(locale: &str) {
    rust_i18n::set_locale(locale)
}
