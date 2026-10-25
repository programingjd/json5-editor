use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    App, Axis, BorderStyle, Bounds, ContentMask, Edges, Element, ElementId, GlobalElementId,
    Hitbox, Hsla, InteractiveElement as _, IntoElement, IsZero as _, LayoutId, OngoingScroll,
    PaintQuad, ParentElement as _, Point, Position, ScrollHandle, ScrollWheelEvent,
    StatefulInteractiveElement as _, Style, StyleRefinement, Styled as _, Window, div, px,
    relative,
};
use gpui::{Corners, Pixels};

use crate::{AxisExt, OngoingScrollExt as _, ScrollbarHandle, StyledExt as _};

#[inline]
#[track_caller]
fn caller_id() -> ElementId {
    ElementId::CodeLocation(*std::panic::Location::caller())
}

pub(crate) fn horizontal_scroll_area(
    id: impl Into<ElementId>,
    scroll_handle: &ScrollHandle,
    style: &StyleRefinement,
    child: impl IntoElement,
) -> impl IntoElement {
    let id = id.into();

    // The mask must be a sibling of the scrolled element (like in Table), not
    // a child of it: children are prepainted with the scroll offset applied,
    // which would slide the mask away from the viewport as the content
    // scrolls, leaving the uncovered part to the parent scroller.
    //
    // The frame cover is a sibling for the same reason: it keeps the rounded
    // corners clean at every scroll offset (see [`RoundedFrameCover`]).
    let frame = RoundedFrameCover {
        corner_radii: Corners {
            top_left: style.corner_radii.top_left,
            top_right: style.corner_radii.top_right,
            bottom_left: style.corner_radii.bottom_left,
            bottom_right: style.corner_radii.bottom_right,
        },
        backdrop: None,
        frame_border: px(0.),
    };

    div()
        .w_full()
        .relative()
        .child(
            div()
                .id(id.clone())
                .w_full()
                .refine_style(style)
                .overflow_hidden()
                .track_scroll(scroll_handle)
                .child(child),
        )
        .child(ScrollableMask::new(Axis::Horizontal, scroll_handle).id(id))
        .child(frame)
}

pub struct RoundedFrameCover {
    corner_radii: Corners<Option<gpui::AbsoluteLength>>,
    backdrop: Option<Hsla>,
    frame_border: Pixels,
}

impl RoundedFrameCover {
    pub fn uniform(radius: Pixels) -> Self {
        let radius = Some(radius.into());
        Self {
            corner_radii: Corners {
                top_left: radius,
                top_right: radius,
                bottom_left: radius,
                bottom_right: radius,
            },
            backdrop: None,
            frame_border: px(0.),
        }
    }

    pub fn frame_border(mut self, width: Pixels) -> Self {
        self.frame_border = width;
        self
    }

    pub fn backdrop(mut self, color: Hsla) -> Self {
        self.backdrop = Some(color);
        self
    }
}

impl IntoElement for RoundedFrameCover {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for RoundedFrameCover {
    type RequestLayoutState = ();
    type PrepaintState = Bounds<Pixels>;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        // Same layout trick as `ScrollableMask`: absolute and parent-sized.
        let mut style = Style::default();
        style.position = Position::Absolute;
        style.flex_grow = 1.0;
        style.flex_shrink = 1.0;
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();

        (window.request_layout(style, None, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Window,
        _: &mut App,
    ) -> Self::PrepaintState {
        // Shift up by one height to cover the viewport, like `ScrollableMask`.
        Bounds {
            origin: Point {
                x: bounds.origin.x,
                y: bounds.origin.y - bounds.size.height,
            },
            size: bounds.size,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        cover_bounds: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        use crate::theme::ActiveTheme as _;

        let mut bounds = *cover_bounds;
        if !self.frame_border.is_zero() {
            bounds.origin.x -= self.frame_border;
            bounds.origin.y -= self.frame_border;
            bounds.size.width += self.frame_border * 2.;
            bounds.size.height += self.frame_border * 2.;
        }
        let rem_size = window.rem_size();
        let resolve = |corner: Option<gpui::AbsoluteLength>| {
            corner
                .map(|radius| radius.to_pixels(rem_size))
                .filter(|radius| !radius.is_zero())
        };
        let radii = Corners {
            top_left: resolve(self.corner_radii.top_left),
            top_right: resolve(self.corner_radii.top_right),
            bottom_left: resolve(self.corner_radii.bottom_left),
            bottom_right: resolve(self.corner_radii.bottom_right),
        };
        if radii.top_left.is_none()
            && radii.top_right.is_none()
            && radii.bottom_left.is_none()
            && radii.bottom_right.is_none()
        {
            return;
        }

        let backdrop = self.backdrop.unwrap_or(cx.theme().tokens.colors.background);
        let corners = [
            (bounds.origin, 1., 1., radii.top_left),
            (bounds.top_right(), -1., 1., radii.top_right),
            (bounds.bottom_right(), -1., -1., radii.bottom_right),
            (
                Point {
                    x: bounds.origin.x,
                    y: bounds.bottom_right().y,
                },
                1.,
                -1.,
                radii.bottom_left,
            ),
        ];
        for (corner, x_dir, y_dir, radius) in corners {
            let Some(radius) = radius else { continue };
            if let Some(path) = corner_notch_path(corner, x_dir, y_dir, radius) {
                window.paint_path(path, backdrop);
            }
        }
    }
}

fn corner_notch_path(
    corner: Point<Pixels>,
    x_dir: f32,
    y_dir: f32,
    radius: Pixels,
) -> Option<gpui::Path<Pixels>> {
    let mut builder = gpui::PathBuilder::fill();
    builder.move_to(corner);
    builder.line_to(Point {
        x: corner.x + radius * x_dir,
        y: corner.y,
    });
    builder.arc_to(
        Point {
            x: radius,
            y: radius,
        },
        px(0.),
        false,
        // The quarter arc nearest the corner: counterclockwise when the two
        // inward directions agree in sign, clockwise otherwise.
        x_dir * y_dir < 0.,
        Point {
            x: corner.x,
            y: corner.y + radius * y_dir,
        },
    );
    builder.close();
    builder.build().ok()
}

pub struct ScrollableMask<H: ScrollbarHandle + Clone = ScrollHandle> {
    axis: Axis,
    id: ElementId,
    scroll_handle: H,
    debug: Option<Hsla>,
}

impl<H: ScrollbarHandle + Clone> ScrollableMask<H> {
    #[track_caller]
    pub fn new(axis: Axis, scroll_handle: &H) -> Self {
        Self {
            scroll_handle: scroll_handle.clone(),
            axis,
            id: caller_id(),
            debug: None,
        }
    }

    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
    }

    #[allow(dead_code)]
    pub fn debug(mut self) -> Self {
        self.debug = Some(gpui::yellow());
        self
    }
}

impl<H: ScrollbarHandle + Clone> IntoElement for ScrollableMask<H> {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl<H: ScrollbarHandle + Clone> Element for ScrollableMask<H> {
    type RequestLayoutState = ();
    type PrepaintState = Hitbox;

    // An id is needed to keep the gesture's axis lock across frames. The axis
    // suffix keeps both masks of one scroller apart when they share an id.
    fn id(&self) -> Option<ElementId> {
        let axis = match self.axis {
            Axis::Horizontal => "horizontal",
            Axis::Vertical => "vertical",
        };

        Some((self.id.clone(), axis).into())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        // Set the layout style relative to the table view to get same size.
        style.position = Position::Absolute;
        style.flex_grow = 1.0;
        style.flex_shrink = 1.0;
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();

        (window.request_layout(style, None, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        _: &mut App,
    ) -> Self::PrepaintState {
        // Move y to bounds height to cover the parent view.
        let cover_bounds = Bounds {
            origin: Point {
                x: bounds.origin.x,
                y: bounds.origin.y - bounds.size.height,
            },
            size: bounds.size,
        };

        window.insert_hitbox(cover_bounds, gpui::HitboxBehavior::Normal)
    }

    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        hitbox: &mut Self::PrepaintState,
        window: &mut Window,
        _: &mut App,
    ) {
        let is_horizontal = self.axis.is_horizontal();
        let line_height = window.line_height();
        let bounds = hitbox.bounds;
        let ongoing_scroll = global_id
            .map(|global_id| {
                window.with_element_state::<Rc<RefCell<OngoingScroll>>, _>(global_id, |state, _| {
                    let state = state.unwrap_or_default();
                    (state.clone(), state)
                })
            })
            .unwrap_or_default();

        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            if let Some(color) = self.debug {
                window.paint_quad(PaintQuad {
                    bounds,
                    border_widths: Edges::all(px(1.0)),
                    border_color: color,
                    background: gpui::transparent_white().into(),
                    corner_radii: Corners::all(px(0.)),
                    border_style: BorderStyle::default(),
                });
            }

            window.on_mouse_event({
                let view_id = window.current_view();
                let scroll_handle = self.scroll_handle.clone();
                let hitbox_id = hitbox.id;
                let ongoing_scroll = ongoing_scroll.clone();

                move |event: &ScrollWheelEvent, phase, window, cx| {
                    // Handle in the capture phase: ancestor scrollers such as
                    // `gpui::list` register their wheel listeners after their
                    // children paint, so in the bubble phase (reverse
                    // registration order) they run first and would consume the
                    // vertical component of a trackpad swipe before this mask
                    // could stop the propagation.
                    //
                    // `should_handle_scroll` (instead of a raw bounds check)
                    // keeps the mask inert when it is occluded, e.g. below an
                    // open dialog or context menu.
                    if !(phase.capture() && hitbox_id.should_handle_scroll(window)) {
                        return;
                    }

                    let mut offset = scroll_handle.offset();
                    let mut delta = event.delta.pixel_delta(line_height);

                    // Lock the gesture to the axis it started on, so a diagonal
                    // trackpad swipe cannot flip which mask consumes it from one
                    // event to the next. Line deltas carry no touch phase.
                    if event.delta.precise() {
                        ongoing_scroll
                            .borrow_mut()
                            .lock_axis(&mut delta, event.touch_phase);
                    }

                    // Limit for only one way scrolling at same time.
                    // When use MacBook touchpad we may get both x and y delta,
                    // only allows the one that more to scroll.
                    if !delta.x.is_zero() && !delta.y.is_zero() {
                        if delta.x.abs() > delta.y.abs() {
                            delta.y = px(0.);
                        } else {
                            delta.x = px(0.);
                        }
                    }

                    if !is_horizontal {
                        // The current offset must be clamped too: after a
                        // bubbled event, the scrolled element's own listener
                        // pushes the shared offset beyond the edge unclamped
                        // (the div only clamps on prepaint), and that
                        // transient overscroll would read as "room to scroll".
                        // `ScrollbarHandle` has no `max_offset`; recover it from
                        // the definition `content_size = viewport + max_offset`.
                        let axis_max = (scroll_handle.content_size().height
                            - scroll_handle.viewport_bounds().size.height)
                            .max(px(0.));
                        let current = offset.y.clamp(-axis_max, px(0.));
                        let new_offset = (current + delta.y).clamp(-axis_max, px(0.));
                        if new_offset == current {
                            // At the edge or no overflow: bubble to the parent.
                            return;
                        }

                        offset.y = new_offset;
                        scroll_handle.set_offset(offset);
                        cx.notify(view_id);
                        cx.stop_propagation();
                        return;
                    }

                    offset.x += delta.x;

                    // NOTE: `set_offset` does not clamp (clamping happens in
                    // the div's prepaint), so any non-zero horizontal-dominant
                    // delta passes this guard — even at the scroll edge the
                    // event is consumed rather than turned into a parent
                    // scroll.
                    if offset != scroll_handle.offset() {
                        scroll_handle.set_offset(offset);
                        cx.notify(view_id);
                        cx.stop_propagation();
                    }
                }
            });
        });
    }
}
