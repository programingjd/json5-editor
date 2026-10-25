use std::rc::Rc;

use gpui::{
    AnyElement, App, ClickEvent, Div, ElementId, FocusHandle, InteractiveElement, Interactivity,
    IntoElement, ParentElement, Refineable as _, RenderOnce, Role, SharedString, Stateful,
    StatefulInteractiveElement, StyleRefinement, Styled, Toggled, Window, div,
    prelude::FluentBuilder as _,
};
use smallvec::SmallVec;

use crate::{RoleOverride, StateStyle, StyledExt as _, TestSupportExt as _};

type ChangeHandler = Rc<dyn Fn(CheckboxState, &ClickEvent, &mut Window, &mut App)>;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CheckboxState {
    #[default]
    Unchecked,
    Checked,
    Indeterminate,
}

impl CheckboxState {
    fn toggled(self) -> Toggled {
        match self {
            Self::Unchecked => Toggled::False,
            Self::Checked => Toggled::True,
            Self::Indeterminate => Toggled::Mixed,
        }
    }

    fn activated(self) -> Self {
        match self {
            Self::Unchecked | Self::Indeterminate => Self::Checked,
            Self::Checked => Self::Unchecked,
        }
    }
}

#[derive(IntoElement)]
pub struct Checkbox {
    id: ElementId,
    base: Stateful<Div>,
    style: StyleRefinement,
    semantic_styles: CheckboxStyles,
    state: CheckboxState,
    disabled: bool,
    children: SmallVec<[AnyElement; 2]>,
    on_change: Option<ChangeHandler>,
    accessibility_label: Option<SharedString>,
    tab_index: isize,
    tab_stop: bool,
    provided_focus_handle: Option<FocusHandle>,
    role: RoleOverride,
}

impl Checkbox {
    pub fn new(id: impl Into<ElementId>) -> Self {
        let id = id.into();
        Self {
            base: div().id(id.clone()),
            id,
            style: StyleRefinement::default(),
            semantic_styles: CheckboxStyles::default(),
            state: CheckboxState::Unchecked,
            disabled: false,
            children: SmallVec::new(),
            on_change: None,
            accessibility_label: None,
            tab_index: 0,
            tab_stop: true,
            provided_focus_handle: None,
            role: RoleOverride::Implicit,
        }
    }
    pub fn role(mut self, role: impl Into<RoleOverride>) -> Self {
        self.role = role.into();
        self
    }

    pub fn state(mut self, state: CheckboxState) -> Self {
        self.state = state;
        self
    }

    pub fn checked(self, checked: bool) -> Self {
        self.state(if checked {
            CheckboxState::Checked
        } else {
            CheckboxState::Unchecked
        })
    }

    pub fn indeterminate(self, indeterminate: bool) -> Self {
        if indeterminate {
            self.state(CheckboxState::Indeterminate)
        } else if self.state == CheckboxState::Indeterminate {
            self.state(CheckboxState::Unchecked)
        } else {
            self
        }
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn styles(mut self, build: impl FnOnce(CheckboxStyles) -> CheckboxStyles) -> Self {
        self.semantic_styles = build(self.semantic_styles);
        self
    }

    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        self.accessibility_label = Some(label.into());
        self
    }

    pub fn on_change(
        mut self,
        handler: impl Fn(CheckboxState, &ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }

    pub fn tab_index(mut self, tab_index: isize) -> Self {
        self.tab_index = tab_index;
        self
    }

    pub fn tab_stop(mut self, tab_stop: bool) -> Self {
        self.tab_stop = tab_stop;
        self
    }

    pub fn track_focus(mut self, focus_handle: &FocusHandle) -> Self {
        self.provided_focus_handle = Some(focus_handle.clone());
        self
    }

    fn focus_handle(&self, window: &mut Window, cx: &mut App) -> FocusHandle {
        self.provided_focus_handle.clone().unwrap_or_else(|| {
            window
                .use_keyed_state(self.id.clone(), cx, |_, cx| cx.focus_handle())
                .read(cx)
                .clone()
        })
    }

    fn resolved_style(&self) -> StyleRefinement {
        crate::state_style::resolve_style(
            &self.style,
            [
                match self.state {
                    CheckboxState::Unchecked => None,
                    CheckboxState::Checked => Some(&self.semantic_styles.checked),
                    CheckboxState::Indeterminate => Some(&self.semantic_styles.indeterminate),
                },
                self.disabled.then_some(&self.semantic_styles.disabled),
            ]
            .into_iter()
            .flatten(),
        )
    }
}

#[derive(Default)]
pub struct CheckboxStyles {
    checked: StyleRefinement,
    indeterminate: StyleRefinement,
    disabled: StyleRefinement,
}

impl CheckboxStyles {
    pub fn checked(mut self, build: impl FnOnce(StateStyle) -> StateStyle) -> Self {
        self.checked
            .refine(&build(StateStyle::default()).into_refinement());
        self
    }

    pub fn indeterminate(mut self, build: impl FnOnce(StateStyle) -> StateStyle) -> Self {
        self.indeterminate
            .refine(&build(StateStyle::default()).into_refinement());
        self
    }

    pub fn disabled(mut self, build: impl FnOnce(StateStyle) -> StateStyle) -> Self {
        self.disabled
            .refine(&build(StateStyle::default()).into_refinement());
        self
    }
}

#[derive(IntoElement)]
pub struct CheckboxIndicator {
    base: Div,
    style: StyleRefinement,
    semantic_styles: CheckboxIndicatorStyles,
    state: CheckboxState,
    disabled: bool,
    children: SmallVec<[AnyElement; 1]>,
}

#[derive(Default)]
pub struct CheckboxIndicatorStyles {
    checked: StyleRefinement,
    indeterminate: StyleRefinement,
    disabled: StyleRefinement,
}

impl CheckboxIndicatorStyles {
    pub fn checked(mut self, build: impl FnOnce(StateStyle) -> StateStyle) -> Self {
        self.checked
            .refine(&build(StateStyle::default()).into_refinement());
        self
    }

    pub fn indeterminate(mut self, build: impl FnOnce(StateStyle) -> StateStyle) -> Self {
        self.indeterminate
            .refine(&build(StateStyle::default()).into_refinement());
        self
    }

    pub fn disabled(mut self, build: impl FnOnce(StateStyle) -> StateStyle) -> Self {
        self.disabled
            .refine(&build(StateStyle::default()).into_refinement());
        self
    }
}

impl CheckboxIndicator {
    pub fn new() -> Self {
        Self {
            base: div(),
            style: StyleRefinement::default(),
            semantic_styles: CheckboxIndicatorStyles::default(),
            state: CheckboxState::Unchecked,
            disabled: false,
            children: SmallVec::new(),
        }
    }

    pub fn state(mut self, state: CheckboxState) -> Self {
        self.state = state;
        self
    }

    pub fn checked(self, checked: bool) -> Self {
        self.state(if checked {
            CheckboxState::Checked
        } else {
            CheckboxState::Unchecked
        })
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn styles(
        mut self,
        build: impl FnOnce(CheckboxIndicatorStyles) -> CheckboxIndicatorStyles,
    ) -> Self {
        self.semantic_styles = build(self.semantic_styles);
        self
    }

    fn resolved_style(&self) -> StyleRefinement {
        crate::state_style::resolve_style(
            &self.style,
            [
                match self.state {
                    CheckboxState::Unchecked => None,
                    CheckboxState::Checked => Some(&self.semantic_styles.checked),
                    CheckboxState::Indeterminate => Some(&self.semantic_styles.indeterminate),
                },
                self.disabled.then_some(&self.semantic_styles.disabled),
            ]
            .into_iter()
            .flatten(),
        )
    }
}

impl Default for CheckboxIndicator {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for CheckboxIndicator {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl ParentElement for CheckboxIndicator {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for CheckboxIndicator {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let style = self.resolved_style();
        self.base.children(self.children).refine_style(&style)
    }
}

impl Styled for Checkbox {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl ParentElement for Checkbox {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl InteractiveElement for Checkbox {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl StatefulInteractiveElement for Checkbox {}

impl RenderOnce for Checkbox {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let focus_handle = self.focus_handle(window, cx);
        let disabled = self.disabled;
        let next_state = self.state.activated();
        let style = self.resolved_style();
        let on_change = self.on_change;

        self.base
            .test_support()
            .when_some(self.role.resolve(|| Role::CheckBox), |this, role| {
                this.role(role)
            })
            .aria_toggled(self.state.toggled())
            .when_some(self.accessibility_label, |this, label| {
                this.aria_label(label)
            })
            .when(!disabled, |this| {
                this.track_focus(
                    &focus_handle
                        .tab_index(self.tab_index)
                        .tab_stop(self.tab_stop),
                )
            })
            .when_some(
                (!disabled).then_some(on_change).flatten(),
                |this, on_change| {
                    this.on_click(move |event, window, cx| {
                        on_change(next_state, event, window, cx);
                    })
                },
            )
            .children(self.children)
            .refine_style(&style)
    }
}
