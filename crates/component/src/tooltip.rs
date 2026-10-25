use crate::root::WindowState;
use std::{cell::Cell, rc::Rc, time::Duration};

use gpui::{
    Action, AnyElement, AnyView, App, AppContext, Bounds, Context, ElementId, FontWeight,
    IntoElement, MouseButton, ParentElement, Pixels, Render, SharedString,
    StatefulInteractiveElement, StyleRefinement, Styled, Window, div, prelude::FluentBuilder, px,
};
pub use gpui_base::TooltipDefaults;
use gpui_base::{
    Tooltip as BaseTooltip, TooltipOverlay as BaseTooltipOverlay,
    TooltipRequest as BaseTooltipRequest, TooltipTransition as BaseTooltipTransition,
};

use crate::{
    ActiveTheme, Placement, StyledExt,
    animation::{EffectTransition, ease_in_out_cubic, ease_out_cubic},
    kbd::Kbd,
    text::Text,
};

pub(crate) fn init(_cx: &mut App) {
    // No app-level init needed — TooltipOverlay is per-window via Root.
}

// ── Tooltip view (unchanged API) ────────────────────────────────────────────

pub(crate) fn tooltip_text_size() -> gpui::Rems {
    gpui::rems(0.6875)
}

enum TooltipContext {
    Text(Text),
    Element(Box<dyn Fn(&mut Window, &mut App) -> AnyElement>),
}

pub struct Tooltip {
    style: StyleRefinement,
    content: TooltipContext,
    key_binding: Option<Kbd>,
    action: Option<(Box<dyn Action>, Option<SharedString>)>,
}

impl Tooltip {
    pub fn new(text: impl Into<Text>) -> Self {
        Self {
            style: StyleRefinement::default(),
            content: TooltipContext::Text(text.into()),
            key_binding: None,
            action: None,
        }
    }

    pub fn element<E, F>(builder: F) -> Self
    where
        E: IntoElement,
        F: Fn(&mut Window, &mut App) -> E + 'static,
    {
        Self {
            style: StyleRefinement::default(),
            key_binding: None,
            action: None,
            content: TooltipContext::Element(Box::new(move |window, cx| {
                builder(window, cx).into_any_element()
            })),
        }
    }

    pub fn action(mut self, action: &dyn Action, context: Option<&str>) -> Self {
        self.action = Some((action.boxed_clone(), context.map(SharedString::new)));
        self
    }

    pub fn key_binding(mut self, key_binding: Option<Kbd>) -> Self {
        self.key_binding = key_binding;
        self
    }

    pub fn build(self, _: &mut Window, cx: &mut App) -> AnyView {
        cx.new(|_| self).into()
    }
}

impl FluentBuilder for Tooltip {}
impl Styled for Tooltip {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}
impl Render for Tooltip {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let key_binding = if let Some(key_binding) = &self.key_binding {
            Some(
                key_binding
                    .clone()
                    .appearance(true)
                    .text_size(gpui::rems(0.53125))
                    .into_any_element(),
            )
        } else {
            self.action.as_ref().and_then(|(action, context)| {
                Kbd::binding_for_action(action.as_ref(), context.as_deref(), window)
                    .map(|keys| keys.text_size(gpui::rems(0.53125)).into_any_element())
            })
        };

        div().child(
            // Wrap in a child, to ensure the left margin is applied to the tooltip
            BaseTooltip::new("tooltip-popup")
                .h_flex()
                .font_family(cx.theme().font_family.clone())
                .m_3()
                .bg(cx.theme().tokens.popover)
                .text_color(cx.theme().popover_foreground)
                .bg(cx.theme().tokens.popover)
                .border_1()
                .border_color(cx.theme().popover_foreground.opacity(0.5))
                .shadow_md()
                .rounded(px(4.))
                .font_weight(FontWeight::NORMAL)
                .justify_between()
                .py_0p5()
                .px_2()
                .text_size(tooltip_text_size())
                .gap_2()
                .refine_style(&self.style)
                .map(|this| {
                    this.child(div().map(|this| match self.content {
                        TooltipContext::Text(ref text) => this.child(text.clone()),
                        TooltipContext::Element(ref builder) => this.child(builder(window, cx)),
                    }))
                })
                .when_some(key_binding, |this, keys| {
                    this.child(
                        div()
                            .text_size(gpui::rems(0.53125))
                            .font_weight(FontWeight::NORMAL)
                            .flex_shrink_0()
                            .text_color(cx.theme().popover_foreground)
                            .child(keys),
                    )
                }),
        )
    }
}

// ── Managed tooltip system ──────────────────────────────────────────────────

const ENTER_DURATION: Duration = Duration::from_millis(150);

const SLIDE_DURATION: Duration = Duration::from_millis(200);
pub(crate) fn render_tooltip(
    content_view: AnyView,
    transition: BaseTooltipTransition,
    _: &mut Window,
    _: &mut App,
) -> AnyElement {
    div().child(content_view).map(|element| match transition {
        BaseTooltipTransition::Switch {
            epoch,
            previous,
            current,
        } => {
            let same_row = (current.origin.y - previous.origin.y).abs() < px(10.);
            if !same_row {
                return element.into_any_element();
            }
            let dx = current.center().x - previous.center().x;
            EffectTransition::new(SLIDE_DURATION)
                .ease(ease_in_out_cubic)
                .slide_x(-dx, px(0.))
                .apply(
                    element,
                    ElementId::NamedInteger("tooltip-slide".into(), epoch as u64),
                )
                .into_any_element()
        }
        BaseTooltipTransition::Enter { epoch } => EffectTransition::new(ENTER_DURATION)
            .ease(ease_out_cubic)
            .slide_y(px(4.), px(0.))
            .fade(0.0, 1.0)
            .apply(
                element,
                ElementId::NamedInteger("tooltip-enter".into(), epoch as u64),
            )
            .into_any_element(),
    })
}

// ── Extension trait for managed tooltips ─────────────────────────────────────

// ── Shared tooltip state for components ─────────────────────────────────────

#[derive(Default)]
pub(crate) struct ComponentTooltip {
    pub text: Option<(
        SharedString,
        Option<(Rc<Box<dyn Action>>, Option<SharedString>)>,
    )>,
    pub builder: Option<Rc<dyn Fn(&mut Window, &mut App) -> AnyView>>,
    pub show_delay: Option<Duration>,
}

impl ComponentTooltip {
    pub fn apply<E: ManagedTooltipExt>(self, el: E) -> E {
        let show_delay = self.show_delay;
        if let Some(builder) = self.builder {
            el.managed_tooltip_with(None, show_delay, move |window, cx| builder(window, cx))
        } else if let Some((text, action)) = self.text {
            el.managed_tooltip_with(None, show_delay, move |window, cx| {
                Tooltip::new(text.clone())
                    .when_some(action.clone(), |this, (action, context)| {
                        this.action(
                            action.boxed_clone().as_ref(),
                            context.as_ref().map(|c| c.as_ref()),
                        )
                    })
                    .build(window, cx)
            })
        } else {
            el
        }
    }
}

// ── Internal managed tooltip trait ──────────────────────────────────────────

pub(crate) trait ManagedTooltipExt:
    StatefulInteractiveElement + crate::ElementExt + Sized
{
    fn managed_tooltip_with(
        self,
        preferred_placement: Option<Placement>,
        show_delay: Option<Duration>,
        build_tooltip: impl Fn(&mut Window, &mut App) -> AnyView + 'static,
    ) -> Self {
        let build_tooltip = Rc::new(build_tooltip);
        let trigger_bounds_cell: Rc<Cell<Bounds<Pixels>>> = Rc::new(Cell::new(Bounds::default()));
        let bounds_writer = trigger_bounds_cell.clone();

        self.on_prepaint(move |bounds, _, _| {
            bounds_writer.set(bounds);
        })
        .on_hover({
            let trigger_bounds_cell = trigger_bounds_cell.clone();
            let build_tooltip = build_tooltip.clone();
            move |hovered, window, cx| {
                if let Some(overlay) = WindowState::tooltip_overlay(window, cx) {
                    if *hovered {
                        let bounds = trigger_bounds_cell.get();
                        overlay.update(cx, |o: &mut BaseTooltipOverlay, cx| {
                            let build = build_tooltip.clone();
                            let request = BaseTooltipRequest::new(bounds, move |window, cx| {
                                build(window, cx)
                            });
                            let request = match preferred_placement {
                                Some(placement) => request.with_placement(placement),
                                None => request,
                            };
                            let request = match show_delay {
                                Some(delay) => request.with_show_delay(delay),
                                None => request,
                            };
                            o.request_show(request, window, cx);
                        });
                    } else {
                        overlay.update(cx, |o: &mut BaseTooltipOverlay, cx| {
                            o.request_hide(window, cx);
                        });
                    }
                }
            }
        })
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            if let Some(overlay) = WindowState::tooltip_overlay(window, cx) {
                overlay.update(cx, |overlay, cx| {
                    overlay.hide(cx);
                });
            }
        })
    }
}

impl<E: StatefulInteractiveElement + crate::ElementExt> ManagedTooltipExt for E {}
