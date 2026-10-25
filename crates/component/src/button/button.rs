use std::{rc::Rc, time::Duration};

use crate::{
    ActiveTheme, Colorize as _, Disableable, Icon, Placement, RoleOverride, Selectable, Sizable,
    Size, StyleSized, StyledExt,
    button::ButtonIcon,
    button::Caret,
    h_flex,
    styled::{FocusLine, focus_style},
    tooltip::{ManagedTooltipExt as _, Tooltip},
};
use gpui::{
    AnyElement, App, Background, ClickEvent, Corners, Edges, ElementId, Hsla, InteractiveElement,
    Interactivity, IntoElement, MouseButton, ParentElement, Pixels, RenderOnce, Role, SharedString,
    StatefulInteractiveElement as _, StyleRefinement, Styled, Window, div,
    prelude::FluentBuilder as _, relative, transparent_white,
};

const FOCUS_LINE_OPACITY: f32 = 0.6;

#[derive(Default, Clone, Copy)]
pub enum ButtonRounded {
    None,
    Small,
    #[default]
    Medium,
    Large,
    Size(Pixels),
}

impl From<Pixels> for ButtonRounded {
    fn from(px: Pixels) -> Self {
        ButtonRounded::Size(px)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ButtonCustomVariant {
    color: Hsla,
    foreground: Hsla,
    shadow: bool,
    hover: Hsla,
    active: Hsla,
}

pub trait ButtonVariants: Sized {
    fn with_variant(self, variant: ButtonVariant) -> Self;

    fn primary(self) -> Self {
        self.with_variant(ButtonVariant::Primary)
    }

    fn secondary(self) -> Self {
        self.with_variant(ButtonVariant::Secondary)
    }

    fn danger(self) -> Self {
        self.with_variant(ButtonVariant::Danger)
    }

    fn warning(self) -> Self {
        self.with_variant(ButtonVariant::Warning)
    }

    fn success(self) -> Self {
        self.with_variant(ButtonVariant::Success)
    }

    fn info(self) -> Self {
        self.with_variant(ButtonVariant::Info)
    }

    fn ghost(self) -> Self {
        self.with_variant(ButtonVariant::Ghost)
    }

    fn link(self) -> Self {
        self.with_variant(ButtonVariant::Link)
    }

    fn text(self) -> Self {
        self.with_variant(ButtonVariant::Text)
    }

    fn custom(self, style: ButtonCustomVariant) -> Self {
        self.with_variant(ButtonVariant::Custom(style))
    }
}

impl ButtonCustomVariant {
    pub fn new(cx: &App) -> Self {
        Self {
            color: cx.theme().transparent,
            foreground: cx.theme().foreground,
            hover: cx.theme().transparent,
            active: cx.theme().transparent,
            shadow: false,
        }
    }

    pub fn color(mut self, color: Hsla) -> Self {
        self.color = color;
        self
    }

    pub fn foreground(mut self, color: Hsla) -> Self {
        self.foreground = color;
        self
    }

    pub fn hover(mut self, color: Hsla) -> Self {
        self.hover = color;
        self
    }

    pub fn active(mut self, color: Hsla) -> Self {
        self.active = color;
        self
    }

    pub fn shadow(mut self, shadow: bool) -> Self {
        self.shadow = shadow;
        self
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum ButtonVariant {
    #[default]
    Default,
    Primary,
    Secondary,
    Danger,
    Info,
    Success,
    Warning,
    Ghost,
    Link,
    Text,
    Custom(ButtonCustomVariant),
}

impl ButtonVariant {
    #[inline]
    pub fn is_link(&self) -> bool {
        matches!(self, Self::Link)
    }

    #[inline]
    pub fn is_text(&self) -> bool {
        matches!(self, Self::Text)
    }

    #[inline]
    pub fn is_ghost(&self) -> bool {
        matches!(self, Self::Ghost)
    }

    #[inline]
    fn no_padding(&self) -> bool {
        self.is_link() || self.is_text()
    }

    #[inline]
    fn is_default(&self) -> bool {
        matches!(self, Self::Default)
    }
}

#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    base: gpui_base::Button,
    icon: Option<ButtonIcon>,
    label: Option<SharedString>,

    accessibility_label: Option<SharedString>,
    children: Vec<AnyElement>,
    disabled: bool,
    pub(crate) selected: bool,

    open: bool,
    toggled: Option<bool>,
    role: RoleOverride,
    variant: ButtonVariant,
    rounded: ButtonRounded,
    outline: bool,
    border_corners: Corners<bool>,
    border_edges: Edges<bool>,
    dropdown_caret: bool,
    hover_group: Option<SharedString>,
    hover_group_held: bool,
    size: Size,
    content_style: StyleRefinement,
    icon_size: Option<Size>,
    compact: bool,
    tooltip: Option<(
        SharedString,
        Option<(Rc<Box<dyn gpui::Action>>, Option<SharedString>)>,
    )>,
    tooltip_placement: Option<Placement>,
    tooltip_show_delay: Option<Duration>,
    tooltip_builder: Option<Rc<dyn Fn(&mut Window, &mut App) -> gpui::AnyView>>,
    on_click: Option<Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>>,
    on_hover: Option<Rc<dyn Fn(&bool, &mut Window, &mut App)>>,
    loading: bool,
    loading_icon: Option<Icon>,
    focus_ring_enabled: bool,

    tab_index: isize,
    tab_stop: bool,
}

impl From<Button> for AnyElement {
    fn from(button: Button) -> Self {
        button.into_any_element()
    }
}

impl Button {
    pub fn new(id: impl Into<ElementId>) -> Self {
        let id = id.into();

        Self {
            id: id.clone(),
            base: gpui_base::Button::new(id),
            icon: None,
            label: None,
            accessibility_label: None,
            children: Vec::new(),
            disabled: false,
            selected: false,
            open: false,
            toggled: None,
            role: RoleOverride::default(),
            variant: ButtonVariant::default(),
            rounded: ButtonRounded::Medium,
            border_corners: Corners {
                top_left: true,
                top_right: true,
                bottom_right: true,
                bottom_left: true,
            },
            border_edges: Edges::all(true),
            size: Size::Medium,
            content_style: StyleRefinement::default(),
            icon_size: None,
            tooltip: None,
            tooltip_placement: None,
            tooltip_show_delay: None,
            tooltip_builder: None,
            on_click: None,
            focus_ring_enabled: true,
            on_hover: None,
            loading: false,
            compact: false,
            outline: false,
            loading_icon: None,
            dropdown_caret: false,
            hover_group: None,
            hover_group_held: false,
            tab_index: 0,
            tab_stop: true,
        }
    }

    pub(crate) fn variant(&self) -> ButtonVariant {
        self.variant
    }

    #[cfg(test)]
    pub(crate) fn is_compact(&self) -> bool {
        self.compact
    }

    pub(crate) fn content_style(mut self, style: StyleRefinement, icon_size: Size) -> Self {
        self.content_style = style;
        self.icon_size = Some(icon_size);
        self
    }

    pub(crate) fn is_disabled(&self) -> bool {
        self.disabled
    }

    pub(crate) fn is_outline(&self) -> bool {
        self.outline
    }

    pub(crate) fn is_icon_only(&self) -> bool {
        self.icon.is_some() && self.label.is_none() && self.children.is_empty()
    }

    pub fn role(mut self, role: impl Into<RoleOverride>) -> Self {
        self.role = role.into();
        self
    }

    pub fn outline(mut self) -> Self {
        self.outline = true;
        self
    }

    pub fn rounded(mut self, rounded: impl Into<ButtonRounded>) -> Self {
        self.rounded = rounded.into();
        self
    }

    pub(crate) fn border_corners(mut self, corners: impl Into<Corners<bool>>) -> Self {
        self.border_corners = corners.into();
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn accessibility_id(mut self, id: impl Into<SharedString>) -> Self {
        self.base = self.base.accessibility_id(id);
        self
    }

    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        self.accessibility_label = Some(label.into());
        self
    }

    pub fn icon(mut self, icon: impl Into<ButtonIcon>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some((tooltip.into(), None));
        self
    }

    pub fn tooltip_with(
        mut self,
        builder: impl Fn(&mut Window, &mut App) -> gpui::AnyView + 'static,
    ) -> Self {
        self.tooltip_builder = Some(Rc::new(builder));
        self
    }

    pub fn tooltip_placement(mut self, placement: Placement) -> Self {
        self.tooltip_placement = Some(placement);
        self
    }

    pub fn tooltip_show_delay(mut self, delay: Duration) -> Self {
        self.tooltip_show_delay = Some(delay);
        self
    }

    pub fn tooltip_with_action(
        mut self,
        tooltip: impl Into<SharedString>,
        action: &dyn gpui::Action,
        context: Option<&str>,
    ) -> Self {
        self.tooltip = Some((
            tooltip.into(),
            Some((
                Rc::new(action.boxed_clone()),
                context.map(|c| c.to_string().into()),
            )),
        ));
        self
    }

    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    pub fn compact(mut self) -> Self {
        self.compact = true;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }

    pub fn on_hover(mut self, handler: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_hover = Some(Rc::new(handler));
        self
    }

    pub fn loading_icon(mut self, icon: impl Into<Icon>) -> Self {
        self.loading_icon = Some(icon.into());
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

    pub fn dropdown_caret(mut self, dropdown_caret: bool) -> Self {
        self.dropdown_caret = dropdown_caret;
        self
    }

    pub fn toggled(mut self, toggled: bool) -> Self {
        self.toggled = Some(toggled);
        self
    }

    #[inline]
    fn shows_selected_style(&self) -> bool {
        self.selected || self.open
    }

    #[inline]
    fn interactive(&self) -> bool {
        !(self.disabled || self.loading)
    }

    #[inline]
    fn hoverable(&self) -> bool {
        self.interactive() && self.on_hover.is_some()
    }
}

impl Disableable for Button {
    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl crate::FocusableExt for Button {
    fn focus_ring(mut self, enabled: bool) -> Self {
        self.focus_ring_enabled = enabled;
        self
    }

    fn is_focus_ring_enabled(&self) -> bool {
        self.focus_ring_enabled
    }
}

impl Selectable for Button {
    fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    fn is_selected(&self) -> bool {
        self.selected
    }

    fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    fn is_open(&self) -> bool {
        self.open
    }
}

impl Sizable for Button {
    fn with_size(mut self, size: impl Into<Size>) -> Self {
        self.size = size.into();
        self
    }

    fn prepare_for_toolbar(self) -> Self {
        self.ghost().compact()
    }
}

impl ButtonVariants for Button {
    fn with_variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }
}

impl Styled for Button {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl ParentElement for Button {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements)
    }
}

impl InteractiveElement for Button {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl RenderOnce for Button {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let style: ButtonVariant = self.variant;
        let interactive = self.interactive();
        let hoverable = self.hoverable();
        let disabled = self.disabled;
        let selected = self.shows_selected_style();
        let loading = self.loading;
        let tooltip_placement = self.tooltip_placement;
        let tooltip_show_delay = self.tooltip_show_delay;
        let hover_group = self.hover_group;
        let hover_group_held = self.hover_group_held;
        let mut base = self.base;
        let children = self.children;
        let instance_style = base.style().clone();
        let normal_style = style.normal(self.outline, cx);
        let selected_style = style.selected(self.outline, cx);
        let disabled_style = style.disabled(self.outline, cx);
        let icon_size = self.icon_size.unwrap_or_else(|| match self.size {
            Size::Size(v) => Size::Size(v * 0.75),
            _ => self.size,
        });
        let has_content = self.icon.is_some() || self.label.is_some() || !children.is_empty();
        // Only borderless variants use this; the others tint their border.
        let focus_line = if style.no_padding() {
            FocusLine::Outside
        } else if style.is_ghost() {
            FocusLine::Edge
        } else {
            FocusLine::Inside(normal_style.fg.opacity(FOCUS_LINE_OPACITY))
        };

        let focus_handle = window
            .use_keyed_state(self.id.clone(), cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        let is_focused = focus_handle.is_focused(window);

        let rounding = match self.rounded {
            ButtonRounded::Small => cx.theme().radius * 0.5,
            ButtonRounded::Medium => cx.theme().radius,
            ButtonRounded::Large => cx.theme().radius * 2.0,
            ButtonRounded::Size(px) => px,
            ButtonRounded::None => Pixels::ZERO,
        };

        let root = base
            .cursor_default()
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .cursor_default()
            .when(
                interactive && (self.variant.is_link() || self.variant.is_text()),
                |this| this.cursor_pointer(),
            )
            .when(
                !disabled && cx.theme().shadow && normal_style.shadow,
                |this| this.shadow_xs(),
            )
            .when(!style.no_padding(), |this| {
                if self.label.is_none() && children.is_empty() {
                    // Icon Button
                    match self.size {
                        Size::Size(px) => this.size(px),
                        Size::XSmall => this.size_5(),
                        Size::Small => this.size_6(),
                        Size::Large | Size::Medium => this.size_8(),
                    }
                } else {
                    // Normal Button
                    match self.size {
                        Size::Size(size) => this.px(size * 0.2),
                        Size::XSmall => this.h_5().px_1().when(self.compact, |this| this.min_w_5()),
                        Size::Small => this
                            .h_6()
                            .px_2()
                            .when(self.compact, |this| this.min_w_6().px_1p5()),
                        Size::Medium => this
                            .h_8()
                            .px_2p5()
                            .when(self.compact, |this| this.min_w_8().px_2()),
                        Size::Large => this
                            .h_8()
                            .px_3()
                            .when(self.compact, |this| this.min_w_8().px_2()),
                    }
                }
            })
            .when(self.border_corners.top_left, |this| {
                this.rounded_tl(rounding)
            })
            .when(self.border_corners.top_right, |this| {
                this.rounded_tr(rounding)
            })
            .when(self.border_corners.bottom_left, |this| {
                this.rounded_bl(rounding)
            })
            .when(self.border_corners.bottom_right, |this| {
                this.rounded_br(rounding)
            })
            .when(self.variant.is_default() || self.outline, |this| {
                this.when(self.border_edges.left, |this| this.border_l_1())
                    .when(self.border_edges.right, |this| this.border_r_1())
                    .when(self.border_edges.top, |this| this.border_t_1())
                    .when(self.border_edges.bottom, |this| this.border_b_1())
            })
            .when(!self.disabled && !selected, |this| {
                this.border_color(normal_style.border)
                    .bg(normal_style.bg)
                    .text_color(normal_style.fg)
                    .when(normal_style.underline, |this| this.text_decoration_1())
                    // A loading button keeps its normal colors, but must not react
                    // to the pointer, it is not waiting for another click.
                    .when(interactive, |this| {
                        this.hover(|this| {
                            let hover_style = style.hovered(self.outline, cx);
                            this.bg(hover_style.bg)
                                .border_color(hover_style.border)
                                .text_color(hover_style.fg)
                        })
                        .active(|this| {
                            let active_style = style.active(self.outline, cx);
                            this.bg(active_style.bg)
                                .border_color(active_style.border)
                                .text_color(active_style.fg)
                        })
                        .when_some(hover_group, |this, group| {
                            let idle_bg = style.hovered(self.outline, cx).bg.opacity(0.5);
                            this.when(hover_group_held, |this| this.bg(idle_bg))
                                .group_hover(group, |this| this.bg(idle_bg))
                        })
                    })
            })
            .line_height(relative(1.25))
            .refine_style(&instance_style);

        // The explicit name wins: it exists precisely for the cases where the
        // visible content is not what a listener needs to hear.
        let accessibility_label = self
            .accessibility_label
            .clone()
            .or_else(|| self.label.clone());
        let content = h_flex()
            .id("label")
            .size_full()
            .min_w_0()
            .overflow_hidden()
            .whitespace_nowrap()
            .items_center()
            .justify_center()
            .button_text_size(self.size)
            .map(|this| match self.size {
                Size::XSmall => this.gap_1(),
                Size::Small => this.gap_1(),
                _ => this.gap_2(),
            })
            .refine_style(&self.content_style)
            .when_some(self.icon, |this, icon| {
                this.child(
                    icon.loading_icon(self.loading_icon)
                        .loading(self.loading)
                        .with_size(icon_size),
                )
            })
            .when_some(self.label, |this, label| {
                this.child(
                    div()
                        .min_w_0()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(label),
                )
            })
            .children(children)
            .when(self.dropdown_caret, |this| {
                this.when(has_content, |this| this.justify_between())
                    .child(Caret::new(self.size).text_color(normal_style.fg.opacity(0.75)))
            });
        root.role(self.role.resolve(|| {
            if self.variant.is_link() {
                Role::Link
            } else {
                Role::Button
            }
        }))
        .selected(selected)
        .disabled(disabled)
        // Base layers semantic states over the builder chain, so the caller's
        // own style is replayed inside each state to keep it the closest layer.
        .styles(|styles| {
            styles
                .selected(|style| {
                    style
                        .bg(selected_style.bg)
                        .border_color(selected_style.border)
                        .text_color(selected_style.fg)
                        .refine_style(&instance_style)
                })
                .disabled(|style| {
                    style
                        .bg(disabled_style.bg)
                        .text_color(disabled_style.fg)
                        .border_color(disabled_style.border)
                        .shadow_none()
                        .refine_style(&instance_style)
                })
        })
        .when_some(accessibility_label, |this, label| {
            this.accessibility_label(label)
        })
        .when_some(self.toggled, |this, toggled| {
            this.aria_toggled(if toggled {
                gpui::accesskit::Toggled::True
            } else {
                gpui::accesskit::Toggled::False
            })
        })
        .track_focus(&focus_handle)
        .tab_index(self.tab_index)
        .tab_stop(self.tab_stop)
        .child(content)
        // Fade the whole button while loading, so every variant is dimmed by
        // the same amount. Fading `bg`, `border` and `fg` one by one instead
        // only shows up on variants that have a background to begin with:
        // `Ghost`, `Link` and `Text` are transparent, so an alpha on their
        // background changes nothing.
        .when(loading && !disabled, |this| this.opacity(0.8))
        .when(!disabled, |this| {
            this.on_mouse_down(MouseButton::Left, move |_, window, cx| {
                if loading {
                    cx.stop_propagation();
                    return;
                }

                // Avoid focus on mouse down.
                window.prevent_default();

                // Pressing a button must not start the window-level text selection.
                crate::global_state::GlobalState::suppress_text_selection(cx);
            })
        })
        .when_some(self.on_click, |this, on_click| {
            this.on_click(move |event, window, cx| {
                if loading {
                    cx.stop_propagation();
                    return;
                }

                on_click(event, window, cx);
            })
        })
        .when(loading, |this| {
            this.on_click(|_, _, cx| cx.stop_propagation())
        })
        .when_some(self.on_hover.filter(|_| hoverable), |this, on_hover| {
            this.on_hover(move |hovered, window, cx| {
                on_hover(hovered, window, cx);
            })
        })
        .map(|this| {
            if let Some(builder) = self.tooltip_builder {
                this.managed_tooltip_with(
                    tooltip_placement,
                    tooltip_show_delay,
                    move |window, cx| builder(window, cx),
                )
            } else if let Some((tooltip, action)) = self.tooltip {
                this.managed_tooltip_with(
                    tooltip_placement,
                    tooltip_show_delay,
                    move |window, cx| {
                        Tooltip::new(tooltip.clone())
                            .when_some(action.clone(), |this, (action, context)| {
                                this.action(
                                    action.boxed_clone().as_ref(),
                                    context.as_ref().map(|c| c.as_ref()),
                                )
                            })
                            .build(window, cx)
                    },
                )
            } else {
                this
            }
        })
        .when(is_focused && self.focus_ring_enabled, |this| {
            focus_style(this, focus_line, window, cx)
        })
    }
}

struct ButtonVariantStyle {
    bg: Background,
    border: Hsla,
    fg: Hsla,
    underline: bool,
    shadow: bool,
}

#[derive(Clone, Copy)]
enum ButtonStyleState {
    Normal,
    Hovered,
    Active,
}

impl ButtonVariant {
    fn outline_background(&self, state: ButtonStyleState, cx: &mut App) -> Background {
        match (self, state) {
            (Self::Default, ButtonStyleState::Normal) => cx.theme().input_background().into(),
            (Self::Default, ButtonStyleState::Hovered) => cx
                .theme()
                .input
                .mix_oklab(cx.theme().transparent, 0.5)
                .into(),
            (Self::Default, ButtonStyleState::Active) => cx
                .theme()
                .input
                .mix_oklab(cx.theme().transparent, 0.7)
                .into(),
            (Self::Primary, ButtonStyleState::Normal) => {
                cx.theme().tokens.primary.background.opacity(0.1)
            }
            (Self::Primary, ButtonStyleState::Hovered) => {
                cx.theme().tokens.primary_hover.background.opacity(0.2)
            }
            (Self::Primary, ButtonStyleState::Active) => {
                cx.theme().tokens.primary_active.background.opacity(0.4)
            }
            (Self::Secondary, ButtonStyleState::Normal) => {
                cx.theme().tokens.secondary.background.opacity(0.1)
            }
            (Self::Secondary, ButtonStyleState::Hovered) => {
                cx.theme().tokens.secondary_hover.background.opacity(0.2)
            }
            (Self::Secondary, ButtonStyleState::Active) => {
                cx.theme().tokens.secondary_active.background.opacity(0.4)
            }
            (Self::Danger, ButtonStyleState::Normal) => {
                cx.theme().tokens.danger.background.opacity(0.1)
            }
            (Self::Danger, ButtonStyleState::Hovered) => {
                cx.theme().tokens.danger_hover.background.opacity(0.2)
            }
            (Self::Danger, ButtonStyleState::Active) => {
                cx.theme().tokens.danger_active.background.opacity(0.4)
            }
            (Self::Warning, ButtonStyleState::Normal) => {
                cx.theme().tokens.warning.background.opacity(0.1)
            }
            (Self::Warning, ButtonStyleState::Hovered) => {
                cx.theme().tokens.warning_hover.background.opacity(0.2)
            }
            (Self::Warning, ButtonStyleState::Active) => {
                cx.theme().tokens.warning_active.background.opacity(0.4)
            }
            (Self::Success, ButtonStyleState::Normal) => {
                cx.theme().tokens.success.background.opacity(0.1)
            }
            (Self::Success, ButtonStyleState::Hovered) => {
                cx.theme().tokens.success_hover.background.opacity(0.2)
            }
            (Self::Success, ButtonStyleState::Active) => {
                cx.theme().tokens.success_active.background.opacity(0.4)
            }
            (Self::Info, ButtonStyleState::Normal) => {
                cx.theme().tokens.info.background.opacity(0.1)
            }
            (Self::Info, ButtonStyleState::Hovered) => {
                cx.theme().tokens.info_hover.background.opacity(0.2)
            }
            (Self::Info, ButtonStyleState::Active) => {
                cx.theme().tokens.info_active.background.opacity(0.4)
            }
            (Self::Ghost | Self::Link | Self::Text, _) => cx.theme().transparent.into(),
            (Self::Custom(colors), _) => colors.color.mix_oklab(cx.theme().transparent, 0.2).into(),
        }
    }

    fn bg_color(&self, outline: bool, cx: &mut App) -> Background {
        if outline {
            return self.outline_background(ButtonStyleState::Normal, cx);
        }

        match self {
            Self::Default => cx.theme().tokens.button.into(),
            Self::Primary => cx.theme().tokens.button_primary.into(),
            Self::Secondary => cx.theme().tokens.button_secondary.into(),
            Self::Danger => cx.theme().tokens.button_danger.into(),
            Self::Warning => cx.theme().tokens.button_warning.into(),
            Self::Success => cx.theme().tokens.button_success.into(),
            Self::Info => cx.theme().tokens.button_info.into(),
            Self::Ghost | Self::Link | Self::Text => cx.theme().transparent.into(),
            Self::Custom(colors) => colors.color.mix_oklab(cx.theme().transparent, 0.2).into(),
        }
    }

    fn text_color(&self, outline: bool, cx: &mut App) -> Hsla {
        match self {
            Self::Default => cx.theme().button_foreground,
            Self::Primary => {
                if outline {
                    cx.theme().primary
                } else {
                    cx.theme().button_primary_foreground
                }
            }
            Self::Secondary => {
                if outline {
                    cx.theme().secondary_foreground
                } else {
                    cx.theme().button_secondary_foreground
                }
            }
            Self::Ghost => cx.theme().secondary_foreground,
            Self::Danger => {
                if outline {
                    cx.theme().danger
                } else {
                    cx.theme().button_danger_foreground
                }
            }
            Self::Warning => {
                if outline {
                    cx.theme().warning
                } else {
                    cx.theme().button_warning_foreground
                }
            }
            Self::Success => {
                if outline {
                    cx.theme().success
                } else {
                    cx.theme().button_success_foreground
                }
            }
            Self::Info => {
                if outline {
                    cx.theme().info
                } else {
                    cx.theme().button_info_foreground
                }
            }
            Self::Link => cx.theme().link,
            Self::Text => cx.theme().foreground.opacity(0.9),
            Self::Custom(colors) => colors.foreground,
        }
    }

    fn border_color(&self, outline: bool, cx: &mut App) -> Hsla {
        match self {
            Self::Default => cx.theme().input,
            Self::Secondary => cx.theme().border,
            Self::Primary => cx.theme().primary,
            Self::Danger => {
                if outline {
                    cx.theme().danger.mix_oklab(transparent_white(), 0.4)
                } else {
                    cx.theme().button_danger
                }
            }
            Self::Info => {
                if outline {
                    cx.theme().info.mix_oklab(transparent_white(), 0.4)
                } else {
                    cx.theme().button_info
                }
            }
            Self::Warning => {
                if outline {
                    cx.theme().warning.mix_oklab(transparent_white(), 0.4)
                } else {
                    cx.theme().button_warning
                }
            }
            Self::Success => {
                if outline {
                    cx.theme().success.mix_oklab(transparent_white(), 0.4)
                } else {
                    cx.theme().button_success
                }
            }
            Self::Ghost | Self::Link | Self::Text => cx.theme().transparent,
            Self::Custom(colors) => {
                if outline {
                    colors.color.mix_oklab(transparent_white(), 0.4)
                } else {
                    colors.color
                }
            }
        }
    }

    fn underline(&self, _: &App) -> bool {
        match self {
            Self::Link => true,
            _ => false,
        }
    }

    fn shadow(&self, _outline: bool, _: &App) -> bool {
        match self {
            Self::Custom(c) => c.shadow,
            _ => false,
        }
    }

    fn normal(&self, outline: bool, cx: &mut App) -> ButtonVariantStyle {
        let bg = self.bg_color(outline, cx);
        let border = self.border_color(outline, cx);
        let fg = self.text_color(outline, cx);
        let underline = self.underline(cx);
        let shadow = self.shadow(outline, cx);

        ButtonVariantStyle {
            bg,
            border,
            fg,
            underline,
            shadow,
        }
    }

    fn hovered(&self, outline: bool, cx: &mut App) -> ButtonVariantStyle {
        let bg: Background = match self {
            Self::Default => {
                if outline {
                    self.outline_background(ButtonStyleState::Hovered, cx)
                } else {
                    cx.theme().tokens.button_hover.into()
                }
            }
            Self::Primary => {
                if outline {
                    self.outline_background(ButtonStyleState::Hovered, cx)
                } else {
                    cx.theme().tokens.button_primary_hover.into()
                }
            }
            Self::Secondary => {
                if outline {
                    self.outline_background(ButtonStyleState::Hovered, cx)
                } else {
                    cx.theme().tokens.button_secondary_hover.into()
                }
            }
            Self::Danger => {
                if outline {
                    self.outline_background(ButtonStyleState::Hovered, cx)
                } else {
                    cx.theme().tokens.button_danger_hover.into()
                }
            }
            Self::Warning => {
                if outline {
                    self.outline_background(ButtonStyleState::Hovered, cx)
                } else {
                    cx.theme().tokens.button_warning_hover.into()
                }
            }
            Self::Success => {
                if outline {
                    self.outline_background(ButtonStyleState::Hovered, cx)
                } else {
                    cx.theme().tokens.button_success_hover.into()
                }
            }
            Self::Info => {
                if outline {
                    self.outline_background(ButtonStyleState::Hovered, cx)
                } else {
                    cx.theme().tokens.button_info_hover.into()
                }
            }
            Self::Custom(colors) => colors.hover.into(),
            Self::Ghost => {
                let accent: Background = cx.theme().tokens.accent.into();
                if cx.theme().mode.is_dark() {
                    accent.opacity(0.5)
                } else {
                    accent
                }
            }
            Self::Link => cx.theme().transparent.into(),
            Self::Text => cx.theme().transparent.into(),
        };

        let border = self.border_color(outline, cx);
        let fg = match self {
            Self::Link => cx.theme().link_hover,
            Self::Text => cx.theme().foreground,
            Self::Ghost => cx.theme().accent_foreground,
            _ => self.text_color(outline, cx),
        };

        let underline = self.underline(cx);
        let shadow = self.shadow(outline, cx);

        ButtonVariantStyle {
            bg,
            border,
            fg,
            underline,
            shadow,
        }
    }

    fn active(&self, outline: bool, cx: &mut App) -> ButtonVariantStyle {
        let bg = match self {
            Self::Default => {
                if outline {
                    self.outline_background(ButtonStyleState::Active, cx)
                } else {
                    cx.theme().tokens.button_active.into()
                }
            }
            Self::Primary => {
                if outline {
                    self.outline_background(ButtonStyleState::Active, cx)
                } else {
                    cx.theme().tokens.button_primary_active.into()
                }
            }
            Self::Secondary => {
                if outline {
                    self.outline_background(ButtonStyleState::Active, cx)
                } else {
                    cx.theme().tokens.button_secondary_active.into()
                }
            }
            Self::Ghost => cx.theme().tokens.button_active.into(),
            Self::Danger => {
                if outline {
                    self.outline_background(ButtonStyleState::Active, cx)
                } else {
                    cx.theme().tokens.button_danger_active.into()
                }
            }
            Self::Warning => {
                if outline {
                    self.outline_background(ButtonStyleState::Active, cx)
                } else {
                    cx.theme().tokens.button_warning_active.into()
                }
            }
            Self::Success => {
                if outline {
                    self.outline_background(ButtonStyleState::Active, cx)
                } else {
                    cx.theme().tokens.button_success_active.into()
                }
            }
            Self::Info => {
                if outline {
                    self.outline_background(ButtonStyleState::Active, cx)
                } else {
                    cx.theme().tokens.button_info_active.into()
                }
            }
            Self::Custom(colors) => colors.active.into(),
            Self::Link => cx.theme().transparent.into(),
            Self::Text => cx.theme().transparent.into(),
        };
        let border = self.border_color(outline, cx);
        let fg = match self {
            Self::Link => cx.theme().link_active,
            Self::Text => cx.theme().foreground.opacity(0.7),
            _ => self.text_color(outline, cx),
        };
        let underline = self.underline(cx);
        let shadow = self.shadow(outline, cx);

        ButtonVariantStyle {
            bg,
            border,
            fg,
            underline,
            shadow,
        }
    }

    fn selected(&self, outline: bool, cx: &mut App) -> ButtonVariantStyle {
        if outline {
            let active_style = self.active(outline, cx);

            return ButtonVariantStyle {
                fg: self.text_color(outline, cx),
                ..active_style
            };
        }

        let bg = match self {
            Self::Default => cx.theme().tokens.button_active.into(),
            Self::Primary => cx.theme().tokens.button_primary_active.into(),
            Self::Secondary => cx.theme().tokens.button_secondary_active.into(),
            Self::Ghost => cx.theme().tokens.secondary_active.into(),
            Self::Danger => cx.theme().tokens.button_danger_active.into(),
            Self::Warning => cx.theme().tokens.button_warning_active.into(),
            Self::Success => cx.theme().tokens.button_success_active.into(),
            Self::Info => cx.theme().tokens.button_info_active.into(),
            Self::Link => cx.theme().transparent.into(),
            Self::Text => cx.theme().transparent.into(),
            Self::Custom(colors) => colors.active.into(),
        };

        let border = self.border_color(outline, cx);
        let fg = match self {
            Self::Link => cx.theme().link_active,
            Self::Text => cx.theme().foreground.opacity(0.7),
            _ => self.text_color(false, cx),
        };
        let underline = self.underline(cx);
        let shadow = self.shadow(outline, cx);

        ButtonVariantStyle {
            bg,
            border,
            fg,
            underline,
            shadow,
        }
    }

    fn disabled(&self, outline: bool, cx: &mut App) -> ButtonVariantStyle {
        let bg = match self {
            Self::Default | Self::Link | Self::Ghost | Self::Text => cx.theme().transparent.into(),
            Self::Primary => cx.theme().tokens.button_primary.background.opacity(0.15),
            Self::Danger => cx.theme().tokens.button_danger.background.opacity(0.15),
            Self::Warning => cx.theme().tokens.button_warning.background.opacity(0.15),
            Self::Success => cx.theme().tokens.button_success.background.opacity(0.15),
            Self::Info => cx.theme().tokens.button_info.background.opacity(0.15),
            Self::Secondary => cx.theme().tokens.button_secondary.background.opacity(1.5),
            Self::Custom(style) => style.color.opacity(0.15).into(),
        };
        let fg = cx.theme().muted_foreground.opacity(0.5);
        let (bg, border) = if outline {
            (
                self.outline_background(ButtonStyleState::Normal, cx)
                    .opacity(0.5),
                self.border_color(true, cx).opacity(0.5),
            )
        } else if let Self::Default = self {
            (
                cx.theme().input_background().opacity(0.5).into(),
                cx.theme().input.opacity(0.5),
            )
        } else {
            let border = match self {
                Self::Primary => cx.theme().button_primary.opacity(0.15),
                Self::Secondary => cx.theme().button_secondary.opacity(1.5),
                Self::Danger => cx.theme().button_danger.opacity(0.15),
                Self::Warning => cx.theme().button_warning.opacity(0.15),
                Self::Success => cx.theme().button_success.opacity(0.15),
                Self::Info => cx.theme().button_info.opacity(0.15),
                Self::Custom(style) => style.color.opacity(0.15),
                Self::Default | Self::Link | Self::Ghost | Self::Text => cx.theme().transparent,
            };
            (bg, border)
        };

        let underline = self.underline(cx);
        let shadow = false;

        ButtonVariantStyle {
            bg,
            border,
            fg,
            underline,
            shadow,
        }
    }
}
