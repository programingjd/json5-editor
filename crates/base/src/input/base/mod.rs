use crate::{StateStyle, StyledExt as _, TestSupportExt as _};
use gpui::{
    AnyElement, App, Div, ElementId, InteractiveElement, Interactivity, IntoElement, ParentElement,
    Refineable as _, RenderOnce, Role, SharedString, StatefulInteractiveElement, StyleRefinement,
    Styled, Window, div, prelude::FluentBuilder as _,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InputContextMenuCapabilities {
    disabled: bool,
    readonly: bool,
    code_editor: bool,
    selection: bool,
    masked: bool,
}

impl InputContextMenuCapabilities {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn readonly(mut self, readonly: bool) -> Self {
        self.readonly = readonly;
        self
    }

    pub fn code_editor(mut self, code_editor: bool) -> Self {
        self.code_editor = code_editor;
        self
    }

    pub fn selection(mut self, selection: bool) -> Self {
        self.selection = selection;
        self
    }

    pub fn masked(mut self, masked: bool) -> Self {
        self.masked = masked;
        self
    }

    pub fn is_disabled(&self) -> bool {
        self.disabled
    }

    pub fn is_readonly(&self) -> bool {
        self.readonly
    }

    pub fn is_editable(&self) -> bool {
        !self.disabled && !self.readonly
    }

    pub fn is_code_editor(&self) -> bool {
        self.code_editor
    }

    pub fn has_selection(&self) -> bool {
        self.selection
    }

    pub fn is_masked(&self) -> bool {
        self.masked
    }

    pub fn is_copyable(&self) -> bool {
        self.selection && !self.masked
    }
}

#[derive(IntoElement)]
pub struct InputBase {
    base: crate::ObservedElement<gpui::Stateful<Div>>,
    style: StyleRefinement,
    semantic_styles: InputStyles,
    children: Vec<AnyElement>,
    focused: bool,
    disabled: bool,
    role: crate::RoleOverride,
}

impl InputBase {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            base: div().id(id).test_support(),
            style: StyleRefinement::default(),
            semantic_styles: InputStyles::default(),
            children: Vec::new(),
            focused: false,
            disabled: false,
            role: crate::RoleOverride::Implicit,
        }
    }
    pub fn role(mut self, role: impl Into<crate::RoleOverride>) -> Self {
        self.role = role.into();
        self
    }

    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn styles(mut self, build: impl FnOnce(InputStyles) -> InputStyles) -> Self {
        self.semantic_styles = build(self.semantic_styles);
        self
    }

    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        self.base = self.base.aria_label(label.into());
        self
    }

    fn resolved_style(&self) -> StyleRefinement {
        crate::state_style::resolve_style(
            &self.style,
            [
                self.focused.then_some(&self.semantic_styles.focused),
                self.disabled.then_some(&self.semantic_styles.disabled),
            ]
            .into_iter()
            .flatten(),
        )
    }
}

#[derive(Default)]
pub struct InputStyles {
    focused: StyleRefinement,
    disabled: StyleRefinement,
}

impl InputStyles {
    pub fn focused(mut self, build: impl FnOnce(StateStyle) -> StateStyle) -> Self {
        self.focused
            .refine(&build(StateStyle::default()).into_refinement());
        self
    }

    pub fn disabled(mut self, build: impl FnOnce(StateStyle) -> StateStyle) -> Self {
        self.disabled
            .refine(&build(StateStyle::default()).into_refinement());
        self
    }
}

impl Styled for InputBase {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl ParentElement for InputBase {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl InteractiveElement for InputBase {
    fn track_focus(mut self, focus: &gpui::FocusHandle) -> Self {
        self.base = self.base.track_focus(focus);
        self
    }

    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl StatefulInteractiveElement for InputBase {}

impl RenderOnce for InputBase {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let style = self.resolved_style();
        self.base
            .when_some(self.role.resolve(|| Role::TextInput), |this, role| {
                this.role(role)
            })
            .children(self.children)
            .refine_style(&style)
    }
}
