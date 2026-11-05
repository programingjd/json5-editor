//! Window roots and presentation-layer plugins.
use crate::input::Copy;
use crate::{StyledExt, TextSelectionLayer};
use gpui::{
    AnyElement, AnyView, App, AppContext, ClipboardItem, Context, Div, Entity, Global,
    InteractiveElement, IntoElement, KeyBinding, ParentElement, Render, Stateful, StyleRefinement,
    Styled, Window, actions, div,
};
use std::{any::TypeId, rc::Rc};

actions!(root, [Tab, TabPrev]);
const CONTEXT: &str = "Root";

fn advance_focus(window: &mut Window, cx: &mut App, advance: fn(&mut Window, &mut App)) {
    let Some(container) = crate::active_focus_trap(window, cx) else {
        advance(window, cx);
        return;
    };

    advance(window, cx);
    // GPUI's tab order can cycle short of the whole window (a stop whose
    // previous stop is itself), so the walk ends at the first repeated stop,
    // not only on a return to the one it began with.
    let mut visited = Vec::new();
    while !container.contains_focused(window, cx) {
        let focused = window.focused(cx);
        if visited.contains(&focused) {
            // No tab stop in the trap is reachable. Its own handle remains a
            // safe focus target even when excluded from ordinary tab order.
            container.focus(window, cx);
            break;
        }
        visited.push(focused);
        advance(window, cx);
    }
}

pub(crate) fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("tab", Tab, Some(CONTEXT)),
        KeyBinding::new("shift-tab", TabPrev, Some(CONTEXT)),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-c", Copy, Some(CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-c", Copy, Some(CONTEXT)),
    ]);
}

pub trait RootPlugin: Render + Sized {
    fn prepare(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {}

    fn style(&self, _surface: &mut Stateful<Div>, _window: &mut Window, _cx: &mut App) {}

    fn decorate(
        &self,
        surface: AnyElement,
        _root: &Root,
        _window: &mut Window,
        _cx: &mut App,
    ) -> impl IntoElement {
        surface
    }
}

type PluginFactory = Rc<dyn Fn(&mut Window, &mut Context<Root>) -> Plugin>;
type Prepare = Rc<dyn Fn(&mut Window, &mut App)>;
type SurfaceStyle = Rc<dyn Fn(&mut Stateful<Div>, &mut Window, &mut App)>;
type Decorate = Rc<dyn Fn(AnyElement, &Root, &mut Window, &mut App) -> AnyElement>;
#[derive(Default)]
struct PluginRegistry(Vec<(TypeId, PluginFactory)>);
impl Global for PluginRegistry {}
struct Plugin {
    view: AnyView,
    prepare: Prepare,
    style: SurfaceStyle,
    decorate: Decorate,
}

pub struct Root {
    view: AnyView,
    style: StyleRefinement,
    plugins: Vec<Plugin>,
}

impl Root {
    pub fn register_plugin<V: RootPlugin>(
        cx: &mut App,
        build: fn(&mut Window, &mut Context<V>) -> V,
    ) {
        if !cx.has_global::<PluginRegistry>() {
            cx.set_global(PluginRegistry::default());
        }
        let factory: PluginFactory = Rc::new(move |window, cx| {
            let entity = cx.new(|cx| build(window, cx));
            let root = cx.weak_entity();
            let observed = entity.clone();
            // Plugin construction can enqueue notifications. Observe only after
            // that effect cycle so mounting a Root does not immediately render
            // application content a second time.
            cx.defer(move |cx| {
                cx.observe(&observed, move |_, cx| {
                    let _ = root.update(cx, |_, cx| cx.notify());
                })
                .detach();
            });
            let prepare = entity.clone();
            let style = entity.clone();
            let decorate = entity.clone();
            Plugin {
                view: entity.into(),
                prepare: Rc::new(move |window, cx| {
                    prepare.update(cx, |state, cx| state.prepare(window, cx))
                }),
                style: Rc::new(move |surface, window, cx| {
                    style.update(cx, |state, cx| state.style(surface, window, cx))
                }),
                decorate: Rc::new(move |surface, root, window, cx| {
                    decorate.update(cx, |state, cx| {
                        state.decorate(surface, root, window, cx).into_any_element()
                    })
                }),
            }
        });
        let plugins = &mut cx.global_mut::<PluginRegistry>().0;
        if let Some(entry) = plugins.iter_mut().find(|(id, _)| *id == TypeId::of::<V>()) {
            entry.1 = factory;
        } else {
            plugins.push((TypeId::of::<V>(), factory));
        }
    }

    pub fn new(view: impl Into<AnyView>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        #[cfg(all(target_os = "macos", not(test)))]
        crate::install_window_hit_test_forwarder(window);
        let factories = cx
            .try_global::<PluginRegistry>()
            .map(|e| e.0.clone())
            .unwrap_or_default();
        Self {
            view: view.into(),
            style: StyleRefinement::default(),
            plugins: factories
                .into_iter()
                .map(|(_, build)| build(window, cx))
                .collect(),
        }
    }

    pub fn view(&self) -> &AnyView {
        &self.view
    }

    pub fn plugin<V: RootPlugin>(&self) -> Option<Entity<V>> {
        self.plugins
            .iter()
            .find_map(|entry| entry.view.clone().downcast::<V>().ok())
    }

    pub fn read<'a>(window: &'a Window, cx: &'a App) -> &'a Self {
        window
            .root::<Self>()
            .flatten()
            .expect("window must have a Base Root")
            .read(cx)
    }
    pub fn update<R>(
        window: &mut Window,
        cx: &mut App,
        f: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) -> R,
    ) -> R {
        let root = window
            .root::<Self>()
            .flatten()
            .expect("window must have a Base Root");
        root.update(cx, |root, cx| f(root, window, cx))
    }
    fn on_action_tab(&mut self, _: &Tab, window: &mut Window, cx: &mut Context<Self>) {
        advance_focus(window, cx, Window::focus_next);
    }

    fn on_action_tab_prev(&mut self, _: &TabPrev, window: &mut Window, cx: &mut Context<Self>) {
        advance_focus(window, cx, Window::focus_prev);
    }

    fn on_action_copy(&mut self, _: &Copy, window: &mut Window, cx: &mut Context<Self>) {
        let text = crate::TextSelection::selected_text(window, cx)
            .trim()
            .to_string();
        if text.is_empty() {
            cx.propagate();
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }
}
impl Styled for Root {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}
impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        for plugin in &self.plugins {
            (plugin.prepare)(window, cx);
        }
        let mut content = div()
            .id("root")
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::on_action_tab))
            .on_action(cx.listener(Self::on_action_tab_prev))
            .on_action(cx.listener(Self::on_action_copy))
            .relative()
            .size_full()
            .child(TextSelectionLayer)
            .child(self.view.clone())
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .children(self.plugins.iter().map(|plugin| plugin.view.clone())),
            );
        for plugin in &self.plugins {
            (plugin.style)(&mut content, window, cx);
        }
        let mut content = content.refine_style(&self.style).into_any_element();
        for plugin in &self.plugins {
            content = (plugin.decorate)(content, self, window, cx);
        }
        content
    }
}
