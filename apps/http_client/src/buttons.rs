use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _, Size,
    button::{Button, ButtonCustomVariant, ButtonVariants as _},
    input::{Input, InputState},
    menu::PopupMenuItem,
};
use gpui_kit::{
    AnyElement, App, ClickEvent, Div, ElementId, Entity, Hsla, InteractiveElement as _,
    IntoElement, ParentElement as _, Rems, Styled as _, Window, anchored, deferred, div, point,
    prelude::FluentBuilder as _, px, rems, rgb, svg,
};

use crate::palette;

const FIELD_ACTION: &str = "request-field-action";
const CHECK_ICON: &[u8] = include_bytes!("../../../assets/icons/check.svg");
const INHERITED_ICON: &[u8] =
    include_bytes!("../../../assets/icons/inherited-attribute-overlay.svg");
const FORCE_REFRESH_ICON: &[u8] = include_bytes!("../../../assets/icons/force-refresh.svg");
const INHERITANCE_ICON_COLOR: u32 = 0xb08e90;

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

pub(super) fn small_icon(data: &'static [u8], color: Hsla) -> Icon {
    Icon::default()
        .data(data)
        .with_size(Size::Small)
        .text_color(color)
}

pub(super) fn inherited_icon() -> Div {
    // JetBrains inheritedAttributeOverlay.svg places its 7x8 arrow at (0..7, 0..8)
    // inside a 16x16 viewBox; scale and offset it to sit centered in a Small (14px) box.
    div()
        .relative()
        .size_3p5()
        .flex_none()
        .overflow_hidden()
        .child(
            svg()
                .data(INHERITED_ICON)
                .absolute()
                .top(rems(0.09375))
                .left(rems(0.1367))
                .size(rems(1.375))
                .text_color(rgb(INHERITANCE_ICON_COLOR)),
        )
}

pub(super) fn reset_icon() -> Icon {
    small_icon(FORCE_REFRESH_ICON, rgb(INHERITANCE_ICON_COLOR).into())
}

pub(super) fn inheritance_button(
    id: impl Into<ElementId>,
    inherited: bool,
    override_label: &'static str,
    reset_label: &'static str,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> Button {
    let (tooltip, label) = if inherited {
        (
            "Inherited from defaults. Click to override.",
            override_label,
        )
    } else {
        ("Reset to default", reset_label)
    };
    action_button(id, cx)
        .with_size(Size::Small)
        .size_6()
        .p_0()
        .flex_none()
        .tooltip(tooltip)
        .accessibility_label(label)
        .map(|button| {
            if inherited {
                button.child(inherited_icon())
            } else {
                button.icon(reset_icon())
            }
        })
        .on_click(on_click)
}

pub(super) fn checked_menu_item(label: &'static str, checked: bool) -> PopupMenuItem {
    let item = PopupMenuItem::new(label).checked(checked);
    if checked {
        item.icon(Icon::default().data(CHECK_ICON))
    } else {
        item
    }
}

pub(super) fn field_input(state: &Entity<InputState>, label: &'static str) -> Input {
    Input::new(state)
        .with_size(Size::Small)
        .appearance(false)
        .bordered(false)
        .aria_label(label)
        .px_2()
        .w_full()
}

pub(super) fn modal_overlay(
    backdrop_id: &'static str,
    dialog: impl IntoElement,
    window: &Window,
    cx: &App,
) -> AnyElement {
    let viewport = window.viewport_size();
    let backdrop = div()
        .id(backdrop_id)
        .w(viewport.width)
        .h(viewport.height)
        .flex()
        .items_center()
        .justify_center()
        .occlude()
        .bg(cx.theme().muted.opacity(0.75))
        .child(dialog);
    deferred(anchored().position(point(px(0.), px(0.))).child(backdrop))
        .with_priority(2)
        .into_any_element()
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
