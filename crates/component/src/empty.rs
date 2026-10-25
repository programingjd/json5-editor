use gpui::{
    AnyElement, App, IntoElement, ParentElement, RenderOnce, StyleRefinement, Styled, Window, div,
    prelude::FluentBuilder as _, relative, rems,
};

use crate::{ActiveTheme as _, StyledExt as _, v_flex};

#[derive(IntoElement)]
pub struct Empty {
    style: StyleRefinement,
    header: Option<EmptyHeader>,
    content: Option<EmptyContent>,
    children: Vec<AnyElement>,
}

impl Empty {
    pub fn new() -> Self {
        Self {
            style: StyleRefinement::default(),
            header: None,
            content: None,
            children: Vec::new(),
        }
    }

    pub fn header(mut self, header: EmptyHeader) -> Self {
        self.header = Some(header);
        self
    }

    pub fn content(mut self, content: EmptyContent) -> Self {
        self.content = Some(content);
        self
    }
}

impl Default for Empty {
    fn default() -> Self {
        Self::new()
    }
}

impl ParentElement for Empty {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for Empty {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Empty {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        v_flex()
            .w_full()
            .min_w_0()
            .flex_1()
            .items_center()
            .justify_center()
            .gap_4()
            .p_6()
            .rounded(cx.theme().radius_tokens().xl)
            .border_dashed()
            .border_color(cx.theme().border)
            .text_center()
            .text_color(cx.theme().foreground)
            .refine_style(&self.style)
            .when_some(self.header, |this, header| this.child(header))
            .when_some(self.content, |this, content| this.child(content))
            .children(self.children)
    }
}

#[derive(IntoElement)]
pub struct EmptyHeader {
    style: StyleRefinement,
    media: Option<EmptyMedia>,
    title: Option<EmptyTitle>,
    description: Option<EmptyDescription>,
}

impl EmptyHeader {
    pub fn new() -> Self {
        Self {
            style: StyleRefinement::default(),
            media: None,
            title: None,
            description: None,
        }
    }

    pub fn media(mut self, media: EmptyMedia) -> Self {
        self.media = Some(media);
        self
    }

    pub fn title(mut self, title: EmptyTitle) -> Self {
        self.title = Some(title);
        self
    }

    pub fn description(mut self, description: EmptyDescription) -> Self {
        self.description = Some(description);
        self
    }
}

impl Default for EmptyHeader {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for EmptyHeader {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for EmptyHeader {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        v_flex()
            .w_full()
            .max_w(rems(24.))
            .min_w_0()
            .items_center()
            .gap_2()
            .refine_style(&self.style)
            .when_some(self.media, |this, media| this.child(media))
            .when_some(self.title, |this, title| this.child(title))
            .when_some(self.description, |this, description| {
                this.child(description)
            })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EmptyMediaVariant {
    #[default]
    Default,

    Icon,
}

#[derive(IntoElement)]
pub struct EmptyMedia {
    style: StyleRefinement,
    variant: EmptyMediaVariant,
    children: Vec<AnyElement>,
}

impl EmptyMedia {
    pub fn new() -> Self {
        Self {
            style: StyleRefinement::default(),
            variant: EmptyMediaVariant::Default,
            children: Vec::new(),
        }
    }

    pub fn with_variant(mut self, variant: EmptyMediaVariant) -> Self {
        self.variant = variant;
        self
    }
}

impl Default for EmptyMedia {
    fn default() -> Self {
        Self::new()
    }
}

impl ParentElement for EmptyMedia {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for EmptyMedia {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for EmptyMedia {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        // A column preserves the intrinsic width of nested rows such as AvatarGroup.
        v_flex()
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .mb_2()
            .when(self.variant == EmptyMediaVariant::Icon, |this| {
                this.size_8()
                    .rounded(cx.theme().radius_tokens().lg)
                    .bg(cx.theme().muted)
                    .text_color(cx.theme().foreground)
                    // Icon inherits one rem unless it has an explicit size.
                    .text_base()
            })
            .refine_style(&self.style)
            .children(self.children)
    }
}

#[derive(IntoElement)]
pub struct EmptyTitle {
    style: StyleRefinement,
    children: Vec<AnyElement>,
}

impl EmptyTitle {
    pub fn new() -> Self {
        Self {
            style: StyleRefinement::default(),
            children: Vec::new(),
        }
    }
}

impl Default for EmptyTitle {
    fn default() -> Self {
        Self::new()
    }
}

impl ParentElement for EmptyTitle {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for EmptyTitle {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for EmptyTitle {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div()
            .max_w_full()
            .min_w_0()
            .text_sm()
            .font_medium()
            .whitespace_normal()
            .refine_style(&self.style)
            .children(self.children)
    }
}

#[derive(IntoElement)]
pub struct EmptyDescription {
    style: StyleRefinement,
    children: Vec<AnyElement>,
}

impl EmptyDescription {
    pub fn new() -> Self {
        Self {
            style: StyleRefinement::default(),
            children: Vec::new(),
        }
    }
}

impl Default for EmptyDescription {
    fn default() -> Self {
        Self::new()
    }
}

impl ParentElement for EmptyDescription {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for EmptyDescription {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for EmptyDescription {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        div()
            .w_full()
            .min_w_0()
            .text_sm()
            .line_height(relative(1.625))
            .text_color(cx.theme().muted_foreground)
            .whitespace_normal()
            .refine_style(&self.style)
            .children(self.children)
    }
}

#[derive(IntoElement)]
pub struct EmptyContent {
    style: StyleRefinement,
    children: Vec<AnyElement>,
}

impl EmptyContent {
    pub fn new() -> Self {
        Self {
            style: StyleRefinement::default(),
            children: Vec::new(),
        }
    }
}

impl Default for EmptyContent {
    fn default() -> Self {
        Self::new()
    }
}

impl ParentElement for EmptyContent {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for EmptyContent {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for EmptyContent {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        v_flex()
            .w_full()
            .max_w(rems(24.))
            .min_w_0()
            .items_center()
            .gap_2p5()
            .text_sm()
            .refine_style(&self.style)
            .children(self.children)
    }
}
