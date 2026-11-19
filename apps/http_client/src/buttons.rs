use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_kit::{
    App, Div, ElementId, Hsla, InteractiveElement as _, IntoElement, ParentElement as _, Rems,
    Styled as _, div, prelude::FluentBuilder as _, rgb, svg,
};

use crate::palette;

const FIELD_ACTION: &str = "request-field-action";

pub(super) fn action_button(id: impl Into<ElementId>, cx: &App) -> Button {
    let palette = palette::get(cx);
    Button::new(id)
        .custom(
            ButtonCustomVariant::new(cx)
                .hover(palette.hover)
                .active(palette.active),
        )
        .cursor_pointer()
}

pub(super) fn field_button(id: impl Into<ElementId>, cx: &App) -> Button {
    Button::new(id)
        .custom(ButtonCustomVariant::new(cx))
        .group(FIELD_ACTION)
}

pub(super) fn field_button_icon(
    data: &[u8],
    size: Rems,
    color: Hsla,
    enabled: bool,
) -> impl IntoElement {
    svg()
        .data(data)
        .id("field-button-icon")
        .size(size)
        .text_color(color)
        .when(enabled, |icon| {
            icon.group_hover(FIELD_ACTION, |style| style.text_color(rgb(0xffffff)))
        })
}

pub(super) fn field_button_label(label: &'static str, color: Hsla) -> Div {
    div()
        .relative()
        .child(
            div()
                .id("field-button-label")
                .text_color(color)
                .group_hover(FIELD_ACTION, |style| style.opacity(0.0))
                .child(label),
        )
        .child(
            div()
                .id("field-button-hover-label")
                .absolute()
                .inset_0()
                .opacity(0.0)
                .group_hover(FIELD_ACTION, |style| style.opacity(1.0))
                .text_color(rgb(0xffffff))
                .child(label),
        )
}
