use std::rc::Rc;

use gpui::{AnyElement, App, Window};

use crate::{Scrollbar, input::DiagnosticSeverity};

#[derive(Clone, Default)]
pub struct InputScrollbarContext {
    diagnostics: Vec<(f32, DiagnosticSeverity)>,
}

impl InputScrollbarContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_diagnostics(
        mut self,
        diagnostics: impl IntoIterator<Item = (f32, DiagnosticSeverity)>,
    ) -> Self {
        self.diagnostics = diagnostics.into_iter().collect();
        self
    }

    pub fn diagnostics(&self) -> &[(f32, DiagnosticSeverity)] {
        &self.diagnostics
    }
}

pub type InputScrollbarRenderer =
    Rc<dyn Fn(Scrollbar, &InputScrollbarContext, &mut Window, &mut App) -> AnyElement>;
