use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, FocusTrapElement as _, Sizable as _, Size,
    ThemeStyled as _,
    checkbox::Checkbox,
    input::{EditorState, Enter, Input, InputEvent, InputState},
};
use gpui_kit::{
    App, AppContext as _, Context, Empty, Entity, FocusHandle, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, Window, div, prelude::FluentBuilder as _, rems,
};

const CONVERT_TO_JSON: &str = "convert-to-json";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Formatting {
    Keep,
    Minimize,
    Indent,
}

impl Formatting {
    const CHOICES: [(Self, &'static str); 3] = [
        (Self::Keep, "Keep as is"),
        (Self::Minimize, "Minimize"),
        (Self::Indent, "Indent"),
    ];

    fn value(self) -> &'static str {
        match self {
            Self::Keep => "keep",
            Self::Minimize => "minimize",
            Self::Indent => "indent",
        }
    }

    fn load() -> Self {
        match super::load_save_formatting().as_str() {
            "keep" => Self::Keep,
            "indent" => Self::Indent,
            _ => Self::Minimize,
        }
    }
}

pub(super) struct SaveOverlay {
    editor: Entity<EditorState>,
    filename: Entity<SharedString>,
    name: Entity<InputState>,
    focus_handle: FocusHandle,
    formatting_focus: [FocusHandle; 3],
    visible: bool,
    initial_name: Option<String>,
    formatting: Formatting,
    convert_to_json: bool,
    saving: bool,
    error: Option<SharedString>,
    _name_subscription: Subscription,
}

impl SaveOverlay {
    pub(super) fn new(
        editor: &Entity<EditorState>,
        filename: Entity<SharedString>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<Self> {
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("filename"));
        cx.new(|cx: &mut Context<Self>| Self {
            editor: editor.clone(),
            filename,
            name: name.clone(),
            focus_handle: cx.focus_handle(),
            formatting_focus: std::array::from_fn(|_| cx.focus_handle()),
            visible: false,
            initial_name: None,
            formatting: Formatting::load(),
            convert_to_json: super::load_save_option(CONVERT_TO_JSON),
            saving: false,
            error: None,
            _name_subscription: cx.subscribe(&name, |this, _, event, cx| {
                if matches!(event, InputEvent::Change) && this.error.take().is_some() {
                    cx.notify();
                }
            }),
        })
    }

    pub(super) fn is_open(&self) -> bool {
        self.visible
    }

    fn select_formatting(
        &mut self,
        formatting: Formatting,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.formatting = formatting;
        super::store_save_formatting(formatting.value());
        self.formatting_focus[formatting as usize].focus(window, cx);
        cx.notify();
    }

    pub(super) fn block_background_key(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.visible || self.focus_handle.contains_focused(window, cx) {
            return false;
        }
        if self.saving {
            self.focus_handle.focus(window, cx);
        } else {
            self.name.update(cx, |name, cx| name.focus(window, cx));
        }
        true
    }

    pub(super) fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.visible {
            self.name.update(cx, |name, cx| name.focus(window, cx));
            return;
        }
        self.visible = true;
        self.focus_handle.focus(window, cx);
        self.initial_name = Some(self.filename.read(cx).to_string());
        self.error = None;
        cx.notify();
    }

    fn initialize(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(filename) = self.initial_name.take() else {
            return;
        };
        // Input must install Inter before set_value measures the filename.
        cx.on_next_frame(window, move |this, window, cx| {
            if !this.visible {
                return;
            }
            this.name.update(cx, |name, cx| {
                name.set_value(filename, window, cx);
                name.select_all(window, cx);
                name.focus(window, cx);
            });
        });
    }

    pub(super) fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.visible {
            return false;
        }
        if !self.saving {
            self.visible = false;
            self.initial_name = None;
            self.editor
                .update(cx, |editor, cx| editor.focus(window, cx));
            cx.notify();
        }
        true
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving || self.initial_name.is_some() {
            return;
        }
        let mut filename = super::safe_file_name(self.name.read(cx).value().as_ref());
        if filename.is_empty() {
            self.error = Some("Enter a file name".into());
            self.name.update(cx, |name, cx| name.focus(window, cx));
            cx.notify();
            return;
        }
        if self.convert_to_json && filename.to_ascii_lowercase().ends_with(".json5") {
            filename.truncate(filename.len() - 1);
        }
        self.saving = true;
        self.error = None;
        super::request_save_file(&filename, self.formatting.value(), self.convert_to_json);
        cx.notify();
    }

    pub(super) fn finish(
        &mut self,
        name: String,
        error: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.saving = false;
        if error.is_empty() {
            self.filename.update(cx, |filename, cx| {
                *filename = name.into();
                cx.notify();
            });
            self.dismiss(window, cx);
        } else {
            self.error = Some(error.into());
            cx.notify();
        }
    }
}

impl Render for SaveOverlay {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.visible {
            return Empty.into_any_element();
        }
        let owner = cx.weak_entity();
        let foreground = cx.theme().popover_foreground;
        crate::overlay_frame::root("save-overlay", cx)
            .child(
                crate::overlay_frame::page("save-page").child(
                    crate::overlay_frame::panel(cx)
                        .w(rems(27.))
                        .gap_3()
                        .track_focus(&self.focus_handle)
                        .on_children_prepainted(move |_, window, cx| {
                            let _ = owner.update(cx, |this, cx| this.initialize(window, cx));
                        })
                        .child(
                            div()
                                .capture_action(cx.listener(|this, _: &Enter, window, cx| {
                                    cx.stop_propagation();
                                    this.submit(window, cx);
                                }))
                                .child(
                                    Input::new(&self.name)
                                        .with_size(Size::Small)
                                        .disabled(self.saving)
                                        .font_family(cx.theme().font_family.clone())
                                        .text_color(foreground)
                                        .aria_label("File name"),
                                ),
                        )
                        .when_some(self.error.clone(), |this, error| {
                            this.child(div().text_xs().text_color(cx.theme().danger).child(error))
                        })
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .text_xs()
                                .child("Formatting")
                                .child(
                                    gpui_kit::base::RadioGroup::new("save-formatting")
                                        .aria_label("Formatting")
                                        .flex()
                                        .flex_col()
                                        .gap_2()
                                        .on_key_down(cx.listener(
                                            |this, event: &gpui_kit::KeyDownEvent, window, cx| {
                                                if this.saving {
                                                    return;
                                                }
                                                let current = this.formatting as usize;
                                                let next = match event.keystroke.key.as_str() {
                                                    "up" | "left" => (current + 2) % 3,
                                                    "down" | "right" => (current + 1) % 3,
                                                    "home" => 0,
                                                    "end" => 2,
                                                    _ => return,
                                                };
                                                window.prevent_default();
                                                cx.stop_propagation();
                                                this.select_formatting(
                                                    Formatting::CHOICES[next].0,
                                                    window,
                                                    cx,
                                                );
                                            },
                                        ))
                                        .children(Formatting::CHOICES.map(
                                            |(formatting, label)| {
                                                let checked = self.formatting == formatting;
                                                let focus = self.formatting_focus
                                                    [formatting as usize]
                                                    .clone()
                                                    .tab_stop(checked && !self.saving);
                                                let select =
                                                    cx.listener(move |this, _: &(), window, cx| {
                                                        this.select_formatting(
                                                            formatting, window, cx,
                                                        );
                                                    });
                                                gpui_kit::base::Radio::new(formatting.value())
                                                    .track_focus(&focus)
                                                    .set_position(formatting as usize + 1, 3)
                                                    .flex()
                                                    .items_center()
                                                    .gap_2()
                                                    .rounded_sm()
                                                    .cursor_pointer()
                                                    .disabled(self.saving)
                                                    .checked(checked)
                                                    .accessibility_label(label)
                                                    .when(
                                                        focus.is_focused(window)
                                                            && window.last_input_was_keyboard(),
                                                        |this| this.focus_ring_style(window, cx),
                                                    )
                                                    .when(self.saving, |this| this.opacity(0.5))
                                                    .child(
                                                        div()
                                                            .size(rems(0.875))
                                                            .flex_shrink_0()
                                                            .flex()
                                                            .items_center()
                                                            .justify_center()
                                                            .border_1()
                                                            .border_color(foreground)
                                                            .rounded_full()
                                                            .when(checked, |this| {
                                                                this.child(
                                                                    div()
                                                                        .size(rems(0.375))
                                                                        .rounded_full()
                                                                        .bg(foreground),
                                                                )
                                                            }),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_xs()
                                                            .text_color(foreground)
                                                            .child(label),
                                                    )
                                                    .on_change(move |_, _, window, cx| {
                                                        select(&(), window, cx)
                                                    })
                                            },
                                        )),
                                ),
                        )
                        .child(
                            Checkbox::new("convert-json")
                                .indicator_color(foreground)
                                .with_size(Size::Small)
                                .items_center()
                                .cursor_pointer()
                                .disabled(self.saving)
                                .checked(self.convert_to_json)
                                .accessibility_label("Convert to JSON")
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(foreground)
                                        .child("Convert to JSON"),
                                )
                                .on_change(cx.listener(|this, checked, _, cx| {
                                    this.convert_to_json = *checked;
                                    super::store_save_option(CONVERT_TO_JSON, *checked);
                                    this.error = None;
                                    cx.notify();
                                })),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_end()
                                .gap_2()
                                .child(
                                    super::bordered_action_button(
                                        "cancel-save-json5",
                                        "Cancel",
                                        cx,
                                    )
                                    .disabled(self.saving)
                                    .on_click(cx.listener(
                                        |this, _, window, cx| {
                                            this.dismiss(window, cx);
                                        },
                                    )),
                                )
                                .child(
                                    super::bordered_action_button(
                                        "confirm-save-json5",
                                        if self.saving {
                                            "Downloading…"
                                        } else {
                                            "Download"
                                        },
                                        cx,
                                    )
                                    .disabled(self.saving)
                                    .on_click(
                                        cx.listener(|this, _, window, cx| this.submit(window, cx)),
                                    ),
                                ),
                        )
                        .focus_trap("save-overlay-focus", &self.focus_handle),
                ),
            )
            .into_any_element()
    }
}
