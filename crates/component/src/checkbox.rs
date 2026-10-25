use std::{rc::Rc, time::Duration};

use crate::{
    ActiveTheme, Disableable, RoleOverride, Selectable, Sizable, Size, text::Text,
    tooltip::ComponentTooltip, v_flex,
};
use crate::{StyleSized as _, StyledExt as _, ThemeStyled as _};
use gpui::{
    AnyElement, App, ElementId, InteractiveElement, IntoElement, MouseButton, ParentElement,
    RenderOnce, SharedString, StatefulInteractiveElement, StyleRefinement, Styled, Window, div,
    prelude::FluentBuilder as _, px, relative, rems, svg,
};
use gpui_base::{CheckboxIndicator, spring};

#[derive(IntoElement)]
pub struct Checkbox {
    id: ElementId,
    base: gpui_base::Checkbox,
    style: StyleRefinement,
    // `Text` is legacy presentation state. During render it is composed into
    // the Base Checkbox child seam together with application children.
    label: Option<Text>,
    accessibility_label: Option<SharedString>,
    children: Vec<AnyElement>,
    checked: bool,
    disabled: bool,
    size: Size,
    tab_stop: bool,
    tab_index: isize,
    on_click: Option<Rc<dyn Fn(&bool, &mut Window, &mut App) + 'static>>,
    tooltip: ComponentTooltip,
    role: RoleOverride,
    focus_ring_enabled: bool,
    indicator_color: Option<gpui::Hsla>,
}

impl Checkbox {
    pub fn new(id: impl Into<ElementId>) -> Self {
        let id = id.into();
        Self {
            id: id.clone(),
            base: gpui_base::Checkbox::new(id),
            style: StyleRefinement::default(),
            label: None,
            accessibility_label: None,
            children: Vec::new(),
            checked: false,
            disabled: false,
            size: Size::default(),
            on_click: None,
            tab_stop: true,
            tab_index: 0,
            tooltip: ComponentTooltip::default(),
            role: RoleOverride::default(),
            focus_ring_enabled: true,
            indicator_color: None,
        }
    }

    pub fn role(mut self, role: impl Into<RoleOverride>) -> Self {
        self.role = role.into();
        self
    }

    pub fn indicator_color(mut self, color: impl Into<gpui::Hsla>) -> Self {
        self.indicator_color = Some(color.into());
        self
    }

    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip.text = Some((tooltip.into(), None));
        self
    }

    pub fn tooltip_show_delay(mut self, delay: Duration) -> Self {
        self.tooltip.show_delay = Some(delay);
        self
    }

    pub fn label(mut self, label: impl Into<Text>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        self.accessibility_label = Some(label.into());
        self
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    pub fn on_click(self, handler: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_change(handler)
    }

    pub fn on_change(mut self, handler: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }

    pub fn tab_stop(mut self, tab_stop: bool) -> Self {
        self.tab_stop = tab_stop;
        self
    }

    pub fn tab_index(mut self, tab_index: isize) -> Self {
        self.tab_index = tab_index;
        self
    }
}

impl InteractiveElement for Checkbox {
    fn interactivity(&mut self) -> &mut gpui::Interactivity {
        self.base.interactivity()
    }
}
impl StatefulInteractiveElement for Checkbox {}

impl Styled for Checkbox {
    fn style(&mut self) -> &mut gpui::StyleRefinement {
        &mut self.style
    }
}

impl Disableable for Checkbox {
    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl crate::FocusableExt for Checkbox {
    fn focus_ring(mut self, enabled: bool) -> Self {
        self.focus_ring_enabled = enabled;
        self
    }

    fn is_focus_ring_enabled(&self) -> bool {
        self.focus_ring_enabled
    }
}

impl Selectable for Checkbox {
    fn selected(self, selected: bool) -> Self {
        self.checked(selected)
    }

    fn is_selected(&self) -> bool {
        self.checked
    }
}

impl ParentElement for Checkbox {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Sizable for Checkbox {
    fn with_size(mut self, size: impl Into<Size>) -> Self {
        self.size = size.into();
        self
    }
}

pub(crate) fn checkbox_check_icon(
    id: ElementId,
    size: Size,
    checked: bool,
    disabled: bool,
    window: &mut Window,
    cx: &mut App,
) -> gpui::Svg {
    // The mark keeps its path while the spring is still fading it out. Guarding
    // the path on `checked` alone unmounted the glyph the moment the box was
    // cleared, so only the fade-in was ever visible.
    let opacity = spring(
        (id, "mark"),
        if checked { 1. } else { 0. },
        cx.theme().motion_tokens().spring_control,
        window,
        cx,
    );
    let color = if disabled {
        cx.theme().primary_foreground.opacity(0.5)
    } else {
        cx.theme().primary_foreground
    };

    svg()
        .absolute()
        .top_px()
        .left_px()
        .map(|this| match size {
            Size::XSmall => this.size_2(),
            Size::Small => this.size_2p5(),
            Size::Medium => this.size_3(),
            Size::Large => this.size_3p5(),
            _ => this.size_3(),
        })
        .text_color(color)
        .when(opacity > 0., |this| {
            this.data(include_bytes!("../../../assets/icons/check.svg"))
                .opacity(opacity)
        })
}

impl RenderOnce for Checkbox {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let checked = self.checked;
        let has_content = self.label.is_some() || !self.children.is_empty();
        let indicator_size = rems(match self.size {
            Size::XSmall => 0.75,
            Size::Small => 0.875,
            Size::Large => 1.125,
            _ => 1.,
        });

        let base = self.base;
        let children = self.children;
        let accessibility_label = self
            .accessibility_label
            .or_else(|| self.label.as_ref().map(|label| label.get_text(cx)));
        let on_click = self.on_click.clone();
        let focus_handle = window
            .use_keyed_state(self.id.clone(), cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        let is_focused = focus_handle.is_focused(window);

        let indicator_color = self.indicator_color;
        let unchecked_border = indicator_color.unwrap_or(cx.theme().input);
        let checked_color = indicator_color.unwrap_or(cx.theme().primary);
        let checked_background = if indicator_color.is_some() {
            cx.theme().muted
        } else {
            cx.theme().primary
        };
        let disabled_indicator_color = if checked {
            checked_color.opacity(0.5)
        } else {
            unchecked_border.opacity(0.5)
        };
        let radius = cx.theme().radius.min(px(4.));
        let disabled_text_color = cx.theme().muted_foreground;
        let center_label = self.style.align_items == Some(gpui::AlignItems::Center);
        let instance_style = self.style.clone();
        base.role(self.role)
            .checked(checked)
            .disabled(self.disabled)
            .styles(|styles| {
                styles.disabled(|style| {
                    style
                        .text_color(disabled_text_color)
                        .refine_style(&instance_style)
                })
            })
            .tab_stop(self.tab_stop)
            .tab_index(self.tab_index)
            .track_focus(&focus_handle)
            .when_some(accessibility_label, |this, label| {
                this.accessibility_label(label)
            })
            .when_some(on_click, |this, on_click| {
                this.on_change(move |_, _, window, cx| {
                    window.prevent_default();
                    on_click(&!checked, window, cx);
                })
            })
            .h_flex()
            .gap_2()
            .items_start()
            .line_height(relative(1.))
            .text_color(cx.theme().foreground)
            .map(|this| match self.size {
                Size::Size(_) => this,
                size => this.input_text_size(size),
            })
            .rounded(cx.theme().radius * 0.5)
            .when(is_focused && self.focus_ring_enabled, |this| {
                this.focus_ring_style(window, cx)
            })
            .refine_style(&self.style)
            .child(
                CheckboxIndicator::new()
                    .checked(checked)
                    .disabled(self.disabled)
                    .relative()
                    .size(indicator_size)
                    // Center on the first 1.25em line, including when the label wraps.
                    .when(has_content && !center_label, |this| {
                        this.mt(indicator_size * 0.125)
                    })
                    .flex_shrink_0()
                    .border_1()
                    .rounded(radius)
                    .when(!checked, |this| {
                        this.bg(cx.theme().input_background())
                            .when(!self.disabled, |this| this.border_color(unchecked_border))
                    })
                    .styles(|styles| {
                        styles
                            .checked(|style| {
                                style.border_color(checked_color).bg(checked_background)
                            })
                            .disabled(|style| {
                                style
                                    .border_color(disabled_indicator_color)
                                    .when(checked, |style| style.bg(disabled_indicator_color))
                            })
                    })
                    .child(
                        checkbox_check_icon(self.id, self.size, checked, self.disabled, window, cx)
                            .when_some(indicator_color, |icon, color| {
                                icon.text_color(if self.disabled {
                                    color.opacity(0.5)
                                } else {
                                    color
                                })
                            }),
                    ),
            )
            .when(self.label.is_some() || !children.is_empty(), |this| {
                this.child(
                    v_flex()
                        .flex_1()
                        .overflow_hidden()
                        .line_height(relative(1.25))
                        .gap_1()
                        .map(|this| {
                            if let Some(label) = self.label {
                                this.child(
                                    div()
                                        .size_full()
                                        .text_color(cx.theme().foreground)
                                        .when(self.disabled, |this| {
                                            this.text_color(cx.theme().muted_foreground)
                                        })
                                        .child(label),
                                )
                            } else {
                                this
                            }
                        })
                        .children(children),
                )
            })
            .on_mouse_down(MouseButton::Left, |_, window, _| {
                // Preserve the legacy Checkbox behavior: pointer presses do
                // not move focus, including while disabled.
                window.prevent_default();
            })
            .map(|this| self.tooltip.apply(this))
    }
}
