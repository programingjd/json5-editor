#[cfg(not(target_family = "wasm"))]
use std::time::Instant;
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};
#[cfg(target_family = "wasm")]
use web_time::Instant;

use gpui::{App, Entity, Global, OwnedMenu};

use crate::text::TextViewState;

pub struct DeferredPopover(#[allow(dead_code)] Rc<()>);

pub struct GlobalState {
    app_menus: Vec<OwnedMenu>,
    deferred_popovers: Vec<Weak<()>>,
    suppress_text_selection: bool,

    text_view_state_stack: RefCell<Vec<Entity<TextViewState>>>,

    selection_document_order: Cell<u64>,

    last_touch: Option<Instant>,
}

impl Global for GlobalState {}

impl GlobalState {
    fn new() -> Self {
        Self {
            app_menus: Vec::new(),
            deferred_popovers: Vec::new(),
            suppress_text_selection: false,
            text_view_state_stack: RefCell::new(Vec::new()),
            selection_document_order: Cell::new(1),
            last_touch: None,
        }
    }

    pub fn note_touch(cx: &mut App) {
        Self::init(cx);
        Self::global_mut(cx).last_touch = Some(Instant::now());
    }

    pub fn is_touch_press(cx: &App) -> bool {
        cx.try_global::<Self>()
            .and_then(|state| state.last_touch)
            .is_some_and(|at| at.elapsed() < std::time::Duration::from_secs(1))
    }

    #[doc(hidden)]
    pub fn init(cx: &mut App) {
        if !cx.has_global::<Self>() {
            cx.set_global(Self::new());
        }
    }

    pub fn suppress_text_selection(cx: &mut App) {
        Self::global_mut(cx).suppress_text_selection = true;
    }

    #[doc(hidden)]
    pub fn reset_text_selection_suppression(cx: &mut App) {
        Self::global_mut(cx).suppress_text_selection = false;
    }

    #[doc(hidden)]
    pub fn is_text_selection_suppressed(cx: &App) -> bool {
        Self::global(cx).suppress_text_selection
    }

    pub fn global(cx: &App) -> &Self {
        cx.global::<Self>()
    }

    pub fn global_mut(cx: &mut App) -> &mut Self {
        cx.global_mut::<Self>()
    }

    pub(crate) fn text_view_state(&self) -> Option<Entity<TextViewState>> {
        self.text_view_state_stack.borrow().last().cloned()
    }

    pub(crate) fn push_text_view_state(&self, state: Entity<TextViewState>) {
        self.text_view_state_stack.borrow_mut().push(state);
    }

    pub(crate) fn pop_text_view_state(&self) {
        self.text_view_state_stack.borrow_mut().pop();
    }

    #[doc(hidden)]
    pub fn begin_selection_frame(&self) {
        self.selection_document_order.set(1);
    }

    pub(crate) fn next_selection_document_order(&self) -> u64 {
        let order = self.selection_document_order.get();
        self.selection_document_order.set(order.wrapping_add(1));
        order
    }

    pub fn app_menus(&self) -> &[OwnedMenu] {
        &self.app_menus
    }

    pub fn set_app_menus(&mut self, menus: Vec<OwnedMenu>) {
        self.app_menus = menus;
    }

    pub fn is_in_deferred_context(cx: &App) -> bool {
        Self::global(cx)
            .deferred_popovers
            .iter()
            .any(|popover| popover.strong_count() > 0)
    }

    pub fn register_deferred_popover(cx: &mut App) -> DeferredPopover {
        let token = Rc::new(());
        let state = Self::global_mut(cx);
        state
            .deferred_popovers
            .retain(|popover| popover.strong_count() > 0);
        state.deferred_popovers.push(Rc::downgrade(&token));
        DeferredPopover(token)
    }
}
