use std::{cell::RefCell, rc::Rc};

use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _, Size,
    button::{Button, ButtonRounded, ButtonVariants as _},
    input::EditorState,
};
use gpui_kit::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, SharedString,
    Styled as _, Window, div, prelude::FluentBuilder as _, px, rems,
};

use crate::tree_sitter_highlighter::ParseSnapshot;

pub(super) struct ConvertAction {
    editor: Entity<EditorState>,
    syntax: Rc<RefCell<ParseSnapshot>>,
    busy: bool,
    error: Option<SharedString>,
}

impl ConvertAction {
    pub(super) fn new(
        editor: &Entity<EditorState>,
        syntax: Rc<RefCell<ParseSnapshot>>,
        cx: &mut App,
    ) -> Entity<Self> {
        cx.new(|_| Self {
            editor: editor.clone(),
            syntax,
            busy: false,
            error: None,
        })
    }

    pub(super) fn is_busy(&self) -> bool {
        self.busy
    }

    pub(super) fn convert(&mut self, cx: &mut Context<Self>) {
        if self.busy || !self.editor.read(cx).is_editable() {
            return;
        }
        self.busy = true;
        self.error = None;
        self.editor
            .update(cx, |editor, cx| editor.set_readonly(true, cx));
        crate::request_convert_to_json();
        cx.notify();
    }

    pub(super) fn finish(
        &mut self,
        revision: u32,
        content: String,
        error: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.busy {
            return;
        }
        self.error = if !error.is_empty() {
            Some(error.into())
        } else if self.syntax.borrow().revision != revision {
            Some("Document changed during conversion. Try again.".into())
        } else {
            self.editor.update(cx, |editor, cx| {
                editor.replace_all(content, window, cx);
                editor.focus(window, cx);
            });
            None
        };
        self.editor
            .update(cx, |editor, cx| editor.set_readonly(false, cx));
        self.busy = false;
        cx.notify();
    }
}

impl Render for ConvertAction {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let busy = self.busy;
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                Button::new("convert-document-to-json")
                    .cursor_pointer()
                    .with_size(Size::Small)
                    .custom(crate::toolbar_button_style(cx))
                    .rounded(ButtonRounded::Size(px(4.)))
                    .child(
                        div()
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_size(rems(0.5625))
                            .font_weight(gpui_kit::FontWeight::NORMAL)
                            .child("JSON"),
                    )
                    .accessibility_label("Convert to JSON")
                    .tooltip_with(move |window, cx| {
                        crate::shortcuts::tooltip(
                            if busy {
                                "Converting to JSON…"
                            } else {
                                "Convert to JSON"
                            },
                            crate::shortcuts::CONVERT,
                            window,
                            cx,
                        )
                    })
                    .disabled(self.busy)
                    .on_click(cx.listener(|this, _, _, cx| this.convert(cx))),
            )
            .when_some(self.error.clone(), |this, error| {
                this.child(div().text_xs().text_color(cx.theme().danger).child(error))
            })
    }
}
