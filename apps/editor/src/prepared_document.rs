//! Detached layout preparation; only the final commit changes the live document.
use crate::{APPLICATION, Json5EditorGlobal};
use gpui_kit::base::input::InputPreparation;
use gpui_kit::component::input::EditorState;
use gpui_kit::{App, Entity, SharedString};
use std::cell::RefCell;
use wasm_bindgen::prelude::*;

struct PendingDocument {
    id: u32,
    content: SharedString,
    preparation: Option<InputPreparation>,
}

thread_local! {
    static PENDING: RefCell<Option<PendingDocument>> = const { RefCell::new(None) };
}

fn with_editor(action: impl FnOnce(Entity<EditorState>, &mut App)) -> bool {
    let mut available = false;
    APPLICATION.with(|application| {
        if let Some(handle) = application.borrow().as_ref() {
            handle.update(|cx| {
                if let Some(editor) = cx
                    .try_global::<Json5EditorGlobal>()
                    .map(|global| global.editor.clone())
                {
                    action(editor, cx);
                    available = true;
                }
            });
        }
    });
    available
}

#[wasm_bindgen]
pub fn prepare_json5_file(id: u32, view: String) -> bool {
    let content: SharedString = view.into();
    with_editor(|editor, cx| {
        let state = editor.read(cx);
        let reuse = crate::streaming::is_receiving() && state.text() == content.as_str();
        let preparation = (!reuse).then(|| state.prepare_value(content.clone()));
        PENDING.with(|pending| {
            *pending.borrow_mut() = Some(PendingDocument {
                id,
                content,
                preparation,
            })
        });
    })
}

/// 0: more batches, 1: ready to commit, -1: cancelled or unavailable.
#[wasm_bindgen]
pub fn advance_json5_file_preparation(id: u32) -> i32 {
    let mut progress = -1;
    with_editor(|editor, cx| {
        PENDING.with(|pending| {
            let mut pending = pending.borrow_mut();
            let Some(document) = pending.as_mut().filter(|document| document.id == id) else {
                return;
            };
            progress = match document.preparation.as_mut() {
                Some(preparation) => i32::from(
                    editor.update(cx, |state, cx| state.advance_preparation(preparation, cx)),
                ),
                None => 1,
            };
        });
    });
    progress
}

/// Prepared and total UTF-8 bytes, or an empty array if preparation was canceled.
#[wasm_bindgen]
pub fn json5_file_preparation_progress(id: u32) -> Vec<u32> {
    PENDING.with(|pending| {
        let pending = pending.borrow();
        let Some(document) = pending.as_ref().filter(|document| document.id == id) else {
            return Vec::new();
        };
        let total = document
            .preparation
            .as_ref()
            .map_or(document.content.len(), |preparation| preparation.text().len());
        let processed = document
            .preparation
            .as_ref()
            .map_or(total, InputPreparation::processed_bytes);
        vec![processed as u32, total as u32]
    })
}

#[wasm_bindgen]
pub fn cancel_json5_file_preparation(id: u32) {
    PENDING.with(|pending| {
        let mut pending = pending.borrow_mut();
        if pending.as_ref().is_some_and(|document| document.id == id) {
            *pending = None;
        }
    });
}

pub(super) fn take_ready(view: &str) -> Result<Option<InputPreparation>, ()> {
    PENDING.with(|pending| {
        let Some(document) = pending.borrow_mut().take() else {
            return Ok(None);
        };
        if document.content.as_str() != view
            || document
                .preparation
                .as_ref()
                .is_some_and(|value| !value.is_complete())
        {
            return Err(());
        }
        Ok(document.preparation)
    })
}
