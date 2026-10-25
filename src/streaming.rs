use gpui_kit::{App, AppContext as _, Entity, SharedString, Window};
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::prelude::*;

use crate::{APPLICATION, Json5EditorGlobal, tree_sitter_highlighter::ParseSnapshot};

thread_local! {
    static PREVIOUS: RefCell<Option<(ParseSnapshot, SharedString)>> = const { RefCell::new(None) };
}

fn update_editor(
    action: impl FnOnce(
        Entity<gpui_kit::component::input::EditorState>,
        Rc<RefCell<ParseSnapshot>>,
        Entity<SharedString>,
        &mut Window,
        &mut App,
    ),
) -> bool {
    let mut updated = false;
    APPLICATION.with(|application| {
        let application = application.borrow();
        let Some(handle) = application.as_ref() else {
            return;
        };
        handle.update(|cx| {
            let Some(window) = cx.windows().into_iter().next() else {
                return;
            };
            let _ = cx.update_window(window, |_, window, cx| {
                let Some((editor, syntax, filename)) =
                    cx.try_global::<Json5EditorGlobal>().map(|global| {
                        (
                            global.editor.clone(),
                            global.syntax.clone(),
                            global.filename.clone(),
                        )
                    })
                else {
                    return;
                };
                action(editor, syntax, filename, window, cx);
                updated = true;
            });
        });
    });
    updated
}

#[wasm_bindgen]
pub fn begin_streaming_document(name: String) -> bool {
    update_editor(|editor, syntax, filename, window, cx| {
        let previous_name = filename.read(cx).clone();
        let previous = std::mem::take(&mut *syntax.borrow_mut());
        PREVIOUS.with(|snapshot| *snapshot.borrow_mut() = Some((previous, previous_name)));
        syntax.borrow_mut().prepare_open();
        editor.update(cx, |state, cx| {
            state.set_value("", window, cx);
            state.set_readonly(true, cx);
        });
        filename.update(cx, |filename, cx| {
            *filename = name.into();
            cx.notify();
        });
    })
}

#[wasm_bindgen]
pub fn append_streaming_document(chunk: String) -> bool {
    update_editor(|editor, _, _, window, cx| {
        editor.update(cx, |state, cx| state.append_text(chunk, window, cx));
    })
}

#[wasm_bindgen]
pub fn cancel_streaming_document() {
    update_editor(|editor, syntax, filename, window, cx| {
        PREVIOUS.with(|snapshot| {
            if let Some((previous, name)) = snapshot.borrow_mut().take() {
                *syntax.borrow_mut() = previous;
                editor.update(cx, |state, cx| {
                    state.set_value("", window, cx);
                    state.set_readonly(false, cx);
                });
                filename.update(cx, |filename, cx| {
                    *filename = name;
                    cx.notify();
                });
            }
        });
    });
}

pub(super) fn finish() {
    PREVIOUS.with(|snapshot| *snapshot.borrow_mut() = None);
}

pub(super) fn is_receiving() -> bool {
    PREVIOUS.with(|snapshot| snapshot.borrow().is_some())
}
