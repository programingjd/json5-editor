use gpui_kit::base::{SelectableText, TextSelection, TextSelectionHandle, input::Copy};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, IndexPath, Selectable, Sizable as _, Size,
    input::{Input, InputEvent, InputState},
    list::{List, ListDelegate, ListState},
};
use gpui_kit::{
    App, AppContext as _, ClipboardItem, Context, CursorStyle, Entity, FocusHandle,
    InteractiveElement as _, IntoElement, KeyBinding, ParentElement as _, Render, RenderOnce,
    SharedString, Styled as _, Subscription, Task, Window, div, rems,
};

use crate::{
    DETAIL_PANE_MAX_WIDTH_REMS,
    buttons::{field_button, field_button_icon},
};

const CONTEXT: &str = "ResponseHeaders";

pub(super) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-c", Copy, Some(CONTEXT)),
        KeyBinding::new("cmd-c", Copy, Some(CONTEXT)),
    ]);
}

struct Header {
    name: SharedString,
    value: SharedString,
    search_text: String,
    name_selection: TextSelectionHandle,
    value_selection: TextSelectionHandle,
}

struct HeaderDelegate {
    headers: Vec<Header>,
    visible: Vec<usize>,
    query: String,
    focus: FocusHandle,
}

impl HeaderDelegate {
    fn set_headers(&mut self, headers: &[(String, String)], cx: &mut App) {
        self.headers = headers
            .iter()
            .map(|(name, value)| {
                let text = format!("{name}: {value}");
                let selections = [name, value].map(|text| {
                    let selection = TextSelectionHandle::new(text.clone(), cx);
                    let focus = self.focus.clone();
                    selection.focus_with(move |window, cx| focus.focus(window, cx), cx);
                    selection
                });
                let [name_selection, value_selection] = selections;
                Header {
                    search_text: text.to_lowercase(),
                    name: name.clone().into(),
                    value: value.clone().into(),
                    name_selection,
                    value_selection,
                }
            })
            .collect();
        self.filter();
    }

    fn filter(&mut self) {
        self.visible = self
            .headers
            .iter()
            .enumerate()
            .filter_map(|(index, header)| header.search_text.contains(&self.query).then_some(index))
            .collect();
    }
}

impl ListDelegate for HeaderDelegate {
    type Item = HeaderRow;

    fn items_count(&self, _: usize, _: &App) -> usize {
        self.visible.len()
    }

    fn perform_search(
        &mut self,
        query: &str,
        window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Task<()> {
        TextSelection::clear(window, cx);
        self.query = query.to_lowercase();
        self.filter();
        cx.notify();
        Task::ready(())
    }

    fn render_item(
        &mut self,
        index: IndexPath,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) -> Option<Self::Item> {
        let id = *self.visible.get(index.row)?;
        let header = &self.headers[id];
        Some(HeaderRow {
            id,
            name: header.name.clone(),
            value: header.value.clone(),
            name_selection: header.name_selection.clone(),
            value_selection: header.value_selection.clone(),
        })
    }

    fn set_selected_index(
        &mut self,
        _: Option<IndexPath>,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) {
    }

    fn render_empty(
        &mut self,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        div()
            .p_3()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(if self.headers.is_empty() {
                "No response headers"
            } else {
                "No matching headers"
            })
    }
}

#[derive(IntoElement)]
struct HeaderRow {
    id: usize,
    name: SharedString,
    value: SharedString,
    name_selection: TextSelectionHandle,
    value_selection: TextSelectionHandle,
}

impl Selectable for HeaderRow {
    fn selected(self, _: bool) -> Self {
        self
    }
    fn is_selected(&self) -> bool {
        false
    }
}

impl RenderOnce for HeaderRow {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        div()
            .id(("response-header", self.id))
            .w_full()
            .h_8()
            .flex()
            .items_center()
            .gap(rems(2.0))
            .px_3()
            .py_1()
            .text_sm()
            .cursor(CursorStyle::IBeam)
            .child(
                div()
                    .id(("header-name-cell", self.id))
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_left()
                    .child(
                        SelectableText::with_handle(
                            ("header-name", self.id),
                            self.name_selection,
                            self.name,
                        )
                        .document_order(self.id as u64 * 2)
                        .selection_color(cx.theme().selection),
                    ),
            )
            .child(
                div()
                    .id(("header-value-cell", self.id))
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .flex()
                    .justify_end()
                    .text_left()
                    .font_family(cx.theme().mono_font_family.clone())
                    .child(
                        SelectableText::with_handle(
                            ("header-value", self.id),
                            self.value_selection,
                            self.value,
                        )
                        .document_order(self.id as u64 * 2 + 1)
                        .selection_color(cx.theme().selection),
                    ),
            )
    }
}

pub(super) struct ResponseHeaders {
    list: Entity<ListState<HeaderDelegate>>,
    query_input: Entity<InputState>,
    focus: FocusHandle,
    _query_subscription: Subscription,
}

impl ResponseHeaders {
    pub(super) fn new(
        headers: &[(String, String)],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        let mut delegate = HeaderDelegate {
            headers: Vec::new(),
            visible: Vec::new(),
            query: String::new(),
            focus: focus.clone(),
        };
        delegate.set_headers(headers, cx);
        let list = cx.new(|cx| ListState::new(delegate, window, cx).selectable(false));
        let query_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search response headers"));
        let query_subscription =
            cx.subscribe_in(&query_input, window, |this, input, event, window, cx| {
                if let InputEvent::Change = event {
                    let query = input.read(cx).value();
                    this.list
                        .update(cx, |list, cx| list.set_query(&query, window, cx));
                    cx.notify();
                }
            });
        Self {
            list,
            query_input,
            focus,
            _query_subscription: query_subscription,
        }
    }

    pub(super) fn set_headers(
        &mut self,
        headers: &[(String, String)],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        TextSelection::clear(window, cx);
        self.list.update(cx, |list, cx| {
            list.delegate_mut().set_headers(headers, cx);
            cx.notify();
        });
    }

    pub(super) fn selected_text(&self, window: &mut Window, cx: &mut App) -> String {
        if self.focus.is_focused(window) {
            TextSelection::selected_text(window, cx)
        } else {
            String::new()
        }
    }
}

impl Render for ResponseHeaders {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("response-headers")
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .max_w(rems(DETAIL_PANE_MAX_WIDTH_REMS))
            .mx_auto()
            .min_h_0()
            .font_family(cx.theme().font_family.clone())
            .capture_action({
                let focus = self.focus.clone();
                move |_: &Copy, window, cx| {
                    if focus.is_focused(window) {
                        let text = TextSelection::selected_text(window, cx);
                        if !text.is_empty() {
                            cx.write_to_clipboard(ClipboardItem::new_string(text));
                            cx.stop_propagation();
                        }
                    }
                }
            })
            .child(
                div()
                    .px_3()
                    .pt(rems(0.5))
                    .mb(rems(0.5))
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        Input::new(&self.query_input)
                            .with_size(Size::Small)
                            .suffix(
                                field_button("clear-response-header-search", cx)
                                    .with_size(Size::Small)
                                    .cursor_pointer()
                                    .disabled(self.query_input.read(cx).value().is_empty())
                                    .accessibility_label("Clear header search")
                                    .child(field_button_icon(
                                        include_bytes!("../../../assets/icons/close.svg"),
                                        rems(0.875),
                                        cx.theme().foreground,
                                        !self.query_input.read(cx).value().is_empty(),
                                    ))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.query_input.update(cx, |input, cx| {
                                            input.clean(window, cx);
                                            input.focus(window, cx);
                                        });
                                    })),
                            )
                            .appearance(false)
                            .p_0(),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(List::new(&self.list).with_size(Size::Small)),
            )
    }
}
