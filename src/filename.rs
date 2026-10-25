use gpui_kit::component::{
    ActiveTheme as _, Sizable as _, Size,
    button::{Button, ButtonVariants as _},
    input::{EditorState, Input, InputEvent, InputState},
};
use gpui_kit::{
    App, AppContext as _, Context, Entity, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, Subscription, Window, div,
};

pub(super) struct Filename {
    name: Entity<SharedString>,
    editor: Entity<EditorState>,
    entry: Option<Entity<InputState>>,
    initial_value: Option<SharedString>,
    entry_subscription: Option<Subscription>,
    _name_observer: Subscription,
}

impl Filename {
    pub(super) fn is_editing(&self) -> bool {
        self.entry.is_some()
    }

    pub(super) fn commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.finish_edit(true, false, window, cx);
    }

    pub(super) fn new(
        name: Entity<SharedString>,
        editor: Entity<EditorState>,
        cx: &mut App,
    ) -> Entity<Self> {
        cx.new(|cx: &mut Context<Self>| Self {
            _name_observer: cx.observe(&name, |this, _, cx| {
                this.entry = None;
                this.initial_value = None;
                this.entry_subscription = None;
                cx.notify();
            }),
            name,
            editor,
            entry: None,
            initial_value: None,
            entry_subscription: None,
        })
    }

    fn begin_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let entry = cx.new(|cx| InputState::new(window, cx).placeholder("Filename"));
        self.entry_subscription =
            Some(
                cx.subscribe_in(&entry, window, |this, _, event, window, cx| {
                    if matches!(event, InputEvent::Blur) {
                        this.finish_edit(true, false, window, cx);
                    }
                }),
            );
        self.entry = Some(entry.clone());
        self.initial_value = Some(self.name.read(cx).clone());
        // Route keystrokes away from the document immediately, including before
        // the field's first styled layout has run.
        entry.update(cx, |state, cx| state.focus(window, cx));
        cx.notify();
    }

    fn initialize_entry(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(value) = self.initial_value.take() else {
            return;
        };
        let Some(entry) = self.entry.clone() else {
            return;
        };
        // Wait for Input's first layout to install the UI font before measuring.
        cx.on_next_frame(window, move |this, window, cx| {
            if this.entry.as_ref() != Some(&entry) {
                return;
            }
            entry.update(cx, |state, cx| {
                if state.value().is_empty() {
                    state.set_value(value, window, cx);
                    state.select_all(window, cx);
                }
                state.focus(window, cx);
            });
        });
    }

    fn finish_edit(
        &mut self,
        apply: bool,
        restore_focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(entry) = self.entry.take() else {
            return;
        };
        self.initial_value = None;
        self.entry_subscription = None;
        if apply {
            let name = super::safe_file_name(entry.read(cx).value().as_ref());
            if !name.is_empty() && name.as_str() != self.name.read(cx).as_ref() {
                self.name.update(cx, |current, cx| {
                    *current = name.into();
                    cx.notify();
                });
            }
        }
        if restore_focus {
            self.editor.update(cx, |state, cx| state.focus(window, cx));
        }
        cx.notify();
    }
}

impl Render for Filename {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(entry) = &self.entry {
            let owner = cx.weak_entity();
            div()
                .w_full()
                .min_w_0()
                .on_children_prepainted(move |_, window, cx| {
                    let _ = owner.update(cx, |this, cx| this.initialize_entry(window, cx));
                })
                .capture_action(cx.listener(
                    |this, _: &gpui_kit::base::input::Enter, window, cx| {
                        cx.stop_propagation();
                        this.finish_edit(true, true, window, cx);
                    },
                ))
                .capture_action(cx.listener(
                    |this, _: &gpui_kit::base::input::Escape, window, cx| {
                        cx.stop_propagation();
                        this.finish_edit(false, true, window, cx);
                    },
                ))
                .child(
                    Input::new(entry)
                        .with_size(Size::XSmall)
                        .appearance(false)
                        .font_family(cx.theme().font_family.clone())
                        .text_center()
                        .text_color(cx.theme().popover_foreground)
                        .w_full()
                        .aria_label("Filename"),
                )
                .into_any_element()
        } else {
            Button::new("edit-filename")
                .ghost()
                .cursor_pointer()
                .with_size(Size::XSmall)
                .max_w_full()
                .text_xs()
                .font_weight(FontWeight::NORMAL)
                .text_color(cx.theme().popover_foreground)
                .label(self.name.read(cx).clone())
                .accessibility_label("Rename file")
                .tooltip("Rename file")
                .on_click(cx.listener(|this, _, window, cx| this.begin_edit(window, cx)))
                .into_any_element()
        }
    }
}
