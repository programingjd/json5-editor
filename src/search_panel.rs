use gpui_kit::base::input::RopeExt as _;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Selectable as _, Sizable as _, Size,
    button::{Button, ButtonRounded, ButtonVariants as _},
    input::{
        EditorState, Enter, InlineToken, InlineTokenContext, InputContent, InputEvent, InputToken,
        Textarea, TextareaState,
    },
    label::Label,
    menu::{DropdownMenu as _, PopupMenuItem},
};
use gpui_kit::{
    App, AppContext as _, Context, Empty, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, Subscription, Window, div,
    prelude::FluentBuilder as _, relative,
};
use std::ops::Range;

pub(super) struct EditorSearchPanel {
    editor: Entity<EditorState>,
    input: Entity<TextareaState>,
    replacement: Entity<TextareaState>,
    visible: bool,
    replace_expanded: bool,
    case_sensitive: bool,
    regex_mode: bool,
    preserve_case: bool,
    regex_error: Option<SharedString>,
    search_history: Vec<SharedString>,
    replacement_history: Vec<SharedString>,
    pending_focus: bool,
    current_match: usize,
    total_matches: usize,
    match_label: SharedString,
    _input_subscription: Subscription,
    _replacement_subscription: Subscription,
    _editor_observer: Subscription,
}

impl EditorSearchPanel {
    pub(super) fn new(
        editor: &Entity<EditorState>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<Self> {
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, 6)
                .placeholder("Search")
        });
        let replacement = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, 6)
                .placeholder("Replace")
        });
        cx.new(|cx: &mut Context<Self>| Self {
            editor: editor.clone(),
            input: input.clone(),
            replacement: replacement.clone(),
            visible: false,
            replace_expanded: false,
            case_sensitive: false,
            regex_mode: false,
            preserve_case: false,
            regex_error: None,
            search_history: Vec::new(),
            replacement_history: Vec::new(),
            pending_focus: false,
            current_match: 0,
            total_matches: 0,
            match_label: "0 results".into(),
            _input_subscription: cx.subscribe(&input, |this, _, event, cx| {
                if let InputEvent::Change = event {
                    this.update_query(cx);
                } else if let InputEvent::Blur = event {
                    this.remember_history(false, cx);
                }
            }),
            _replacement_subscription: cx.subscribe(&replacement, |this, _, event, cx| {
                if let InputEvent::Blur = event {
                    this.remember_history(true, cx);
                } else if let InputEvent::Change = event {
                    cx.notify();
                }
            }),
            _editor_observer: cx.observe(editor, |this, editor, cx| {
                let session = editor.read(cx).search_session();
                // The matcher already retains every match; reading its index
                // and length adds no document scan, even for a large file.
                let (current, total) = if session.is_active() {
                    (
                        session.matcher.current().map_or(0, |index| index + 1),
                        session.matcher.len(),
                    )
                } else {
                    (0, 0)
                };
                if (this.current_match, this.total_matches) != (current, total) {
                    this.current_match = current;
                    this.total_matches = total;
                    this.match_label = if total == 0 {
                        "0 results".into()
                    } else {
                        format!("{current}/{total}").into()
                    };
                    cx.notify();
                }
            }),
        })
    }

    pub(super) fn open(&mut self, replace: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.replace_expanded = replace;
        if !self.visible {
            self.visible = true;
            self.pending_focus = true;
            self.update_query(cx);
        } else {
            self.focus_query(window, cx);
        }
        cx.notify();
    }

    fn focus_query(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| {
            input.select_all(window, cx);
            input.focus(window, cx);
        });
    }

    fn initialize_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !std::mem::take(&mut self.pending_focus) {
            return;
        }
        cx.on_next_frame(window, |this, window, cx| {
            if this.visible {
                this.focus_query(window, cx);
            }
        });
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.remember_history(false, cx);
        self.remember_history(true, cx);
        self.visible = false;
        self.replace_expanded = false;
        self.pending_focus = false;
        self.editor.update(cx, |editor, cx| {
            editor.close_search(cx);
            editor.focus(window, cx);
        });
        cx.notify();
    }

    pub(super) fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.visible {
            return false;
        }
        self.close(window, cx);
        true
    }

    fn toggle_replace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.replace_expanded = !self.replace_expanded;
        self.input.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    fn update_query(&mut self, cx: &mut Context<Self>) {
        let query = field_value(self.input.read(cx));
        self.regex_error = self.editor.update(cx, |editor, cx| {
            if query.is_empty() {
                editor.close_search(cx);
                return None;
            }
            if self.regex_mode {
                if let Err(error) =
                    editor.set_search_regex(query.as_ref(), !self.case_sensitive, cx)
                {
                    return Some(error.to_string().into());
                }
            } else {
                editor.set_search_query(query.as_ref(), !self.case_sensitive, cx);
            }
            Self::reveal_current(editor, cx);
            None
        });
        cx.notify();
    }

    fn navigate(&mut self, previous: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate_match(previous, cx);
        self.input.update(cx, |input, cx| input.focus(window, cx));
    }

    pub(super) fn navigate_match(&mut self, previous: bool, cx: &mut Context<Self>) {
        if !self.visible {
            return;
        }
        self.remember_history(false, cx);
        self.editor.update(cx, |editor, cx| {
            let range = if previous {
                editor.previous_search_match(cx)
            } else {
                editor.next_search_match(cx)
            };
            if let Some(range) = range {
                Self::reveal_match(editor, range, cx);
            }
        });
    }

    fn insert_newline(&mut self, replacement: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.regex_mode {
            return;
        }
        let input = if replacement {
            &self.replacement
        } else {
            &self.input
        };
        input.update(cx, |input, cx| {
            if let Err(error) = input.replace_with_token(newline_token(), window, cx) {
                log::warn!("Could not insert newline chip: {error}");
                input.replace("\n", window, cx);
            }
            input.focus(window, cx);
        });
    }

    fn replace(&mut self, all: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.remember_history(false, cx);
        self.remember_history(true, cx);
        let replacement = field_value(self.replacement.read(cx));
        self.editor.update(cx, |editor, cx| {
            if all {
                editor.replace_all_search_matches(replacement.as_ref(), window, cx);
            } else {
                editor.replace_current_search_match(replacement.as_ref(), window, cx);
            }
            Self::reveal_current(editor, cx);
        });
        self.replacement
            .update(cx, |input, cx| input.focus(window, cx));
    }

    fn reveal_current(editor: &mut EditorState, cx: &mut Context<EditorState>) {
        let matcher = &editor.search_session().matcher;
        let current = matcher
            .current()
            .and_then(|index| matcher.matched_ranges().get(index).cloned());
        if let Some(range) = current {
            Self::reveal_match(editor, range, cx);
        }
    }

    fn reveal_match(editor: &mut EditorState, range: Range<usize>, cx: &mut Context<EditorState>) {
        editor.unfold_at(editor.text().offset_to_position(range.start), cx);
        editor.unfold_at(editor.text().offset_to_position(range.end), cx);
        editor.set_selected_range(range, cx);
        editor.center_cursor(cx);
    }

    fn remember_history(&mut self, replacement: bool, cx: &App) {
        let (history, value) = if replacement {
            (
                &mut self.replacement_history,
                field_value(self.replacement.read(cx)),
            )
        } else {
            (&mut self.search_history, field_value(self.input.read(cx)))
        };
        remember(history, value);
    }

    fn render_history(&self, replacement: bool, cx: &Context<Self>) -> impl IntoElement {
        let owner = cx.weak_entity();
        panel_button(
            if replacement {
                "replacement-history"
            } else {
                "search-history"
            },
            cx,
        )
        .icon(
            Icon::default()
                .data(include_bytes!("../assets/icons/search-history.svg"))
                .size_4(),
        )
        .w_6()
        .accessibility_label(if replacement {
            "Replacement history"
        } else {
            "Search history"
        })
        .tooltip(if replacement {
            "Replacement history"
        } else {
            "Search history"
        })
        .dropdown_menu(move |menu, window, cx| {
            let menu = menu
                .with_size(Size::Small)
                .scrollable(true)
                .min_w(window.rem_size() * 8.)
                .max_w(window.rem_size() * 18.)
                .item_colors(search_text_color(cx), cx.theme().selection);
            let history = owner
                .read_with(cx, |this, cx| {
                    let (mut history, value) = if replacement {
                        (
                            this.replacement_history.clone(),
                            field_value(this.replacement.read(cx)),
                        )
                    } else {
                        (
                            this.search_history.clone(),
                            field_value(this.input.read(cx)),
                        )
                    };
                    remember(&mut history, value);
                    history
                })
                .unwrap_or_default();
            if history.is_empty() {
                return menu.item(
                    PopupMenuItem::element(|_, cx| {
                        div()
                            .text_xs()
                            .italic()
                            .text_color(search_text_color(cx))
                            .child("No history yet")
                    })
                    .disabled(true),
                );
            }
            history.into_iter().fold(menu, |menu, value| {
                let owner = owner.clone();
                let label: SharedString = value.replace('\r', "\\r").replace('\n', "\\n").into();
                menu.item(
                    PopupMenuItem::element(move |_, _| div().text_xs().child(label.clone()))
                        .on_click(move |_, window, cx| {
                            let _ = owner.update(cx, |this, cx| {
                                let input = if replacement {
                                    this.replacement.clone()
                                } else {
                                    this.input.clone()
                                };
                                input.update(cx, |input, cx| {
                                    input.set_value(newline_content(&value), window, cx)
                                });
                                this.remember_history(replacement, cx);
                                if !replacement {
                                    this.update_query(cx);
                                }
                                // Menus restore their trigger's focus on dismissal.
                                // Return focus to the chosen field after dismissal.
                                cx.on_next_frame(window, move |_, window, cx| {
                                    input.update(cx, |input, cx| {
                                        input.select_all(window, cx);
                                        input.focus(window, cx);
                                    });
                                });
                            });
                        }),
                )
            })
        })
    }

    fn render_search_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let no_results = self.total_matches == 0 && !self.input.read(cx).value().is_empty();
        div()
            .flex()
            .items_center()
            .w_full()
            .min_w_0()
            .min_h_8()
            .gap_1()
            .child(
                div()
                    .w_96()
                    .min_w_0()
                    .border_r_1()
                    .border_color(super::editor_separator_color(cx))
                    .key_context("SearchField")
                    .capture_action(cx.listener(|this, action: &Enter, window, cx| {
                        cx.stop_propagation();
                        if action.secondary && action.shift {
                            this.insert_newline(false, window, cx);
                        } else {
                            this.navigate(action.shift, window, cx);
                        }
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .w_full()
                            .px_1p5()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .h_8()
                                    .child(self.render_history(false, cx)),
                            )
                            .child(
                                Textarea::new(&self.input)
                                    .with_size(Size::Small)
                                    .appearance(false)
                                    .font_family(cx.theme().font_family.clone())
                                    .text_xs()
                                    .line_height(relative(1.5))
                                    .pl_1()
                                    .pr_2()
                                    .token(move |token, _, cx| newline_chip(token, no_results, cx))
                                    .flex_1()
                                    .min_w_0()
                                    .text_color(if no_results {
                                        cx.theme().danger
                                    } else {
                                        search_text_color(cx)
                                    })
                                    .aria_label("Search document"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .h_8()
                                    .gap_1()
                                    .child(
                                        panel_button("clear-search", cx)
                                            .disabled(self.input.read(cx).value().is_empty())
                                            .icon(Icon::default().data(include_bytes!(
                                                "../assets/icons/close.svg"
                                            )))
                                            .accessibility_label("Clear search")
                                            .tooltip("Clear search")
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.input.update(cx, |input, cx| {
                                                    input.set_value("", window, cx);
                                                    input.focus(window, cx);
                                                });
                                                this.update_query(cx);
                                            })),
                                    )
                                    .child(
                                        panel_button("newline-search", cx)
                                            .disabled(self.regex_mode)
                                            .icon(Icon::default().data(include_bytes!(
                                                "../assets/icons/new-line.svg"
                                            )))
                                            .accessibility_label("Insert newline in search")
                                            .tooltip_with(|window, cx| {
                                                crate::shortcuts::tooltip(
                                                    "Insert newline",
                                                    "Ctrl/Cmd+Shift+Enter",
                                                    window,
                                                    cx,
                                                )
                                            })
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.insert_newline(false, window, cx)
                                            })),
                                    )
                                    .child(
                                        panel_button("match-case", cx)
                                            .icon(Icon::default().data(include_bytes!(
                                                "../assets/icons/match-case.svg"
                                            )))
                                            .toggled(self.case_sensitive)
                                            .selected(self.case_sensitive)
                                            .accessibility_label("Match case")
                                            .tooltip("Match case")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.case_sensitive = !this.case_sensitive;
                                                this.update_query(cx);
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        panel_button("regex-search", cx)
                                            .icon(Icon::default().data(include_bytes!(
                                                "../assets/icons/regex.svg"
                                            )))
                                            .toggled(self.regex_mode)
                                            .selected(self.regex_mode)
                                            .accessibility_label("Regular expression")
                                            .tooltip(
                                                self.regex_error
                                                    .clone()
                                                    .unwrap_or_else(|| "Regular expression".into()),
                                            )
                                            .when(self.regex_error.is_some(), |this| {
                                                this.text_color(cx.theme().danger)
                                            })
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.regex_mode = !this.regex_mode;
                                                this.update_query(cx);
                                            })),
                                    ),
                            ),
                    ),
            )
            .child(
                Label::new(self.match_label.clone())
                    .flex_none()
                    .min_w_16()
                    .px_2()
                    .text_xs()
                    .text_color(
                        (if no_results {
                            cx.theme().danger
                        } else {
                            search_text_color(cx)
                        })
                        .opacity(0.25),
                    ),
            )
            .child(
                panel_button("previous-search-match", cx)
                    .icon(
                        Icon::default().data(include_bytes!("../assets/icons/chevron-up.svg")),
                    )
                    .disabled(self.total_matches == 0)
                    .accessibility_label("Previous match")
                    .tooltip_with(|window, cx| {
                        crate::shortcuts::tooltip("Previous match", "Shift+F3", window, cx)
                    })
                    .on_click(cx.listener(|this, _, window, cx| this.navigate(true, window, cx))),
            )
            .child(
                panel_button("next-search-match", cx)
                    .icon(
                        Icon::default()
                            .data(include_bytes!("../assets/icons/chevron-down.svg")),
                    )
                    .disabled(self.total_matches == 0)
                    .accessibility_label("Next match")
                    .tooltip_with(|window, cx| {
                        crate::shortcuts::tooltip("Next match", "F3", window, cx)
                    })
                    .on_click(cx.listener(|this, _, window, cx| this.navigate(false, window, cx))),
            )
            .child(div().flex_1())
            .child(
                panel_button("close-search", cx)
                    .icon(Icon::default().data(include_bytes!("../assets/icons/close.svg")))
                    .accessibility_label("Close search")
                    .tooltip_with(|window, cx| {
                        crate::shortcuts::tooltip("Close search", "Escape", window, cx)
                    })
                    .on_click(cx.listener(|this, _, window, cx| this.close(window, cx))),
            )
    }

    fn render_replace_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let disabled = self.total_matches == 0 || !self.editor.read(cx).is_replaceable();
        div()
            .flex()
            .items_center()
            .w_full()
            .min_w_0()
            .min_h_8()
            .gap_2()
            .border_t_1()
            .border_color(super::editor_separator_color(cx))
            .child(
                div()
                    .w_96()
                    .min_w_0()
                    .border_r_1()
                    .border_color(super::editor_separator_color(cx))
                    .key_context("SearchField")
                    .capture_action(cx.listener(|this, action: &Enter, window, cx| {
                        cx.stop_propagation();
                        if action.secondary && action.shift {
                            this.insert_newline(true, window, cx);
                        } else {
                            this.replace(false, window, cx);
                        }
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .w_full()
                            .px_1p5()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .h_8()
                                    .child(self.render_history(true, cx)),
                            )
                            .child(
                                Textarea::new(&self.replacement)
                                    .with_size(Size::Small)
                                    .appearance(false)
                                    .font_family(cx.theme().font_family.clone())
                                    .text_xs()
                                    .line_height(relative(1.5))
                                    .pl_1()
                                    .pr_2()
                                    .text_color(search_text_color(cx))
                                    .token(|token, _, cx| newline_chip(token, false, cx))
                                    .flex_1()
                                    .min_w_0()
                                    .aria_label("Replace with"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .h_8()
                                    .gap_1()
                                    .child(
                                        panel_button("clear-replacement", cx)
                                            .disabled(self.replacement.read(cx).value().is_empty())
                                            .icon(Icon::default().data(include_bytes!(
                                                "../assets/icons/close.svg"
                                            )))
                                            .accessibility_label("Clear replacement")
                                            .tooltip("Clear replacement")
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.remember_history(true, cx);
                                                this.replacement.update(cx, |input, cx| {
                                                    input.set_value("", window, cx);
                                                    input.focus(window, cx);
                                                });
                                            })),
                                    )
                                    .child(
                                        panel_button("newline-replacement", cx)
                                            .disabled(self.regex_mode)
                                            .icon(Icon::default().data(include_bytes!(
                                                "../assets/icons/new-line.svg"
                                            )))
                                            .accessibility_label("Insert newline in replacement")
                                            .tooltip_with(|window, cx| {
                                                crate::shortcuts::tooltip(
                                                    "Insert newline",
                                                    "Ctrl/Cmd+Shift+Enter",
                                                    window,
                                                    cx,
                                                )
                                            })
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.insert_newline(true, window, cx)
                                            })),
                                    )
                                    .child(
                                        panel_button("preserve-replacement-case", cx)
                                            .icon(Icon::default().data(include_bytes!(
                                                "../assets/icons/preserve-case.svg"
                                            )))
                                            .toggled(self.preserve_case)
                                            .selected(self.preserve_case)
                                            .accessibility_label("Preserve case")
                                            .tooltip("Preserve case")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.preserve_case = !this.preserve_case;
                                                this.editor.update(cx, |editor, cx| {
                                                    editor.set_search_preserve_case(
                                                        this.preserve_case,
                                                        cx,
                                                    );
                                                });
                                                cx.notify();
                                            })),
                                    ),
                            ),
                    ),
            )
            .child(
                super::bordered_action_button("replace-match", "Replace", cx)
                    .disabled(disabled)
                    .on_click(cx.listener(|this, _, window, cx| this.replace(false, window, cx))),
            )
            .child(
                super::bordered_action_button("replace-all-matches", "Replace All", cx)
                    .disabled(disabled)
                    .on_click(cx.listener(|this, _, window, cx| this.replace(true, window, cx))),
            )
    }
}

impl Render for EditorSearchPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.visible {
            return Empty.into_any_element();
        }
        let owner = cx.weak_entity();
        div()
            .flex()
            .flex_none()
            .w_full()
            .min_w_0()
            .bg(cx.theme().muted)
            .border_b_1()
            .border_color(super::editor_separator_color(cx))
            .font_family(cx.theme().font_family.clone())
            .on_children_prepainted(move |_, window, cx| {
                let _ = owner.update(cx, |this, cx| this.initialize_focus(window, cx));
            })
            .child(
                div()
                    .w_7()
                    .flex_none()
                    .flex()
                    .justify_center()
                    .pt_1()
                    .border_r_1()
                    .border_color(super::editor_separator_color(cx))
                    .child(
                        panel_button("toggle-replace", cx)
                            .icon(Icon::default().data(if self.replace_expanded {
                                include_bytes!("../assets/icons/chevron-down.svg") as &[u8]
                            } else {
                                include_bytes!("../assets/icons/chevron-right.svg") as &[u8]
                            }))
                            .toggled(self.replace_expanded)
                            .accessibility_label("Toggle replace row")
                            .tooltip(if self.replace_expanded {
                                "Hide replace"
                            } else {
                                "Show replace"
                            })
                            .on_click(
                                cx.listener(|this, _, window, cx| this.toggle_replace(window, cx)),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .child(self.render_search_row(cx))
                    .when(self.replace_expanded, |this| {
                        this.child(self.render_replace_row(cx))
                    }),
            )
            .into_any_element()
    }
}

fn panel_button(id: &'static str, cx: &App) -> Button {
    Button::new(id)
        .cursor_pointer()
        .with_size(Size::XSmall)
        .custom(super::toolbar_button_style(cx))
        .rounded(ButtonRounded::Small)
}

fn remember(history: &mut Vec<SharedString>, value: SharedString) {
    if value.is_empty() {
        return;
    }
    history.retain(|previous| previous != &value);
    history.insert(0, value);
    history.truncate(30);
}

const NEWLINE_TOKEN_ID: &str = "search-newline";

fn newline_token() -> InlineToken {
    InlineToken::new(NEWLINE_TOKEN_ID, "\\n").with_label("\\n")
}

fn field_value(input: &TextareaState) -> SharedString {
    let content = input.content();
    if content.tokens().is_empty() {
        return content.text().clone();
    }
    let mut value = String::with_capacity(content.text().len());
    let mut start = 0;
    for span in content
        .tokens()
        .iter()
        .filter(|span| span.token().id().as_ref() == NEWLINE_TOKEN_ID)
    {
        let range = span.range();
        value.push_str(&content.text()[start..range.start]);
        value.push('\n');
        start = range.end;
    }
    value.push_str(&content.text()[start..]);
    value.into()
}

fn newline_content(value: &str) -> InputContent {
    let mut display = String::with_capacity(value.len());
    let mut ranges = Vec::new();
    for part in value.split_inclusive('\n') {
        if let Some(part) = part.strip_suffix('\n') {
            display.push_str(part);
            let start = display.len();
            display.push_str("\\n");
            ranges.push(start..display.len());
        } else {
            display.push_str(part);
        }
    }
    let mut content = InputContent::new(display);
    for range in ranges {
        match content.with_token(range, newline_token()) {
            Ok(next) => content = next,
            // Preserve unusual grapheme sequences instead of rejecting the
            // history entry if a token boundary would split a grapheme.
            Err(_) => return InputContent::new(value),
        }
    }
    content
}

fn search_text_color(cx: &App) -> gpui_kit::Hsla {
    cx.theme().popover_foreground
}

fn newline_chip(token: &InlineTokenContext, error: bool, cx: &App) -> InputToken {
    InputToken::new(token)
        .text_xs()
        .rounded(cx.theme().radius * 0.5)
        .border_color(if token.is_selected() {
            cx.theme().selection
        } else {
            cx.theme().border.opacity(0.5)
        })
        .text_color(if error {
            cx.theme().danger
        } else {
            search_text_color(cx)
        })
}
