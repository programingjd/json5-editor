use gpui::{AnyElement, App, Context, IntoElement, ParentElement as _, Styled as _, Task, Window};

use crate::{
    ActiveTheme as _, Icon, IconName, IndexPath, Selectable, h_flex,
    list::{ListState, loading::Loading},
};

#[allow(unused)]
pub trait ListDelegate: Sized + 'static {
    type Item: Selectable + IntoElement;

    fn perform_search(
        &mut self,
        query: &str,
        window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Task<()> {
        Task::ready(())
    }

    fn sections_count(&self, cx: &App) -> usize {
        1
    }

    fn items_count(&self, section: usize, cx: &App) -> usize;

    fn render_item(
        &mut self,
        ix: IndexPath,
        window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<Self::Item>;

    fn render_section_header(
        &mut self,
        section: usize,
        window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<impl IntoElement> {
        None::<AnyElement>
    }

    fn render_section_footer(
        &mut self,
        section: usize,
        window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<impl IntoElement> {
        None::<AnyElement>
    }

    fn render_empty(
        &mut self,
        window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        h_flex()
            .size_full()
            .justify_center()
            .text_color(cx.theme().muted_foreground.opacity(0.6))
            .child(Icon::new(IconName::Folder).size_12())
            .into_any_element()
    }

    fn render_initial(
        &mut self,
        window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<AnyElement> {
        None
    }

    fn loading(&self, cx: &App) -> bool {
        false
    }

    fn render_loading(
        &mut self,
        window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        Loading
    }

    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    );

    fn set_right_clicked_index(
        &mut self,
        ix: Option<IndexPath>,
        window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
    }

    fn confirm(&mut self, secondary: bool, window: &mut Window, cx: &mut Context<ListState<Self>>) {
    }

    fn cancel(&mut self, window: &mut Window, cx: &mut Context<ListState<Self>>) {}

    fn has_more(&self, cx: &App) -> bool {
        false
    }

    fn load_more_threshold(&self) -> usize {
        20
    }

    fn load_more(&mut self, window: &mut Window, cx: &mut Context<ListState<Self>>) {}
}
