use std::{cell::Cell, rc::Rc};

use gpui_kit::base::{
    ElementExt as _, FocusTrapElement as _, SelectableText, TextSelection, TextSelectionHandle,
    TextSelectionScopeId, input::Copy,
};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, IndexPath, Selectable, Sizable as _, Size,
    button::{Button, ButtonVariants as _},
    input::{Input, InputEvent, InputState},
    list::{List, ListDelegate, ListState},
};
use gpui_kit::{
    AnyElement, App, AppContext as _, ClipboardItem, Context, CursorStyle, Entity, FocusHandle,
    InteractiveElement as _, IntoElement, KeyBinding, KeyDownEvent, ParentElement as _, Pixels,
    Render, RenderOnce, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription,
    Task, WeakEntity, Window, accesskit::Role, canvas, div, rems,
};

use crate::{
    DETAIL_PANE_MAX_WIDTH_REMS,
    buttons::{action_button, field_button, field_button_icon, modal_overlay, small_icon},
};

const CONTEXT: &str = "ResponseHeaders";
const ROW_INSET_REMS: f32 = 0.75;
const COLUMN_GAP_REMS: f32 = 2.0;
const VALUE_FONT_SIZE_REMS: f32 = 0.875;

fn copy_selection(window: &mut Window, cx: &mut App) {
    let text = TextSelection::selected_text(window, cx);
    if !text.is_empty() {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        cx.stop_propagation();
    }
}

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
    owner: WeakEntity<ResponseHeaders>,
    list_width: Rc<Cell<Option<Pixels>>>,
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
            owner: self.owner.clone(),
            list_width: self.list_width.get(),
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
    owner: WeakEntity<ResponseHeaders>,
    list_width: Option<Pixels>,
}

impl HeaderRow {
    fn render_value(&self, window: &Window, cx: &App) -> AnyElement {
        let font_family = cx.theme().mono_font_family.clone();
        let rem = window.rem_size();
        let overflows = self.list_width.is_some_and(|width| {
            let available = width * 0.5 - rem * (ROW_INSET_REMS + COLUMN_GAP_REMS * 0.5);
            let mut style = window.text_style();
            style.font_family = font_family.clone();
            let line = window.text_system().layout_line(
                &self.value,
                rem * VALUE_FONT_SIZE_REMS,
                &[style.to_run(self.value.len())],
                None,
            );
            line.width > available
        });
        if !overflows {
            return SelectableText::with_handle(
                ("header-value", self.id),
                self.value_selection.clone(),
                self.value.clone(),
            )
            .document_order(self.id as u64 * 2 + 1)
            .selection_color(cx.theme().selection)
            .into_any_element();
        }

        let value = self.value.clone();
        let name = self.name.clone();
        let owner = self.owner.clone();
        Button::new(("show-full-header-value", self.id))
            .text()
            .w_full()
            .min_w_0()
            .p_0()
            .h_6()
            .cursor_pointer()
            .accessibility_label(format!("Show full {} header value", self.name))
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .truncate()
                    .text_right()
                    .text_size(rems(VALUE_FONT_SIZE_REMS))
                    .child(self.value.clone()),
            )
            .on_click(move |_, window, cx| {
                let _ = owner.update(cx, |this, cx| {
                    this.open_value(name.clone(), value.clone(), window, cx);
                });
            })
            .into_any_element()
    }
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
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let value = self.render_value(window, cx);
        div()
            .id(("response-header", self.id))
            .w_full()
            .h_8()
            .flex()
            .items_center()
            .gap(rems(COLUMN_GAP_REMS))
            .px(rems(ROW_INSET_REMS))
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
                    .text_size(rems(VALUE_FONT_SIZE_REMS))
                    .child(value),
            )
    }
}

pub(super) struct ResponseHeaders {
    list: Entity<ListState<HeaderDelegate>>,
    query_input: Entity<InputState>,
    focus: FocusHandle,
    modal_focus: FocusHandle,
    modal_value: Option<(SharedString, SharedString)>,
    previous_focus: Option<FocusHandle>,
    inactive_selection_scope: TextSelectionScopeId,
    list_width: Rc<Cell<Option<Pixels>>>,
    _query_subscription: Subscription,
}

impl ResponseHeaders {
    pub(super) fn new(
        headers: &[(String, String)],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        let list_width = Rc::new(Cell::new(None));
        let mut delegate = HeaderDelegate {
            headers: Vec::new(),
            visible: Vec::new(),
            query: String::new(),
            focus: focus.clone(),
            owner: cx.weak_entity(),
            list_width: list_width.clone(),
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
            modal_focus: cx.focus_handle(),
            modal_value: None,
            previous_focus: None,
            inactive_selection_scope: TextSelectionScopeId::new(),
            list_width,
            _query_subscription: query_subscription,
        }
    }

    pub(super) fn set_headers(
        &mut self,
        headers: &[(String, String)],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_value(window, cx);
        self.list.update(cx, |list, cx| {
            list.delegate_mut().set_headers(headers, cx);
            cx.notify();
        });
    }

    pub(super) fn selected_text(&self, window: &mut Window, cx: &mut App) -> String {
        if self.focus.is_focused(window) || self.modal_focus.contains_focused(window, cx) {
            TextSelection::selected_text(window, cx)
        } else {
            String::new()
        }
    }

    pub(super) fn has_modal_open(&self) -> bool {
        self.modal_value.is_some()
    }

    fn open_value(
        &mut self,
        name: SharedString,
        value: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        TextSelection::clear(window, cx);
        self.previous_focus = window.focused(cx);
        self.modal_value = Some((name, value));
        self.modal_focus.focus(window, cx);
        cx.notify();
    }

    fn close_value(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        TextSelection::clear(window, cx);
        if self.modal_value.take().is_some() {
            if let Some(previous) = self.previous_focus.take() {
                previous.focus(window, cx);
            }
            cx.notify();
        }
    }

    fn value_dialog(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (name, value) = self.modal_value.as_ref()?;
        let viewport = window.viewport_size();
        let dialog = div()
            .id("header-value-dialog")
            .role(Role::Dialog)
            .aria_label(format!("{name} header value"))
            .key_context(CONTEXT)
            .capture_action(|_: &Copy, window, cx| copy_selection(window, cx))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    window.prevent_default();
                    cx.stop_propagation();
                    this.close_value(window, cx);
                }
            }))
            .w(rems(36.0))
            .max_w(viewport.width * 0.9)
            .flex()
            .flex_col()
            .gap_2()
            .p_4()
            .bg(cx.theme().popover)
            .text_color(cx.theme().popover_foreground)
            .border_1()
            .border_color(cx.theme().border)
            .rounded(cx.theme().radius)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(div().min_w_0().truncate().child(name.clone()))
                    .child(
                        action_button("close-header-value", cx)
                            .with_size(Size::Small)
                            .accessibility_label("Close header value")
                            .icon(small_icon(
                                include_bytes!("../../../assets/icons/close.svg"),
                                cx.theme().popover_foreground,
                            ))
                            .on_click(
                                cx.listener(|this, _, window, cx| this.close_value(window, cx)),
                            ),
                    ),
            )
            .child(
                div()
                    .id("header-value-scroll")
                    .w_full()
                    .max_h(viewport.height * 0.65)
                    .overflow_y_scroll()
                    .cursor(CursorStyle::IBeam)
                    .whitespace_normal()
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_size(rems(VALUE_FONT_SIZE_REMS))
                    .child(
                        SelectableText::new("full-header-value-text", value.clone())
                            .selection_color(cx.theme().selection),
                    ),
            )
            .focus_trap("header-value-focus-trap", &self.modal_focus);
        Some(modal_overlay("header-value-backdrop", dialog, window, cx))
    }
}

impl Render for ResponseHeaders {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dialog = self.value_dialog(window, cx);
        // Keep the background visible while excluding its text from modal selections.
        let list_scope = if dialog.is_some() {
            self.inactive_selection_scope
        } else {
            TextSelectionScopeId::default()
        };
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
                        copy_selection(window, cx);
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
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child({
                        let list_width = self.list_width.clone();
                        canvas(
                            move |bounds, window, _| {
                                if list_width.replace(Some(bounds.size.width))
                                    != Some(bounds.size.width)
                                {
                                    window.refresh();
                                }
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .inset_0()
                    })
                    .child(
                        div()
                            .size_full()
                            .child(List::new(&self.list).with_size(Size::Small))
                            .text_selection_scope(list_scope),
                    ),
            )
            .children(dialog)
    }
}
