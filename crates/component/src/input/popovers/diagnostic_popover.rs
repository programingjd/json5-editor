use std::rc::Rc;

use gpui::{
    App, AppContext as _, Context, Empty, Entity, IntoElement, Render, Styled, WeakEntity, Window,
};

use crate::{
    ActiveTheme as _,
    highlighter::DiagnosticEntry,
    input::{
        EditorState,
        popovers::{Popover, render_markdown},
    },
};

pub struct DiagnosticPopover {
    state: WeakEntity<EditorState>,
    pub(crate) diagnostic: Rc<DiagnosticEntry>,
}

impl DiagnosticPopover {
    pub fn new(
        diagnostic: &DiagnosticEntry,
        state: Entity<EditorState>,
        cx: &mut App,
    ) -> Entity<Self> {
        let diagnostic = Rc::new(diagnostic.clone());

        cx.new(|_| Self {
            diagnostic,
            state: state.downgrade(),
        })
    }
}

impl Render for DiagnosticPopover {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(state) = self.state.upgrade() else {
            return Empty.into_any_element();
        };
        let message = self.diagnostic.message.clone();

        Popover::new(
            "diagnostic-popover",
            state,
            self.diagnostic.range.clone(),
            move |window, cx| render_markdown("message", message.clone(), window, cx),
        )
        .text_size(crate::tooltip::tooltip_text_size())
        .text_color(cx.theme().popover_foreground)
        .into_any_element()
    }
}
