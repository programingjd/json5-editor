use std::rc::Rc;

use gpui::{
    App, DefiniteLength, Entity, IntoElement, RenderOnce, SharedString, StyleRefinement, Styled,
    Window, prelude::FluentBuilder as _,
};

use super::{Input, TextareaState};
use crate::context_menu::ContextMenu;
use crate::{RoleOverride, Sizable, Size, StyledExt as _};

#[derive(IntoElement)]
pub struct Textarea {
    token_renderer: Option<gpui_base::input::InlineTokenRenderer>,
    token_click_listener: Option<gpui_base::input::InlineTokenClickListener>,
    token_hover_listener: Option<gpui_base::input::InlineTokenHoverListener>,
    state: Entity<TextareaState>,
    style: StyleRefinement,
    size: Size,
    height: Option<DefiniteLength>,
    appearance: bool,
    bordered: bool,
    disabled: bool,
    readonly: bool,
    tab_index: isize,
    role: RoleOverride,
    accessibility_id: Option<SharedString>,
    aria_label: Option<SharedString>,

    context_menu_builder: Option<Rc<dyn Fn(ContextMenu, &mut Window, &mut App) -> ContextMenu>>,

    paste_handler: Option<Rc<dyn Fn(&gpui::ClipboardItem, &mut Window, &mut App) -> bool>>,
}

impl Textarea {
    pub fn token<R: IntoElement>(
        mut self,
        render: impl Fn(&super::InlineTokenContext, &mut Window, &mut App) -> R + 'static,
    ) -> Self {
        self.token_renderer = Some(Rc::new(move |token, window, cx| {
            render(token, window, cx).into_any_element()
        }));
        self
    }

    pub fn on_token_click(
        mut self,
        listener: impl Fn(&super::InlineTokenClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.token_click_listener = Some(Rc::new(listener));
        self
    }

    pub fn on_token_hover(
        mut self,
        listener: impl Fn(&super::InlineTokenHoverEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.token_hover_listener = Some(Rc::new(listener));
        self
    }

    pub fn new(state: &Entity<TextareaState>) -> Self {
        Self {
            state: state.clone(),
            style: StyleRefinement::default(),
            size: Size::default(),
            height: None,
            appearance: true,
            bordered: true,
            disabled: false,
            readonly: false,
            tab_index: 0,
            role: RoleOverride::default(),
            accessibility_id: None,
            aria_label: None,
            context_menu_builder: None,
            paste_handler: None,
            token_renderer: None,
            token_click_listener: None,
            token_hover_listener: None,
        }
    }

    pub fn h(mut self, height: impl Into<DefiniteLength>) -> Self {
        self.height = Some(height.into());
        self
    }

    pub fn appearance(mut self, appearance: bool) -> Self {
        self.appearance = appearance;
        self
    }

    pub fn bordered(mut self, bordered: bool) -> Self {
        self.bordered = bordered;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn readonly(mut self, readonly: bool) -> Self {
        self.readonly = readonly;
        self
    }

    pub fn tab_index(mut self, index: isize) -> Self {
        self.tab_index = index;
        self
    }

    pub fn role(mut self, role: impl Into<RoleOverride>) -> Self {
        self.role = role.into();
        self
    }

    pub fn accessibility_id(mut self, id: impl Into<SharedString>) -> Self {
        self.accessibility_id = Some(id.into());
        self
    }

    pub fn aria_label(mut self, label: impl Into<SharedString>) -> Self {
        self.aria_label = Some(label.into());
        self
    }

    pub fn context_menu(
        mut self,
        f: impl Fn(ContextMenu, &mut Window, &mut App) -> ContextMenu + 'static,
    ) -> Self {
        self.context_menu_builder = Some(Rc::new(f));
        self
    }

    pub fn on_paste(
        mut self,
        handler: impl Fn(&gpui::ClipboardItem, &mut Window, &mut App) -> bool + 'static,
    ) -> Self {
        self.paste_handler = Some(Rc::new(handler));
        self
    }
}

impl Sizable for Textarea {
    fn with_size(mut self, size: impl Into<Size>) -> Self {
        self.size = size.into();
        self
    }
}

impl Styled for Textarea {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl Textarea {
    pub(crate) fn into_input(self) -> Input {
        Input::from_state(self.state.clone())
            .when_some(self.token_renderer, |this, render| {
                this.token(move |token, window, cx| render(token, window, cx))
            })
            .when_some(self.token_click_listener, |this, listener| {
                this.on_token_click(move |event, window, cx| listener(event, window, cx))
            })
            .when_some(self.token_hover_listener, |this, listener| {
                this.on_token_hover(move |event, window, cx| listener(event, window, cx))
            })
            .appearance(self.appearance)
            .bordered(self.bordered)
            .disabled(self.disabled)
            .readonly(self.readonly)
            .tab_index(self.tab_index)
            .role(self.role)
            .with_size(self.size)
            .when_some(self.height, |this, height| this.h(height))
            .when_some(self.accessibility_id, |this, id| this.accessibility_id(id))
            .when_some(self.aria_label, |this, label| this.aria_label(label))
            .when_some(self.context_menu_builder, |this, build| {
                this.context_menu(move |menu, window, cx| build(menu, window, cx))
            })
            .when_some(self.paste_handler, |this, handler| {
                this.on_paste(move |item, window, cx| handler(item, window, cx))
            })
            .refine_style(&self.style)
    }
}

impl RenderOnce for Textarea {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.into_input()
    }
}
