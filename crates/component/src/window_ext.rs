use crate::{input::AnyInputState, root::WindowState};
use gpui::{App, Window};

pub trait WindowExt {
    fn has_focused_input(&mut self, cx: &mut App) -> bool;
    fn focused_input(&mut self, cx: &mut App) -> Option<AnyInputState>;
}
impl WindowExt for Window {
    fn has_focused_input(&mut self, cx: &mut App) -> bool {
        self.focused_input(cx).is_some()
    }
    fn focused_input(&mut self, cx: &mut App) -> Option<AnyInputState> {
        let state = WindowState::read(self, cx).focused_input.clone()?;
        if state.focus_handle(cx).is_focused(self) {
            return Some(state);
        }
        WindowState::try_update(self, cx, |root, _, cx| {
            if root.focused_input.as_ref() == Some(&state) {
                root.focused_input = None;
                cx.notify();
            }
        });
        None
    }
}
