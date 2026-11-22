//! Editor window presentation: input registration, text selection and tooltips.
use crate::context_menu::ContextMenuOverlay;
use crate::{
    ActiveTheme, input::AnyInputState, tooltip::render_tooltip,
    touch_selection::WindowTouchSelectionOverlay,
};
use gpui::{
    AnyElement, App, AppContext, Bounds, Context, DispatchPhase, Entity, GlobalElementId,
    InteractiveElement, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    ParentElement, Pixels, Render, Styled, Window, canvas, div,
};
use gpui_base::{TextSelection, TextSelectionScopeId};

pub(crate) fn init(cx: &mut App) {
    gpui_base::Root::register_plugin::<WindowState>(cx, WindowState::new);
}
#[derive(Default)]
struct ContextMenuClaim {
    armed: bool,
    opened: bool,
    input_gesture: bool,
}
pub(crate) struct WindowState {
    pub(super) focused_input: Option<AnyInputState>,
    tooltip_overlay: Entity<gpui_base::TooltipOverlay>,
    tooltip_hovered_trigger: Option<GlobalElementId>,
    tooltip_trigger_lifetime: std::rc::Weak<()>,
    tooltip_pressed_trigger: Option<Bounds<Pixels>>,
    context_menu_overlay: Entity<ContextMenuOverlay>,
    context_menu_claim: ContextMenuClaim,
    touch_selection_overlay: Entity<WindowTouchSelectionOverlay>,
}
impl WindowState {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            focused_input: None,
            tooltip_hovered_trigger: None,
            tooltip_trigger_lifetime: std::rc::Weak::new(),
            tooltip_pressed_trigger: None,
            context_menu_overlay: cx.new(|_| ContextMenuOverlay::new()),
            context_menu_claim: ContextMenuClaim::default(),
            tooltip_overlay: cx
                .new(|_| gpui_base::TooltipOverlay::new().render_with(render_tooltip)),
            touch_selection_overlay: cx.new(|cx| WindowTouchSelectionOverlay::new(window, cx)),
        }
    }
    fn entity(window: &Window, cx: &App) -> Option<Entity<Self>> {
        window.root::<gpui_base::Root>()??.read(cx).plugin::<Self>()
    }
    pub(crate) fn active_text_selection_scope(&self) -> TextSelectionScopeId {
        TextSelectionScopeId::default()
    }
    pub(crate) fn try_read<'a>(window: &Window, cx: &'a App) -> Option<&'a Self> {
        Self::entity(window, cx).map(|root| root.read(cx))
    }
    pub(crate) fn try_update<R>(
        window: &mut Window,
        cx: &mut App,
        f: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) -> R,
    ) -> Option<R> {
        Some(Self::entity(window, cx)?.update(cx, |state, cx| f(state, window, cx)))
    }
    pub(crate) fn read<'a>(window: &Window, cx: &'a App) -> &'a Self {
        Self::try_read(window, cx).expect("initialize editor components before opening the window")
    }
    pub(crate) fn tooltip_overlay(
        window: &Window,
        cx: &App,
    ) -> Option<Entity<gpui_base::TooltipOverlay>> {
        Some(Self::try_read(window, cx)?.tooltip_overlay.clone())
    }
    pub(crate) fn tooltip_overlay_for_trigger(
        trigger: &GlobalElementId,
        lifetime: std::rc::Weak<()>,
        hovered: bool,
        window: &Window,
        cx: &mut App,
    ) -> Option<Entity<gpui_base::TooltipOverlay>> {
        Self::entity(window, cx)?.update(cx, |state, _| {
            if hovered {
                state.tooltip_hovered_trigger = Some(trigger.clone());
                state.tooltip_trigger_lifetime = lifetime;
            } else if state.tooltip_hovered_trigger.as_ref() == Some(trigger) {
                state.tooltip_hovered_trigger = None;
                state.tooltip_trigger_lifetime = std::rc::Weak::new();
            } else {
                return None;
            }
            Some(state.tooltip_overlay.clone())
        })
    }

    fn dismiss_removed_tooltip(&self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(trigger) = self.tooltip_hovered_trigger.clone() else {
            return;
        };
        let state = cx.weak_entity();
        // Defer until GPUI has retained this frame's element states and dropped
        // absent ones. A reused cached subtree keeps its trigger alive.
        window.defer(cx, move |_, cx| {
            _ = state.update(cx, |state, cx| {
                if state.tooltip_hovered_trigger.as_ref() != Some(&trigger)
                    || state.tooltip_trigger_lifetime.upgrade().is_some()
                {
                    return;
                }
                state.tooltip_hovered_trigger = None;
                state.tooltip_trigger_lifetime = std::rc::Weak::new();
                state
                    .tooltip_overlay
                    .update(cx, |overlay, cx| overlay.hide(cx));
            });
        });
    }

    pub(crate) fn press_tooltip_trigger(bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        Self::try_update(window, cx, |state, _, cx| {
            state.tooltip_pressed_trigger = Some(bounds);
            cx.notify();
        });
    }

    pub(crate) fn is_tooltip_trigger_pressed(
        bounds: Bounds<Pixels>,
        window: &Window,
        cx: &App,
    ) -> bool {
        Self::try_read(window, cx)
            .is_some_and(|state| state.tooltip_pressed_trigger == Some(bounds))
    }

    fn tooltip_press_listener(&self, cx: &Context<Self>) -> Option<impl IntoElement + use<>> {
        let bounds = self.tooltip_pressed_trigger?;
        let state = cx.entity().downgrade();
        Some(canvas(
            |_, _, _| {},
            move |_, _, window, _| {
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                    if phase != DispatchPhase::Capture || bounds.contains(&event.position) {
                        return;
                    }
                    _ = state.update(cx, |state, cx| {
                        if state.tooltip_pressed_trigger == Some(bounds) {
                            state.tooltip_pressed_trigger = None;
                            cx.notify();
                        }
                    });
                });
            },
        ))
    }

    pub(crate) fn context_menu_overlay(
        window: &Window,
        cx: &App,
    ) -> Option<Entity<ContextMenuOverlay>> {
        Some(Self::try_read(window, cx)?.context_menu_overlay.clone())
    }

    pub(crate) fn arm_context_menu(window: &mut Window, cx: &mut App) {
        Self::try_update(window, cx, |state, _, _| {
            state.context_menu_claim.armed = true;
            state.context_menu_claim.input_gesture = true;
        });
    }

    pub(crate) fn take_input_context_menu_gesture(window: &mut Window, cx: &mut App) -> bool {
        Self::try_update(window, cx, |state, _, _| {
            std::mem::take(&mut state.context_menu_claim.input_gesture)
        })
        .unwrap_or(true)
    }

    pub(crate) fn mark_context_menu_opened(window: &mut Window, cx: &mut App) {
        Self::try_update(window, cx, |state, _, _| {
            state.context_menu_claim.opened = true;
        });
    }

    pub(crate) fn clear_context_menu_opened(window: &mut Window, cx: &mut App) {
        Self::try_update(window, cx, |state, _, _| {
            state.context_menu_claim.opened = false;
        });
    }

    fn reset_context_menu_claim(window: &mut Window, cx: &mut App) {
        Self::try_update(window, cx, |state, _, _| {
            state.context_menu_claim = ContextMenuClaim::default();
        });
    }

    fn disarm_context_menu_claim(window: &mut Window, cx: &mut App) {
        Self::try_update(window, cx, |state, _, _| {
            state.context_menu_claim.armed = false;
        });
    }

    pub(crate) fn consume_context_menu_claim(window: &mut Window, cx: &mut App) -> bool {
        Self::try_update(window, cx, |state, _, _| {
            // Preserve `input_gesture` for platforms where `contextmenu` fires
            // before `pointerup` so the release can still open the input menu.
            let armed = std::mem::take(&mut state.context_menu_claim.armed);
            let opened = std::mem::take(&mut state.context_menu_claim.opened);
            armed || opened
        })
        .unwrap_or(false)
    }
}
impl gpui_base::RootPlugin for WindowState {
    fn prepare(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dismiss_removed_tooltip(window, cx);
        window.set_rem_size(cx.theme().font_size);
        TextSelection::activate_scope(self.active_text_selection_scope(), window, cx);
    }
    fn style(&self, surface: &mut gpui::Stateful<gpui::Div>, _: &mut Window, cx: &mut App) {
        use gpui::Refineable as _;
        surface.style().refine(
            &gpui::StyleRefinement::default()
                .font_family(cx.theme().font_family.clone())
                .bg(cx.theme().tokens.background)
                .text_color(cx.theme().foreground),
        );
    }
    fn decorate(
        &self,
        surface: AnyElement,
        _: &gpui_base::Root,
        _: &mut Window,
        _: &mut App,
    ) -> impl IntoElement {
        div()
            .relative()
            .size_full()
            .capture_key_down(|_, window, cx| {
                Self::reset_context_menu_claim(window, cx);
            })
            .child(div().absolute().size_0().child(canvas(
                |_, _, _| {},
                |_, _, window, _| {
                    window.on_mouse_event(|_: &MouseDownEvent, phase, window, cx| {
                        if phase == DispatchPhase::Capture {
                            Self::reset_context_menu_claim(window, cx);
                        }
                    });
                    window.on_mouse_event(|event: &MouseUpEvent, phase, window, cx| {
                        if phase == DispatchPhase::Capture && event.button == MouseButton::Right {
                            Self::disarm_context_menu_claim(window, cx);
                        }
                    });
                },
            )))
            .child(surface)
    }
}
impl Render for WindowState {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .absolute()
            .inset_0()
            .child(self.touch_selection_overlay.clone())
            .child(self.tooltip_overlay.clone())
            .children(self.tooltip_press_listener(cx))
            .child(self.context_menu_overlay.clone())
    }
}
