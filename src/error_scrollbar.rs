use gpui_kit::base::{
    Scrollbar,
    input::{DiagnosticEntry, DiagnosticSeverity, InputScrollbarContext, RopeExt as _},
};
use gpui_kit::component::{input::EditorState, tooltip::Tooltip};
use gpui_kit::{
    AnyElement, App, Bounds, Element, ElementId, GlobalElementId, Hsla, InspectorElementId,
    InteractiveElement as _, IntoElement, LayoutId, MouseButton, Pixels, Position,
    StatefulInteractiveElement as _, Style, Styled as _, WeakEntity, Window, div, fill, point, px,
    relative, size, transparent_black,
};

const ERROR_MARKER_HEIGHT: Pixels = px(2.);
const ERROR_MARKER_HIT_PADDING: Pixels = px(2.);

pub(super) fn render(
    editor: &WeakEntity<EditorState>,
    scrollbar: Scrollbar,
    context: &InputScrollbarContext,
    _: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let errors = editor
        .upgrade()
        .map(|editor| {
            // The hook projects this same DiagnosticSet in iterator order.
            // Keep the source entry with its projected position for navigation
            // and tooltips, without adding application data to the Base hook.
            context
                .diagnostics()
                .iter()
                .zip(
                    editor
                        .read(cx)
                        .diagnostics()
                        .into_iter()
                        .flat_map(|set| set.iter()),
                )
                .filter(|((_, severity), _)| *severity == DiagnosticSeverity::Error)
                .map(|((position, _), diagnostic)| (*position, diagnostic.clone()))
                .collect()
        })
        .unwrap_or_default();
    ErrorScrollbar {
        editor: editor.clone(),
        scrollbar: scrollbar
            .track_clickable_when_hidden(true)
            .styles(|styles| {
                // Keep the track transparent in every thumb visibility state.
                styles
                    .track(|style| style.bg(transparent_black()))
                    .track_hover(|style| style.bg(transparent_black()))
                    .track_active(|style| style.bg(transparent_black()))
            })
            .into_any_element(),
        errors,
        error_color: crate::palette::get(cx).error_marker,
    }
    .into_any_element()
}

struct ErrorScrollbar {
    editor: WeakEntity<EditorState>,
    scrollbar: AnyElement,
    errors: Vec<(f32, DiagnosticEntry)>,
    error_color: Hsla,
}

struct ErrorScrollbarLayout {
    markers: Vec<(Bounds<Pixels>, AnyElement)>,
}

impl IntoElement for ErrorScrollbar {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for ErrorScrollbar {
    type RequestLayoutState = ();
    type PrepaintState = ErrorScrollbarLayout;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.position = Position::Absolute;
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> ErrorScrollbarLayout {
        self.scrollbar
            .prepaint_as_root(bounds.origin, bounds.size.into(), window, cx);
        let width = Scrollbar::width().min(bounds.size.width);
        let track = Bounds::new(
            point(bounds.right() - width, bounds.top()),
            size(width, bounds.size.height),
        );
        let scale = window.scale_factor();
        let height =
            px((f32::from(ERROR_MARKER_HEIGHT) * scale).ceil() / scale).min(track.size.height);
        let last_top = (track.size.height - height).max(px(0.));
        let mut rows: Vec<(i32, usize)> = self
            .errors
            .iter()
            .enumerate()
            .map(|(index, (position, _))| {
                let offset = (track.size.height * position.clamp(0., 1.)).min(last_top);
                (
                    (f32::from(track.top() + offset) * scale).floor() as i32,
                    index,
                )
            })
            .collect();
        rows.sort_unstable();
        rows.dedup_by_key(|(row, _)| *row);
        let markers = rows
            .iter()
            .enumerate()
            .map(|(index, (row, error_index))| {
                let top = px(*row as f32 / scale).max(track.top());
                let marker = Bounds::new(point(track.left(), top), size(width, height));
                // Pad the thin bar's target, splitting nearby targets halfway
                // between their centers so neighboring errors remain reachable.
                let previous = index.checked_sub(1).map(|index| rows[index].0);
                let next = rows.get(index + 1).map(|(row, _)| *row);
                let hit_top = (top - ERROR_MARKER_HIT_PADDING).max(track.top()).max(
                    previous.map_or(track.top(), |previous| {
                        px((previous as f32 + *row as f32) / (2. * scale)) + height / 2.
                    }),
                );
                let hit_bottom = (marker.bottom() + ERROR_MARKER_HIT_PADDING)
                    .min(track.bottom())
                    .min(next.map_or(track.bottom(), |next| {
                        px((next as f32 + *row as f32) / (2. * scale)) + height / 2.
                    }));
                let diagnostic = &self.errors[*error_index].1;
                let message = diagnostic.message.clone();
                let offset = diagnostic.range.start;
                let editor = self.editor.clone();
                let mut target = div()
                    .id(("error-marker", offset))
                    .w(width)
                    .h(hit_bottom - hit_top)
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        // The marker owns this click; the underlying thumb must
                        // not start dragging or move the scroll offset as well.
                        window.prevent_default();
                        cx.stop_propagation();
                        let _ = editor.update(cx, |state, cx| {
                            let position = state
                                .text()
                                .offset_to_position(offset.min(state.text().len()));
                            state.unfold_at(position, cx);
                            state.set_cursor_position(position, window, cx);
                            state.center_cursor(cx);
                        });
                    })
                    .tooltip(move |window, cx| {
                        Tooltip::new(message.clone())
                            .py_1()
                            .max_w(px(500.))
                            .build(window, cx)
                    })
                    .into_any_element();
                target.prepaint_as_root(
                    point(track.left(), hit_top),
                    size(width, hit_bottom - hit_top).into(),
                    window,
                    cx,
                );
                (marker, target)
            })
            .collect();
        ErrorScrollbarLayout { markers }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        layout: &mut ErrorScrollbarLayout,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.scrollbar.paint(window, cx);
        for (marker, target) in &mut layout.markers {
            window.paint_quad(fill(*marker, self.error_color));
            target.paint(window, cx);
        }
    }
}
