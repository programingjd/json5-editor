use std::rc::Rc;

use gpui::{
    App, Bounds, Hitbox, HitboxBehavior, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, RenderOnce, Styled as _, TouchDragEvent, TouchPhase, Window,
    canvas, deferred,
};
use gpui_base::{SelectionEdge, TouchHandle, TouchSelectionSnapshot};

use crate::ActiveTheme as _;

pub(crate) type SnapshotSource = Rc<dyn Fn(&Window, &App) -> Option<TouchSelectionSnapshot>>;

pub(crate) type DragHandler =
    Rc<dyn Fn(SelectionEdge, TouchPhase, Point<Pixels>, &mut Window, &mut App)>;

pub(crate) type SurfaceHandler = Rc<dyn Fn(Bounds<Pixels>, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub(crate) struct SelectionHandles {
    source: SnapshotSource,
    on_drag: DragHandler,
    on_paint: Option<SurfaceHandler>,
}

struct LaidOutHandle {
    edge: SelectionEdge,
    caret: Bounds<Pixels>,
    hitbox: Hitbox,
}

impl SelectionHandles {
    pub(crate) fn new(source: SnapshotSource, on_drag: DragHandler) -> Self {
        Self {
            source,
            on_drag,
            on_paint: None,
        }
    }

    pub(crate) fn on_paint(mut self, on_paint: SurfaceHandler) -> Self {
        self.on_paint = Some(on_paint);
        self
    }

    fn layout(source: &SnapshotSource, window: &mut Window, cx: &mut App) -> Vec<LaidOutHandle> {
        let Some(snapshot) = source(window, cx) else {
            return Vec::new();
        };
        let window_bounds = Bounds::new(Point::default(), window.viewport_size());
        let mut handles = Vec::with_capacity(2);
        if !snapshot.is_empty() {
            for edge in [SelectionEdge::Start, SelectionEdge::End] {
                // No handle for an end scrolled out of its owner, nor for one
                // outside the window.
                let caret = snapshot.edge(edge);
                if !snapshot.is_edge_visible(edge) || !window_bounds.contains(&caret.origin) {
                    continue;
                }
                let hitbox = window.insert_hitbox(
                    TouchHandle::hit_bounds(edge, caret),
                    HitboxBehavior::BlockMouse,
                );
                handles.push(LaidOutHandle {
                    edge,
                    caret,
                    hitbox,
                });
            }
        }
        handles
    }

    fn paint_handle(handle: &LaidOutHandle, window: &mut Window, cx: &mut App) {
        TouchHandle::paint(
            handle.edge,
            handle.caret,
            cx.theme().selection.alpha(1.),
            window,
        );
    }

    fn listen(
        handles: &[LaidOutHandle],
        source: &SnapshotSource,
        on_drag: &DragHandler,
        window: &mut Window,
    ) {
        for handle in handles {
            // Touch: the drag is offered on the first touch, before it can
            // become a tap, a long press or a pan.
            window.on_mouse_event({
                let hitbox = handle.hitbox.clone();
                let edge = handle.edge;
                let on_drag = on_drag.clone();
                move |event: &TouchDragEvent, phase, window, cx| {
                    if !phase.bubble()
                        || event.phase != TouchPhase::Started
                        || window.default_prevented()
                        || !hitbox.is_hovered(window)
                    {
                        return;
                    }
                    window.prevent_default();
                    cx.stop_propagation();
                    on_drag(edge, TouchPhase::Started, event.position, window, cx);
                }
            });
            // Mouse: the same drag for a pointer.
            window.on_mouse_event({
                let hitbox = handle.hitbox.clone();
                let edge = handle.edge;
                let on_drag = on_drag.clone();
                move |event: &MouseDownEvent, phase, window, cx| {
                    if !phase.bubble()
                        || event.button != MouseButton::Left
                        || !hitbox.is_hovered(window)
                    {
                        return;
                    }
                    cx.stop_propagation();
                    on_drag(edge, TouchPhase::Started, event.position, window, cx);
                }
            });
        }

        // The drag in progress is read live: it may have begun after this
        // frame painted, and its first moves must not be lost.
        let dragging = {
            let source = source.clone();
            move |window: &Window, cx: &App| source(window, cx).and_then(|s| s.dragging())
        };
        window.on_mouse_event({
            let on_drag = on_drag.clone();
            let dragging = dragging.clone();
            move |event: &TouchDragEvent, phase, window, cx| {
                if !phase.bubble() || event.phase == TouchPhase::Started {
                    return;
                }
                let Some(edge) = dragging(window, cx) else {
                    return;
                };
                cx.stop_propagation();
                on_drag(edge, event.phase, event.position, window, cx);
            }
        });
        window.on_mouse_event({
            let on_drag = on_drag.clone();
            let dragging = dragging.clone();
            move |event: &MouseMoveEvent, phase, window, cx| {
                if !phase.bubble() || event.pressed_button != Some(MouseButton::Left) {
                    return;
                }
                let Some(edge) = dragging(window, cx) else {
                    return;
                };
                on_drag(edge, TouchPhase::Moved, event.position, window, cx);
            }
        });
        window.on_mouse_event({
            let on_drag = on_drag.clone();
            move |event: &MouseUpEvent, phase, window, cx| {
                if !phase.bubble() || event.button != MouseButton::Left {
                    return;
                }
                let Some(edge) = dragging(window, cx) else {
                    return;
                };
                on_drag(edge, TouchPhase::Ended, event.position, window, cx);
            }
        });
    }
}

impl RenderOnce for SelectionHandles {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let source = self.source;
        let on_drag = self.on_drag;
        let on_paint = self.on_paint;
        deferred(
            canvas(
                {
                    let source = source.clone();
                    move |_, window, cx| Self::layout(&source, window, cx)
                },
                move |_, handles, window, cx| {
                    for handle in &handles {
                        Self::paint_handle(handle, window, cx);
                        if let Some(on_paint) = on_paint.as_ref() {
                            on_paint(handle.hitbox.bounds, window, cx);
                        }
                    }
                    Self::listen(&handles, &source, &on_drag, window);
                },
            )
            .absolute()
            .size_0(),
        )
        .with_priority(gpui_base::POPUP_PRIORITY)
    }
}
