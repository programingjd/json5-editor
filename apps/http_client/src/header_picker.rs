use std::rc::Rc;

use gpui_kit::base::input::{Enter, InputEvent};
use gpui_kit::component::{
    ActiveTheme as _, Sizable as _, Size,
    button::{Button, ButtonVariants as _},
    input::InputState,
    popover::PopoverState,
    scroll::ScrollableElement as _,
};
use gpui_kit::{
    App, AppContext as _, Context, Entity, Focusable as _, InteractiveElement as _, IntoElement,
    ParentElement as _, Point, Render, ScrollHandle, StatefulInteractiveElement as _, Styled as _,
    Subscription, WeakEntity, Window, div, rems,
};

use crate::buttons::field_input;

const HEADERS: &[&str] = &[
    "Accept",
    "Accept-Charset",
    "Accept-Encoding",
    "Accept-Language",
    "Authorization",
    "Cache-Control",
    "Connection",
    "Content-Disposition",
    "Content-Encoding",
    "Content-Language",
    "Content-Length",
    "Content-Type",
    "Cookie",
    "Expect",
    "Forwarded",
    "Host",
    "If-Match",
    "If-Modified-Since",
    "If-None-Match",
    "If-Range",
    "If-Unmodified-Since",
    "Origin",
    "Pragma",
    "Prefer",
    "Range",
    "Referer",
    "User-Agent",
    "X-API-Key",
    "X-Requested-With",
];

type ChooseHeader = dyn Fn(Option<&'static str>, &mut Window, &mut App);

pub(super) struct HeaderPicker {
    search: Entity<InputState>,
    scroll: ScrollHandle,
    popover: WeakEntity<PopoverState>,
    choose: Rc<ChooseHeader>,
    _search_subscription: Subscription,
}

fn matching_headers(filter: &str) -> impl Iterator<Item = &'static str> + '_ {
    let filter = filter.trim().to_ascii_lowercase();
    HEADERS
        .iter()
        .copied()
        .filter(move |name| name.to_ascii_lowercase().contains(&filter))
}

impl HeaderPicker {
    pub(super) fn new(
        popover: WeakEntity<PopoverState>,
        choose: impl Fn(Option<&'static str>, &mut Window, &mut App) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search headers"));
        let focus = search.read(cx).focus_handle(cx);
        window.on_next_frame(move |window, cx| focus.focus(window, cx));
        let subscription = cx.subscribe(&search, |this, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                this.scroll.set_offset(Point::default());
                cx.notify();
            }
        });
        Self {
            search,
            scroll: ScrollHandle::new(),
            popover,
            choose: Rc::new(choose),
            _search_subscription: subscription,
        }
    }

    fn choose(&self, header: Option<&'static str>, window: &mut Window, cx: &mut App) {
        let _ = self
            .popover
            .update(cx, |state, cx| state.dismiss(window, cx));
        (self.choose)(header, window, cx);
    }
}

impl Render for HeaderPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let filter = self.search.read(cx).value();
        let muted = cx.theme().popover_foreground.opacity(0.8);
        let border = cx.theme().border.opacity(0.5);
        div()
            .id("header-picker")
            .w_56()
            .font_family(cx.theme().font_family.clone())
            .flex()
            .flex_col()
            .gap_1()
            .capture_action(cx.listener(|this, _: &Enter, window, cx| {
                cx.stop_propagation();
                let filter = this.search.read(cx).value();
                if filter.trim().is_empty() {
                    this.choose(None, window, cx);
                } else if let Some(name) = matching_headers(&filter).next() {
                    this.choose(Some(name), window, cx);
                }
            }))
            .child(
                div()
                    .border_1()
                    .border_color(border)
                    .rounded(cx.theme().radius)
                    .child(field_input(&self.search, "Search headers")),
            )
            .child(
                div()
                    .relative()
                    .child(
                        div()
                            .id("header-options")
                            .max_h(rems(13.0))
                            .track_scroll(&self.scroll)
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .child(option_button("Edit", None).on_click(
                                cx.listener(|this, _, window, cx| this.choose(None, window, cx)),
                            ))
                            .children(matching_headers(&filter).map(|name| {
                                option_button(name, Some(muted)).on_click(cx.listener(
                                    move |this, _, window, cx| this.choose(Some(name), window, cx),
                                ))
                            })),
                    )
                    .vertical_scrollbar(&self.scroll),
            )
    }
}

fn option_button(label: &'static str, suggestion_color: Option<gpui_kit::Hsla>) -> Button {
    let label_view = div().w_full().text_left().child(label);
    let label_view = match suggestion_color {
        Some(color) => label_view.italic().text_color(color),
        None => label_view,
    };
    Button::new(label)
        .ghost()
        .with_size(Size::Small)
        .w_full()
        .flex_none()
        .cursor_pointer()
        .accessibility_label(label)
        .child(label_view)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_headers_stay_sorted_when_filtered() {
        assert!(
            HEADERS
                .windows(2)
                .all(|pair| pair[0].to_ascii_lowercase() < pair[1].to_ascii_lowercase())
        );
        assert_eq!(
            matching_headers("cOnTenT-T").collect::<Vec<_>>(),
            ["Content-Type"]
        );
    }
}
