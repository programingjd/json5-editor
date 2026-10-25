//! GPUI-rendered context menus for the browser editor.

use crate::{menu::PopupMenu, root::WindowState};
use gpui::{
    Action, App, Context, DismissEvent, Entity, FocusHandle, Focusable, IntoElement, ParentElement,
    Pixels, Point, Render, SharedString, Subscription, Window, anchored, deferred, div,
    prelude::FluentBuilder as _, px,
};

enum ContextMenuItem {
    Separator,
    Action {
        label: SharedString,
        disabled: bool,
        action: Box<dyn Action>,
    },
}

#[derive(Default)]
pub struct ContextMenu {
    items: Vec<ContextMenuItem>,
}

impl ContextMenu {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn menu(self, label: impl Into<SharedString>, action: Box<dyn Action>) -> Self {
        self.menu_with_disabled(label, false, action)
    }

    pub fn menu_with_disabled(
        mut self,
        label: impl Into<SharedString>,
        disabled: bool,
        action: Box<dyn Action>,
    ) -> Self {
        self.items.push(ContextMenuItem::Action {
            label: label.into(),
            disabled,
            action,
        });
        self
    }

    pub fn separator(mut self) -> Self {
        self.items.push(ContextMenuItem::Separator);
        self
    }

    pub fn show(self, position: Point<Pixels>, window: &mut Window, cx: &mut App) {
        if self.items.is_empty() {
            return;
        }
        let action_context = window.focused(cx);
        if let Some(overlay) = WindowState::context_menu_overlay(window, cx) {
            overlay.update(cx, |overlay, cx| {
                overlay.show(self.items, position, action_context, window, cx);
            });
        }
    }
}

pub(crate) struct ContextMenuOverlay {
    active: Option<ActiveMenu>,
}

struct ActiveMenu {
    menu: Entity<PopupMenu>,
    position: Point<Pixels>,
    _subscription: Subscription,
}

impl ContextMenuOverlay {
    pub(crate) fn new() -> Self {
        Self { active: None }
    }

    fn show(
        &mut self,
        items: Vec<ContextMenuItem>,
        position: Point<Pixels>,
        action_context: Option<FocusHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let menu = PopupMenu::build(window, cx, move |menu, _, _| {
            let menu = menu.when_some(action_context, |menu, handle| menu.action_context(handle));
            items.into_iter().fold(menu, |menu, item| match item {
                ContextMenuItem::Separator => menu.separator(),
                ContextMenuItem::Action {
                    label,
                    disabled,
                    action,
                } => menu.menu_with_check_and_disabled(label, false, action, disabled),
            })
        });
        let subscription = cx.subscribe(&menu, |this, _, _: &DismissEvent, cx| {
            this.active = None;
            cx.notify();
        });
        menu.focus_handle(cx).focus(window, cx);
        self.active = Some(ActiveMenu {
            menu,
            position,
            _subscription: subscription,
        });
        cx.notify();
    }
}

impl Render for ContextMenuOverlay {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().when_some(self.active.as_ref(), |root, active| {
            root.child(
                deferred(
                    anchored()
                        .position(active.position)
                        .snap_to_window_with_margin(px(8.))
                        .child(active.menu.clone()),
                )
                .with_priority(gpui_base::POPUP_PRIORITY),
            )
        })
    }
}
