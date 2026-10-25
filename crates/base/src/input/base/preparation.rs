//! Detached, cancellable layout preparation for whole-document installation.
use gpui::{Context, SharedString, Window};
use ropey::Rope;
use std::borrow::Cow;

use super::{EditorMode, InputBaseState, display_map::RewrapJob};

/// An unpublished document and its incrementally prepared wrapping layout.
/// Dropping this value cancels preparation without changing the editor.
pub struct InputPreparation {
    content: SharedString,
    job: RewrapJob,
}

impl InputPreparation {
    pub fn text(&self) -> &str {
        &self.content
    }
    pub fn is_complete(&self) -> bool {
        self.job.is_complete() && self.job.has_complete_layout()
    }

    /// Number of document bytes whose layout has been prepared.
    pub fn processed_bytes(&self) -> usize {
        self.job.processed_bytes()
    }
}

impl InputBaseState<EditorMode> {
    /// Begin preparing a value without changing the current document or history.
    pub fn prepare_value(&self, value: impl Into<SharedString>) -> InputPreparation {
        let value = value.into();
        let content = match self.normalize_input(&value) {
            Cow::Borrowed(_) => value,
            Cow::Owned(value) => value.into(),
        };
        let width = self
            .last_layout
            .as_ref()
            .and_then(|layout| layout.wrap_width);
        let job = self.display_map.begin_preparation(
            Rope::from(content.as_str()),
            width,
            self.wrapping_indent,
        );
        InputPreparation { content, job }
    }

    /// Prepare one bounded batch. Size/font changes restart only the unpublished layout.
    pub fn advance_preparation(
        &self,
        preparation: &mut InputPreparation,
        cx: &mut Context<Self>,
    ) -> bool {
        let width = self
            .last_layout
            .as_ref()
            .and_then(|layout| layout.wrap_width);
        if !self
            .display_map
            .preparation_matches(&preparation.job, width, self.wrapping_indent)
        {
            preparation.job = self.display_map.begin_preparation(
                preparation.job.text().clone(),
                width,
                self.wrapping_indent,
            );
        }
        preparation.job.advance(cx)
    }

    /// Install a completed value using its prepared layout and reset history as set_value does.
    pub fn set_prepared_value(
        &mut self,
        preparation: InputPreparation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let width = self
            .last_layout
            .as_ref()
            .and_then(|layout| layout.wrap_width);
        if !preparation.is_complete()
            || !self
                .display_map
                .preparation_matches(&preparation.job, width, self.wrapping_indent)
        {
            return false;
        }
        self.rewrap_epoch = self.rewrap_epoch.wrapping_add(1);
        self.rewrap_task = None;
        self.pending_rewrap = None;
        self.display_map.stage_prepared_replacement(preparation.job);
        self.set_value(preparation.content, window, cx);
        true
    }
}
