use gpui::{
    AnyElement, App, Div, ElementId, FocusHandle, InteractiveElement, Interactivity, IntoElement,
    KeyDownEvent, ParentElement, RenderOnce, Role, SharedString, Stateful,
    StatefulInteractiveElement, StyleRefinement, Styled, Window, accesskit, div,
    prelude::FluentBuilder as _,
};
use smallvec::SmallVec;

use crate::StyledExt as _;

const MAX_FOCUS_ATTEMPTS: usize = 100;

#[derive(IntoElement)]
pub struct Toolbar {
    id: ElementId,
    base: Stateful<Div>,
    style: StyleRefinement,
    disabled: bool,
    children: SmallVec<[AnyElement; 4]>,
}

impl Toolbar {
    pub fn new(id: impl Into<ElementId>) -> Self {
        let id = id.into();
        Self {
            base: div().id(id.clone()),
            style: StyleRefinement::default(),
            disabled: false,
            children: SmallVec::new(),
            id,
        }
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

fn step_focus(window: &mut Window, cx: &mut App, forward: bool) {
    if forward {
        window.focus_next(cx);
    } else {
        window.focus_prev(cx);
    }
}

fn move_focus(container: &FocusHandle, forward: bool, window: &mut Window, cx: &mut App) {
    let Some(start) = window.focused(cx) else {
        return;
    };

    step_focus(window, cx, forward);
    if container.contains_focused(window, cx)
        && window.focused(cx).is_some_and(|focused| focused != start)
    {
        return;
    }

    for _ in 0..MAX_FOCUS_ATTEMPTS {
        step_focus(window, cx, forward);
        if container.contains_focused(window, cx)
            && window.focused(cx).is_some_and(|focused| focused != start)
        {
            return;
        }
        if window.focused(cx).is_some_and(|focused| focused == start) {
            break;
        }
    }

    window.focus(&start, cx);
}

fn handle_key_down(
    disabled: bool,
    container: &FocusHandle,
    event: &KeyDownEvent,
    window: &mut Window,
    cx: &mut App,
) {
    if disabled {
        return;
    }

    let forward = match event.keystroke.key.as_str() {
        "left" => Some(false),
        "right" => Some(true),
        _ => None,
    };

    if let Some(forward) = forward {
        move_focus(container, forward, window, cx);
        cx.stop_propagation();
    }
}

impl Styled for Toolbar {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl ParentElement for Toolbar {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl InteractiveElement for Toolbar {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl StatefulInteractiveElement for Toolbar {}

impl RenderOnce for Toolbar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Self {
            base,
            style,
            disabled,
            children,
            ..
        } = self;

        // The handle must survive across frames so containment checks and the
        // key handler refer to the same dispatch-tree node; button.rs uses
        // the same keyed-state pattern for its own focus handle.
        let focus_handle = window
            .use_keyed_state(self.id.clone(), cx, |_, cx| {
                cx.focus_handle().tab_stop(false)
            })
            .read(cx)
            .clone();
        let key_handler = {
            let focus_handle = focus_handle.clone();
            move |event: &KeyDownEvent, window: &mut Window, cx: &mut App| {
                handle_key_down(disabled, &focus_handle, event, window, cx);
            }
        };

        base.track_focus(&focus_handle)
            .role(Role::Toolbar)
            .aria_orientation(accesskit::Orientation::Horizontal)
            .on_key_down(key_handler)
            .children(children)
            .refine_style(&style)
    }
}

#[derive(IntoElement)]
pub struct ToolbarGroup {
    base: Stateful<Div>,
    style: StyleRefinement,
    label: Option<SharedString>,
    children: SmallVec<[AnyElement; 4]>,
}

impl ToolbarGroup {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            base: div().id(id.into()),
            style: StyleRefinement::default(),
            label: None,
            children: SmallVec::new(),
        }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }
}

impl Styled for ToolbarGroup {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl ParentElement for ToolbarGroup {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl InteractiveElement for ToolbarGroup {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl StatefulInteractiveElement for ToolbarGroup {}

impl RenderOnce for ToolbarGroup {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        // A group is one inline segment of the bar, so its children flow
        // along the row and center on the bar's cross axis; the same neutral
        // geometry `Tab` applies. Spacing between items is the caller's
        // (matching the bar's own gap).
        self.base
            .flex()
            .items_center()
            .role(Role::Group)
            .when_some(self.label, |this, label| this.aria_label(label))
            .children(self.children)
            .refine_style(&self.style)
    }
}
