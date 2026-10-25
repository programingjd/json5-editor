//! Editor window presentation: input registration, text selection and tooltips.
use crate::context_menu::ContextMenuOverlay;
use crate::{
    ActiveTheme, input::AnyInputState, tooltip::render_tooltip,
    touch_selection::WindowTouchSelectionOverlay,
};
use gpui::{
    App, AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, Window, div,
};
use gpui_base::{TextSelection, TextSelectionScopeId};

pub(crate) fn init(cx: &mut App) {
    gpui_base::Root::register_plugin::<WindowState>(cx, WindowState::new);
}
pub(crate) struct WindowState {
    pub(super) focused_input: Option<AnyInputState>,
    tooltip_overlay: Entity<gpui_base::TooltipOverlay>,
    context_menu_overlay: Entity<ContextMenuOverlay>,
    touch_selection_overlay: Entity<WindowTouchSelectionOverlay>,
}
impl WindowState {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            focused_input: None,
            context_menu_overlay: cx.new(|_| ContextMenuOverlay::new()),
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
    pub(crate) fn context_menu_overlay(
        window: &Window,
        cx: &App,
    ) -> Option<Entity<ContextMenuOverlay>> {
        Some(Self::try_read(window, cx)?.context_menu_overlay.clone())
    }
}
impl gpui_base::RootPlugin for WindowState {
    fn prepare(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
}
impl Render for WindowState {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .absolute()
            .inset_0()
            .child(self.touch_selection_overlay.clone())
            .child(self.tooltip_overlay.clone())
            .child(self.context_menu_overlay.clone())
    }
}
