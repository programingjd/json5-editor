use gpui::{
    AnyElement, App, ElementId, IntoElement, ParentElement, RenderOnce, SharedString,
    StyleRefinement, Styled, Window, div, prelude::FluentBuilder as _,
};
use smallvec::SmallVec;

use gpui_base::{Toolbar as BaseToolbar, ToolbarGroup as BaseToolbarGroup};

use crate::{Sizable, Size, StyleSized as _, StyledExt as _};

enum ToolbarItem {
    Sized(Box<dyn FnOnce(Size) -> AnyElement>),
    Content(AnyElement),
}

impl ToolbarItem {
    fn sized(item: impl Sizable + IntoElement + 'static) -> Self {
        Self::Sized(Box::new(move |size| {
            item.prepare_for_toolbar()
                .with_size(size)
                .into_any_element()
        }))
    }

    fn content(content: impl IntoElement) -> Self {
        Self::Content(content.into_any_element())
    }

    fn into_element(self, size: Size) -> AnyElement {
        match self {
            Self::Sized(item) => div()
                .flex()
                .items_center()
                .input_h(size)
                .child(item(size))
                .into_any_element(),
            Self::Content(content) => content,
        }
    }
}

#[derive(IntoElement)]
pub struct ToolbarGroup {
    id: ElementId,
    style: StyleRefinement,
    size: Size,
    label: Option<SharedString>,
    children: SmallVec<[ToolbarItem; 4]>,
}

impl ToolbarGroup {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            style: StyleRefinement::default(),
            size: Size::Small,
            label: None,
            children: SmallVec::new(),
        }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn child(mut self, child: impl Sizable + IntoElement + 'static) -> Self {
        self.children.push(ToolbarItem::sized(child));
        self
    }

    pub fn children<T>(mut self, children: impl IntoIterator<Item = T>) -> Self
    where
        T: Sizable + IntoElement + 'static,
    {
        self.children
            .extend(children.into_iter().map(ToolbarItem::sized));
        self
    }

    pub fn content(mut self, content: impl IntoElement) -> Self {
        self.children.push(ToolbarItem::content(content));
        self
    }
}

impl ParentElement for ToolbarGroup {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children
            .extend(elements.into_iter().map(ToolbarItem::Content));
    }
}

impl Styled for ToolbarGroup {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl Sizable for ToolbarGroup {
    fn with_size(mut self, size: impl Into<Size>) -> Self {
        self.size = match size.into() {
            Size::Large => Size::Medium,
            size => size,
        };
        self
    }
}

impl RenderOnce for ToolbarGroup {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let size = self.size;
        BaseToolbarGroup::new(self.id)
            .when_some(self.label, |this, label| this.label(label))
            .children(
                self.children
                    .into_iter()
                    .map(|item| item.into_element(size)),
            )
            .refine_style(&self.style)
    }
}

#[derive(IntoElement)]
pub struct Toolbar {
    id: ElementId,
    style: StyleRefinement,
    size: Size,
    disabled: bool,
    items: SmallVec<[ToolbarItem; 4]>,
}

impl Toolbar {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            style: StyleRefinement::default(),
            size: Size::Small,
            disabled: false,
            items: SmallVec::new(),
        }
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn child(mut self, child: impl Sizable + IntoElement + 'static) -> Self {
        self.items.push(ToolbarItem::sized(child));
        self
    }

    pub fn children<T>(mut self, children: impl IntoIterator<Item = T>) -> Self
    where
        T: Sizable + IntoElement + 'static,
    {
        self.items
            .extend(children.into_iter().map(ToolbarItem::sized));
        self
    }

    pub fn content(mut self, content: impl IntoElement) -> Self {
        self.items.push(ToolbarItem::content(content));
        self
    }

    pub fn contents(mut self, contents: impl IntoIterator<Item = AnyElement>) -> Self {
        self.items
            .extend(contents.into_iter().map(ToolbarItem::Content));
        self
    }
}

impl ParentElement for Toolbar {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.items
            .extend(elements.into_iter().map(ToolbarItem::Content));
    }
}

impl Styled for Toolbar {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl Sizable for Toolbar {
    fn with_size(mut self, size: impl Into<Size>) -> Self {
        self.size = match size.into() {
            Size::Large => Size::Medium,
            size => size,
        };
        self
    }
}

impl RenderOnce for Toolbar {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let size = self.size;
        let items = self.items.into_iter().map(|item| item.into_element(size));

        BaseToolbar::new(self.id)
            .disabled(self.disabled)
            .flex()
            .items_center()
            .flex_shrink_0()
            .map(|this| match size {
                Size::XSmall => this.h_7().p_1().gap_1().text_xs(),
                Size::Small => this.h_8().p_1().gap_1().text_sm(),
                _ => this.h_12().p_2().gap_2().text_sm(),
            })
            .refine_style(&self.style)
            .children(items)
    }
}
