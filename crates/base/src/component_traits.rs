#[allow(patterns_in_fns_without_body)]
pub trait Selectable: Sized {
    fn selected(mut self, selected: bool) -> Self;
    fn is_selected(&self) -> bool;

    fn secondary_selected(self, _: bool) -> Self {
        self
    }

    fn open(self, open: bool) -> Self {
        self.selected(open)
    }

    fn is_open(&self) -> bool {
        self.is_selected()
    }
}

#[allow(patterns_in_fns_without_body)]
pub trait Disableable {
    fn disabled(mut self, disabled: bool) -> Self;
}

pub trait FocusableExt: Sized {
    fn focus_ring(self, enabled: bool) -> Self;
    fn is_focus_ring_enabled(&self) -> bool;
}

pub trait Collapsible {
    fn collapsed(self, collapsed: bool) -> Self;
    fn is_collapsed(&self) -> bool;
}
