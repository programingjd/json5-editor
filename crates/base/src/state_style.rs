use gpui::{Refineable as _, StyleRefinement, Styled, prelude::FluentBuilder};

#[derive(Default)]
pub struct StateStyle {
    refinement: StyleRefinement,
}

impl StateStyle {
    pub(crate) fn into_refinement(self) -> StyleRefinement {
        self.refinement
    }
}

impl Styled for StateStyle {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.refinement
    }
}

impl FluentBuilder for StateStyle {}

pub(crate) fn resolve_style<'a>(
    instance: &StyleRefinement,
    active_states: impl IntoIterator<Item = &'a StyleRefinement>,
) -> StyleRefinement {
    let mut style = StyleRefinement::default();
    style.refine(instance);
    for state in active_states {
        style.refine(state);
    }
    style
}
