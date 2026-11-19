use std::{cell::Cell, rc::Rc};

use gpui_kit::base::input::InputEvent;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _, Size,
    input::{Input, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
    popover::Popover,
    scroll::ScrollableElement as _,
};
use gpui_kit::{
    App, AppContext as _, Context, CursorStyle, Div, Entity, EntityId, Focusable as _, Hsla,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Pixels, Render,
    ScrollHandle, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Window,
    accesskit::Role, div, prelude::FluentBuilder as _, rems,
};

use crate::{
    buttons::{action_button, field_button, field_button_icon},
    header_picker::HeaderPicker,
    query::{decode_pair, encode_pair, split_url, with_query},
};

const DRAG_ICON: &[u8] = include_bytes!("../../../assets/icons/drag.svg");
const DELETE_ICON: &[u8] = include_bytes!("../../../assets/icons/delete.svg");
const ADD_ICON: &[u8] = include_bytes!("../../../assets/icons/create-column.svg");
const CHEVRON_ICON: &[u8] = include_bytes!("../../../assets/icons/chevron-down.svg");
const CHECK_ICON: &[u8] = include_bytes!("../../../assets/icons/check.svg");

struct ParamRow {
    id: usize,
    key: Entity<InputState>,
    value: Entity<InputState>,
    has_value: bool,
    editing_header: bool,
    original: Option<String>,
    _subscriptions: [Subscription; 2],
}

pub(super) struct RequestFields {
    url: Option<Entity<InputState>>,
    last_query: Option<String>,
    rows: Vec<ParamRow>,
    next_id: usize,
    row_width: Rc<Cell<Pixels>>,
    dragged_row: Rc<Cell<Option<usize>>>,
    scroll: ScrollHandle,
    _url_subscription: Option<Subscription>,
}

impl RequestFields {
    pub(super) fn query(
        url: Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscription = cx.subscribe_in(&url, window, |this, _, event, window, cx| {
            if matches!(event, InputEvent::Change) {
                this.sync_from_url(window, cx);
            }
        });
        let mut params = Self::new(Some(url));
        params._url_subscription = Some(subscription);
        params.sync_from_url(window, cx);
        params.ensure_empty_row(window, cx);
        params
    }

    pub(super) fn headers(headers: &str, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut fields = Self::new(None);
        for line in headers.lines().filter(|line| !line.trim().is_empty()) {
            let row = fields.new_row(Some(line), window, cx);
            fields.rows.push(row);
        }
        fields.ensure_empty_row(window, cx);
        fields
    }

    pub(super) fn header_text(&self, cx: &App) -> String {
        self.rows
            .iter()
            .filter_map(|row| {
                let key = row.key.read(cx).value();
                let key = key.trim();
                (!key.is_empty()).then(|| format!("{key}: {}", row.value.read(cx).value()))
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub(super) fn set_content_type(
        &mut self,
        content_type: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut existing = None;
        self.rows.retain(|row| {
            let name = row.key.read(cx).value();
            if !name.trim().eq_ignore_ascii_case("Content-Type") {
                return true;
            }
            if content_type.is_none() || existing.is_some() {
                return false;
            }
            existing = Some(row.id);
            true
        });
        if let Some(content_type) = content_type {
            let row = self.rows.iter_mut().find(|row| {
                if let Some(id) = existing {
                    row.id == id
                } else {
                    row.key.read(cx).value().is_empty() && row.value.read(cx).value().is_empty()
                }
            });
            if let Some(row) = row {
                if existing.is_none() {
                    row.editing_header = false;
                    update_field(&row.key, "Content-Type".to_owned(), window, cx);
                }
                update_field(&row.value, content_type.to_owned(), window, cx);
            } else {
                let row = self.new_row(Some(&format!("Content-Type: {content_type}")), window, cx);
                self.rows.push(row);
            }
        }
        self.ensure_empty_row(window, cx);
        cx.notify();
    }

    fn new(url: Option<Entity<InputState>>) -> Self {
        Self {
            url,
            last_query: None,
            rows: Vec::new(),
            next_id: 0,
            row_width: Rc::default(),
            dragged_row: Rc::default(),
            scroll: ScrollHandle::new(),
            _url_subscription: None,
        }
    }

    fn new_row(
        &mut self,
        pair: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> ParamRow {
        let id = self.next_id;
        self.next_id += 1;
        let headers = self.url.is_none();
        let (key_text, value_text) = pair
            .map(|pair| {
                if headers {
                    let (key, value) = pair.split_once(':').unwrap_or((pair, ""));
                    (key.trim().to_owned(), Some(value.trim().to_owned()))
                } else {
                    decode_pair(pair)
                }
            })
            .unwrap_or_default();
        let key = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(if headers { "Header Name" } else { "Key" })
                .default_value(key_text)
        });
        let has_value = headers || value_text.is_some();
        let value = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Value")
                .default_value(value_text.unwrap_or_default())
        });
        let subscriptions = [&key, &value].map(|field| {
            cx.subscribe_in(field, window, |this, _, event, window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.sync_to_url(window, cx);
                }
            })
        });
        ParamRow {
            id,
            key,
            value,
            has_value,
            editing_header: false,
            original: pair.map(str::to_owned),
            _subscriptions: subscriptions,
        }
    }

    fn sync_from_url(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(url) = &self.url else {
            return;
        };
        let url = url.read(cx).value();
        let (_, query, _) = split_url(&url);
        if self.last_query.as_deref() == query {
            return;
        }
        self.last_query = query.map(str::to_owned);
        let pairs = query
            .into_iter()
            .flat_map(|query| query.split('&'))
            .filter(|pair| !pair.is_empty());
        let mut count = 0;
        for (index, pair) in pairs.enumerate() {
            count += 1;
            if let Some(row) = self.rows.get_mut(index) {
                if row.original.as_deref() == Some(pair) {
                    continue;
                }
                let (key, value) = decode_pair(pair);
                update_field(&row.key, key, window, cx);
                row.has_value = value.is_some();
                update_field(&row.value, value.unwrap_or_default(), window, cx);
                row.original = Some(pair.to_owned());
            } else {
                let row = self.new_row(Some(pair), window, cx);
                self.rows.push(row);
            }
        }
        self.rows.truncate(count);
        self.ensure_empty_row(window, cx);
        cx.notify();
    }

    fn ensure_empty_row(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.rows.is_empty() {
            let row = self.new_row(None, window, cx);
            self.rows.push(row);
        }
    }

    fn sync_to_url(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(url_field) = &self.url else {
            cx.notify();
            return;
        };
        let mut query = String::new();
        for row in &mut self.rows {
            let key = row.key.read(cx).value();
            let value = row.has_value.then(|| row.value.read(cx).value());
            row.original = encode_pair(&key, value.as_deref(), row.original.as_deref());
            if let Some(pair) = &row.original {
                if !query.is_empty() {
                    query.push('&');
                }
                query.push_str(pair);
            }
        }
        let url = url_field.read(cx).value();
        let query = (!query.is_empty()).then_some(query);
        let next_url = with_query(&url, query.as_deref());
        self.last_query = query;
        if next_url != url.as_str() {
            url_field.update(cx, |state, cx| state.replace_all(next_url, window, cx));
        }
        cx.notify();
    }

    fn add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let row = self.new_row(None, window, cx);
        if self.url.is_some() {
            row.key.read(cx).focus_handle(cx).focus(window, cx);
        }
        self.rows.push(row);
        self.scroll.scroll_to_bottom();
        cx.notify();
    }

    fn move_row(
        &mut self,
        source: usize,
        target: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if source != target {
            let row = self.rows.remove(source);
            self.rows.insert(target, row);
            self.sync_to_url(window, cx);
        }
    }

    fn row(&self, row: &ParamRow, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let id = row.id;
        let list = cx.entity_id();
        let hover = crate::palette::get(cx).hover;
        let foreground = cx.theme().muted_foreground;
        let headers = self.url.is_none();
        let reorder_label = if headers {
            "Reorder header with Up or Down"
        } else {
            "Reorder parameter with Up or Down"
        };
        let remove_label = if headers {
            "Remove header"
        } else {
            "Remove parameter"
        };
        let key = row.key.read(cx).value();
        let draft = if headers {
            key.is_empty()
        } else {
            row.original.is_none()
        };
        let row_width = self.row_width.clone();
        let dragged_row = self.dragged_row.clone();
        let drag = ParamDrag {
            list,
            id,
            key,
            value: row.has_value.then(|| row.value.read(cx).value()),
            headers,
            width: Pixels::ZERO,
        };
        div()
            .id(("query-param", id))
            .role(Role::ListItem)
            .group("query-param-row")
            .flex()
            .flex_none()
            .items_center()
            .gap_2()
            .rounded(cx.theme().radius)
            .when(
                cx.has_active_drag() && self.dragged_row.get() == Some(id),
                |row| row.opacity(0.0),
            )
            .can_drop(move |drag, _, _| {
                drag.downcast_ref::<ParamDrag>()
                    .is_some_and(|drag| drag.list == list && drag.id != id)
            })
            .drag_over(move |style, drag: &ParamDrag, _, _| {
                if drag.list == list && drag.id != id {
                    style.bg(hover)
                } else {
                    style
                }
            })
            .on_drop(cx.listener(move |this, drag: &ParamDrag, window, cx| {
                let source = this.rows.iter().position(|row| row.id == drag.id);
                let target = this.rows.iter().position(|row| row.id == id);
                if let (Some(source), Some(target)) = (source, target) {
                    this.move_row(source, target, window, cx);
                }
            }))
            .child(
                div()
                    .id("drag-handle")
                    .aria_label(reorder_label)
                    .tab_index(0)
                    .flex()
                    .size_6()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .cursor(CursorStyle::OpenHand)
                    .rounded(cx.theme().radius)
                    .opacity(0.0)
                    .group_hover("query-param-row", |style| style.opacity(1.0))
                    .focus_visible(move |style| style.opacity(1.0).bg(hover))
                    .child(param_icon(DRAG_ICON, foreground))
                    .on_drag(drag, move |drag, _, _, cx| {
                        dragged_row.set(Some(id));
                        cx.new(|_| ParamDrag {
                            width: row_width.get(),
                            ..drag.clone()
                        })
                    })
                    .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                        let Some(index) = this.rows.iter().position(|row| row.id == id) else {
                            return;
                        };
                        let target = match event.keystroke.key.as_str() {
                            "up" => index.saturating_sub(1),
                            "down" => (index + 1).min(this.rows.len() - 1),
                            _ => return,
                        };
                        cx.stop_propagation();
                        this.move_row(index, target, window, cx);
                    })),
            )
            .map(|view| {
                if headers {
                    view.child(self.header_key(row, draft, cx)).child(
                        param_field(draft, cx).child(field_input(&row.value, "Header value")),
                    )
                } else {
                    view.child(param_field(draft, cx).child(field_input(&row.key, "Parameter key")))
                        .child(self.value_field(row, cx))
                }
            })
            .child(
                action_button("remove-param", cx)
                    .with_size(Size::Small)
                    .tooltip(remove_label)
                    .accessibility_label(remove_label)
                    .icon(param_icon(DELETE_ICON, foreground))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.rows.retain(|row| row.id != id);
                        this.ensure_empty_row(window, cx);
                        this.sync_to_url(window, cx);
                    })),
            )
    }

    fn header_key(
        &self,
        row: &ParamRow,
        draft: bool,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let id = row.id;
        let owner = cx.weak_entity();
        let name = row.key.read(cx).value();
        let muted = cx.theme().muted_foreground;
        let foreground = cx.theme().foreground;
        let placeholder = param_placeholder_color(cx);
        let chevron = field_button_icon(CHEVRON_ICON, rems(0.75), muted, true);
        let button = field_button("header-key-options", cx)
            .with_size(Size::Small)
            .cursor_pointer()
            .accessibility_label("Header name options")
            .map(|button| {
                if row.editing_header {
                    button.child(chevron).min_w_6().flex_none()
                } else {
                    let empty = name.is_empty();
                    button.w_full().px_2().child(
                        div()
                            .w_full()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_1()
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_color(if empty { placeholder } else { foreground })
                                    .child(if empty { "Header Name".into() } else { name }),
                            )
                            .child(chevron),
                    )
                }
            });
        let picker = Popover::new(("header-name-picker", id))
            .trigger(button)
            .p_2()
            .content(move |_, window, cx| {
                let popover = cx.weak_entity();
                let owner = owner.clone();
                window.use_keyed_state(("header-search-state", id), cx, move |window, cx| {
                    HeaderPicker::new(
                        popover,
                        move |name, window, cx| {
                            let _ = owner.update(cx, |this, cx| {
                                if let Some(row) = this.rows.iter_mut().find(|row| row.id == id) {
                                    row.editing_header = name.is_none();
                                    if let Some(name) = name {
                                        update_field(&row.key, name.to_owned(), window, cx);
                                    } else {
                                        let focus = row.key.read(cx).focus_handle(cx);
                                        window.on_next_frame(move |window, cx| {
                                            focus.focus(window, cx)
                                        });
                                    }
                                    cx.notify();
                                }
                            });
                        },
                        window,
                        cx,
                    )
                })
            });
        param_field(draft, cx)
            .when(!row.editing_header, |field| {
                field.flex_col().items_stretch().justify_center()
            })
            .when(row.editing_header, |field| {
                field.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(field_input(&row.key, "Header name")),
                )
            })
            .child(picker)
    }

    fn value_field(&self, row: &ParamRow, cx: &Context<Self>) -> impl IntoElement + use<> {
        let id = row.id;
        let has_value = row.has_value;
        let owner = cx.weak_entity();
        let muted = cx.theme().muted_foreground;
        let placeholder = param_placeholder_color(cx);
        let chevron = field_button_icon(CHEVRON_ICON, rems(0.75), muted, true);
        let button = field_button("param-value-type", cx)
            .with_size(Size::Small)
            .cursor_pointer()
            .accessibility_label(if has_value {
                "Parameter value options"
            } else {
                "No Value"
            })
            .map(|button| {
                if has_value {
                    button.child(chevron).min_w_6().flex_none()
                } else {
                    button.w_full().px_2().child(
                        div()
                            .flex()
                            .w_full()
                            .items_center()
                            .justify_between()
                            .text_color(placeholder)
                            .child("No Value")
                            .child(chevron),
                    )
                }
            })
            .dropdown_menu(move |menu, _, _| {
                [false, true]
                    .into_iter()
                    .fold(menu.with_size(Size::Small), |menu, edit| {
                        let owner = owner.clone();
                        let checked = has_value == edit;
                        let item = PopupMenuItem::new(if edit { "Edit" } else { "No Value" })
                            .checked(checked);
                        let item = if checked {
                            item.icon(param_icon(CHECK_ICON, muted))
                        } else {
                            item
                        };
                        menu.item(item.on_click(move |_, window, cx| {
                            let _ = owner.update(cx, |this, cx| {
                                if let Some(row) = this.rows.iter_mut().find(|row| row.id == id) {
                                    row.has_value = edit;
                                    if edit {
                                        let focus = row.value.read(cx).focus_handle(cx);
                                        window.on_next_frame(move |window, cx| {
                                            focus.focus(window, cx)
                                        });
                                    }
                                    this.sync_to_url(window, cx);
                                }
                            });
                        }))
                    })
            });
        param_field(row.original.is_none(), cx)
            .when(!has_value, |field| {
                field.flex_col().items_stretch().justify_center()
            })
            .when(has_value, |field| {
                field.child(
                    div().flex_1().min_w_0().child(
                        Input::new(&row.value)
                            .with_size(Size::Small)
                            .appearance(false)
                            .bordered(false)
                            .aria_label("Parameter value")
                            .px_2()
                            .w_full(),
                    ),
                )
            })
            .child(button)
    }
}

impl Render for RequestFields {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let row_width = self.row_width.clone();
        let add_label = if self.url.is_none() {
            "Add header"
        } else {
            "Add parameter"
        };
        let list = div()
            .id("query-parameter-scroll")
            .size_full()
            .min_h_0()
            .track_scroll(&self.scroll)
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_1()
            .pt_4()
            .pr_4()
            .child(
                div()
                    .on_children_prepainted(move |rows, _, _| {
                        if let Some(row) = rows.first() {
                            row_width.set(row.size.width);
                        }
                    })
                    .id("query-parameter-rows")
                    .role(Role::List)
                    .flex()
                    .flex_col()
                    .flex_none()
                    .gap_1()
                    .children(self.rows.iter().map(|row| self.row(row, cx))),
            )
            .child(
                div().flex().flex_none().pl_8().child(
                    action_button("add-query-param", cx)
                        .with_size(Size::Small)
                        .tooltip(add_label)
                        .accessibility_label(add_label)
                        .icon(param_icon(ADD_ICON, cx.theme().muted_foreground))
                        .on_click(cx.listener(|this, _, window, cx| this.add(window, cx))),
                ),
            );
        div()
            .id("query-parameters")
            .size_full()
            .min_h_0()
            .relative()
            .child(list)
            .vertical_scrollbar(&self.scroll)
    }
}

#[derive(Clone)]
struct ParamDrag {
    list: EntityId,
    id: usize,
    key: SharedString,
    value: Option<SharedString>,
    headers: bool,
    width: Pixels,
}

impl Render for ParamDrag {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let placeholder = param_placeholder_color(cx);
        let key_empty = self.key.is_empty();
        let draft = key_empty && (self.headers || self.value.is_none());
        div()
            .w(self.width)
            .flex()
            .items_center()
            .gap_2()
            .rounded(cx.theme().radius)
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .font_family(cx.theme().font_family.clone())
            .text_sm()
            .child(
                div()
                    .size_6()
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(param_icon(DRAG_ICON, muted)),
            )
            .child(
                param_field(draft, cx)
                    .px_2()
                    .justify_between()
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .when(key_empty, |text| text.text_color(placeholder))
                            .child(if key_empty {
                                if self.headers {
                                    "Header Name".into()
                                } else {
                                    "Key".into()
                                }
                            } else {
                                self.key.clone()
                            }),
                    )
                    .when(self.headers, |field| {
                        field.child(param_icon(CHEVRON_ICON, muted).with_size(Size::XSmall))
                    }),
            )
            .child(
                param_field(draft, cx)
                    .px_2()
                    .justify_between()
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .when(
                                self.value.as_ref().is_none_or(|value| value.is_empty()),
                                |text| text.text_color(placeholder),
                            )
                            .child(match &self.value {
                                None => "No Value".into(),
                                Some(value) if value.is_empty() => "Value".into(),
                                Some(value) => value.clone(),
                            }),
                    )
                    .when(!self.headers, |field| {
                        field.child(param_icon(CHEVRON_ICON, muted).with_size(Size::XSmall))
                    }),
            )
            .child(
                div()
                    .size_6()
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(param_icon(DELETE_ICON, muted)),
            )
    }
}

pub(super) fn param_field(draft: bool, cx: &App) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .h_7()
        .flex()
        .items_center()
        .border_1()
        .border_color(cx.theme().border.opacity(0.5))
        .rounded(cx.theme().radius)
        .when(draft, |field| field.border_dashed())
}

fn field_input(state: &Entity<InputState>, label: &'static str) -> Input {
    Input::new(state)
        .with_size(Size::Small)
        .appearance(false)
        .bordered(false)
        .aria_label(label)
        .px_2()
        .w_full()
}

fn param_icon(data: &'static [u8], color: Hsla) -> Icon {
    Icon::default()
        .data(data)
        .with_size(Size::Small)
        .text_color(color)
}

pub(super) fn param_placeholder_color(cx: &App) -> Hsla {
    cx.theme()
        .highlight_theme
        .style
        .editor_line_number
        .unwrap_or(cx.theme().muted_foreground)
}

fn update_field(field: &Entity<InputState>, text: String, window: &mut Window, cx: &mut App) {
    if field.read(cx).value().as_str() != text {
        field.update(cx, |state, cx| state.set_value(text, window, cx));
    }
}
