use gpui::{
    App, ClickEvent, InteractiveElement, OngoingScroll, Pixels, Point, Stateful,
    StatefulInteractiveElement, TouchPhase, Window,
};

pub trait OngoingScrollExt {
    fn lock_axis(&mut self, delta: &mut Point<Pixels>, touch_phase: TouchPhase);
}

impl OngoingScrollExt for OngoingScroll {
    fn lock_axis(&mut self, delta: &mut Point<Pixels>, touch_phase: TouchPhase) {
        #[cfg(target_family = "wasm")]
        let _ = (delta, touch_phase);
        #[cfg(not(target_family = "wasm"))]
        self.filter(delta, touch_phase);
    }
}

pub trait InteractiveElementExt: InteractiveElement {
    fn lock_scroll_axis(self) -> Self
    where
        Self: Sized + StatefulInteractiveElement,
    {
        #[cfg(target_family = "wasm")]
        {
            self
        }
        #[cfg(not(target_family = "wasm"))]
        {
            self.restrict_scroll_to_axis()
        }
    }

    fn on_double_click(
        mut self,
        listener: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self
    where
        Self: Sized,
    {
        self.interactivity().on_click(move |event, window, cx| {
            if event.click_count() == 2 {
                listener(event, window, cx);
            }
        });
        self
    }
}

impl<E: InteractiveElement> InteractiveElementExt for Stateful<E> {}
