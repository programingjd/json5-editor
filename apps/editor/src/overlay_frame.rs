use gpui_kit::component::{
    ActiveTheme as _,
    scroll::{Scrollable, ScrollableElement as _},
};
use gpui_kit::{
    App, Div, InteractiveElement as _, MouseButton, ParentElement as _, Stateful, Styled as _, div,
};

pub(super) fn root(id: &'static str, cx: &App) -> Stateful<Div> {
    div()
        .id(id)
        .absolute()
        .inset_0()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .occlude()
        .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
        .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_mouse_move(|_, _, cx| cx.stop_propagation())
        .on_scroll_wheel(|_, window, cx| {
            window.prevent_default();
            cx.stop_propagation();
        })
        .font_family(cx.theme().font_family.clone())
        .text_color(cx.theme().popover_foreground)
        .child(
            div()
                .absolute()
                .inset_0()
                .size_full()
                .bg(cx.theme().muted.opacity(0.75)),
        )
}

pub(super) fn panel(cx: &App) -> Div {
    div()
        .relative()
        .max_w_full()
        .flex_shrink_0()
        .p_12()
        .bg(super::editor_overlay_background(cx))
        .border_1()
        .border_color(cx.theme().border.opacity(0.5))
        .rounded(cx.theme().radius)
        .flex()
        .flex_col()
}

pub(super) fn page(id: &'static str) -> Scrollable<Stateful<Div>> {
    div()
        .id(id)
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .p_4()
        .overflow_y_scrollbar()
}
