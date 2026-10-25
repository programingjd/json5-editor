//! Shared popup positioning.
//!
//! Every anchored surface in Base resolves its position here so that flipping,
//! alignment, and viewport clamping cannot drift apart between popups,
//! tooltips, and menus.

use gpui::{
    Anchor, AnyElement, App, Bounds, Decorations, Display, Edges, Element, GlobalElementId,
    Half as _, HitboxBehavior, InspectorElementId, IntoElement, LayoutId, ParentElement, Pixels,
    Point, Position, Size, Style, Window, point, px,
};

use crate::Placement;
use std::{cell::Cell, rc::Rc};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Align {
    Start,

    #[default]
    Center,

    End,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Strategy {
    Corner {
        anchor: Anchor,
        position: Point<Pixels>,
    },

    Side {
        trigger_bounds: Bounds<Pixels>,
        placement: Option<Placement>,
        align: Align,
        offset: Pixels,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedPosition {
    pub bounds: Bounds<Pixels>,

    pub placement: Option<Placement>,
}

pub struct Positioner {
    strategy: Strategy,
    corner_position: Option<Rc<Cell<Point<Pixels>>>>,
    on_position: Option<Box<dyn Fn(ResolvedPosition)>>,
    margin: Pixels,
    occlude: bool,
    children: Vec<AnyElement>,
}

#[doc(hidden)]
pub struct PositionerState {
    child_layout_ids: Vec<LayoutId>,
}

impl Positioner {
    pub fn side(trigger_bounds: Bounds<Pixels>) -> Self {
        Self {
            strategy: Strategy::Side {
                trigger_bounds,
                placement: None,
                align: Align::Center,
                offset: px(0.),
            },
            corner_position: None,
            on_position: None,
            margin: px(4.),
            occlude: false,
            children: Vec::new(),
        }
    }

    pub fn corner(anchor: Anchor, position: Point<Pixels>) -> Self {
        Self {
            strategy: Strategy::Corner { anchor, position },
            corner_position: None,
            on_position: None,
            margin: px(4.),
            occlude: false,
            children: Vec::new(),
        }
    }

    pub fn position(mut self, position: Point<Pixels>) -> Self {
        if let Strategy::Corner {
            position: current, ..
        } = &mut self.strategy
        {
            *current = position;
        }
        self
    }

    pub fn placement(mut self, placement: Placement) -> Self {
        if let Strategy::Side {
            placement: slot, ..
        } = &mut self.strategy
        {
            *slot = Some(placement);
        }
        self
    }

    pub fn align(mut self, align: Align) -> Self {
        if let Strategy::Side { align: slot, .. } = &mut self.strategy {
            *slot = align;
        }
        self
    }

    pub fn offset(mut self, offset: Pixels) -> Self {
        if let Strategy::Side { offset: slot, .. } = &mut self.strategy {
            *slot = offset;
        }
        self
    }

    // Read after trigger prepaint so an open popup follows a moving trigger.
    pub(crate) fn tracked_corner_position(mut self, position: Rc<Cell<Point<Pixels>>>) -> Self {
        self.corner_position = Some(position);
        self
    }

    pub fn on_position(mut self, callback: impl Fn(ResolvedPosition) + 'static) -> Self {
        self.on_position = Some(Box::new(callback));
        self
    }

    pub fn occlude(mut self) -> Self {
        self.occlude = true;
        self
    }

    pub fn margin(mut self, margin: Pixels) -> Self {
        self.margin = margin;
        self
    }
}

fn frame_insets(decorations: Decorations, client_inset: Pixels) -> Edges<Pixels> {
    match decorations {
        Decorations::Server => Edges::default(),
        Decorations::Client { tiling } => Edges {
            top: if tiling.top { px(0.) } else { client_inset },
            right: if tiling.right { px(0.) } else { client_inset },
            bottom: if tiling.bottom { px(0.) } else { client_inset },
            left: if tiling.left { px(0.) } else { client_inset },
        },
    }
}

fn resolve(
    strategy: Strategy,
    popup_size: Size<Pixels>,
    viewport_size: Size<Pixels>,
    margin: Edges<Pixels>,
) -> ResolvedPosition {
    match strategy {
        Strategy::Corner { anchor, position } => ResolvedPosition {
            bounds: clamp(
                Bounds::from_anchor_and_size(anchor, position, popup_size),
                viewport_size,
                margin,
            ),
            placement: None,
        },
        Strategy::Side {
            trigger_bounds,
            placement,
            align,
            offset,
        } => {
            let placement =
                resolve_placement(trigger_bounds, popup_size, viewport_size, margin, placement);
            let origin = side_origin(trigger_bounds, popup_size, placement, align, offset);
            ResolvedPosition {
                bounds: clamp(Bounds::new(origin, popup_size), viewport_size, margin),
                placement: Some(placement),
            }
        }
    }
}

fn resolve_placement(
    trigger_bounds: Bounds<Pixels>,
    popup_size: Size<Pixels>,
    viewport_size: Size<Pixels>,
    margin: Edges<Pixels>,
    preferred: Option<Placement>,
) -> Placement {
    let right_limit = (viewport_size.width - margin.right).max(margin.left);
    let bottom_limit = (viewport_size.height - margin.bottom).max(margin.top);
    let available_left = (trigger_bounds.left() - margin.left).max(px(0.));
    let available_right = (right_limit - trigger_bounds.right()).max(px(0.));
    let available_above = (trigger_bounds.top() - margin.top).max(px(0.));
    let available_below = (bottom_limit - trigger_bounds.bottom()).max(px(0.));

    match preferred {
        Some(Placement::Right) if popup_size.width <= available_right => Placement::Right,
        Some(Placement::Right) if popup_size.width <= available_left => Placement::Left,
        Some(Placement::Right) if available_right >= available_left => Placement::Right,
        Some(Placement::Right) => Placement::Left,
        Some(Placement::Left) if popup_size.width <= available_left => Placement::Left,
        Some(Placement::Left) if popup_size.width <= available_right => Placement::Right,
        Some(Placement::Left) if available_left >= available_right => Placement::Left,
        Some(Placement::Left) => Placement::Right,
        Some(Placement::Bottom) if popup_size.height <= available_below => Placement::Bottom,
        Some(Placement::Bottom) if popup_size.height <= available_above => Placement::Top,
        Some(Placement::Bottom) if available_below >= available_above => Placement::Bottom,
        Some(Placement::Bottom) => Placement::Top,
        Some(Placement::Top) | None if popup_size.height <= available_above => Placement::Top,
        Some(Placement::Top) | None if popup_size.height <= available_below => Placement::Bottom,
        Some(Placement::Top) | None if available_below >= available_above => Placement::Bottom,
        Some(Placement::Top) | None => Placement::Top,
    }
}

fn side_origin(
    trigger_bounds: Bounds<Pixels>,
    popup_size: Size<Pixels>,
    placement: Placement,
    align: Align,
    offset: Pixels,
) -> Point<Pixels> {
    let aligned_x = match align {
        Align::Start => trigger_bounds.left(),
        Align::Center => trigger_bounds.center().x - popup_size.width.half(),
        Align::End => trigger_bounds.right() - popup_size.width,
    };
    let aligned_y = match align {
        Align::Start => trigger_bounds.top(),
        Align::Center => trigger_bounds.center().y - popup_size.height.half(),
        Align::End => trigger_bounds.bottom() - popup_size.height,
    };

    match placement {
        Placement::Top => point(aligned_x, trigger_bounds.top() - popup_size.height - offset),
        Placement::Bottom => point(aligned_x, trigger_bounds.bottom() + offset),
        Placement::Left => point(trigger_bounds.left() - popup_size.width - offset, aligned_y),
        Placement::Right => point(trigger_bounds.right() + offset, aligned_y),
    }
}

fn clamp(
    mut bounds: Bounds<Pixels>,
    viewport_size: Size<Pixels>,
    margin: Edges<Pixels>,
) -> Bounds<Pixels> {
    let right_limit = (viewport_size.width - margin.right).max(margin.left);
    let bottom_limit = (viewport_size.height - margin.bottom).max(margin.top);

    if bounds.right() > right_limit {
        bounds.origin.x -= bounds.right() - right_limit;
    }
    if bounds.left() < margin.left {
        bounds.origin.x = margin.left;
    }
    if bounds.bottom() > bottom_limit {
        bounds.origin.y -= bounds.bottom() - bottom_limit;
    }
    if bounds.top() < margin.top {
        bounds.origin.y = margin.top;
    }

    bounds
}

impl ParentElement for Positioner {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Element for Positioner {
    type RequestLayoutState = PositionerState;
    type PrepaintState = ();

    fn id(&self) -> Option<gpui::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let child_layout_ids = self
            .children
            .iter_mut()
            .map(|child| child.request_layout(window, cx))
            .collect::<Vec<_>>();
        let layout_id = window.request_layout(
            Style {
                position: Position::Absolute,
                display: Display::Flex,
                ..Style::default()
            },
            child_layout_ids.iter().copied(),
            cx,
        );

        (layout_id, PositionerState { child_layout_ids })
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if request_layout.child_layout_ids.is_empty() {
            return;
        }

        let mut child_min = point(Pixels::MAX, Pixels::MAX);
        let mut child_max = Point::default();
        for child_layout_id in &request_layout.child_layout_ids {
            let child_bounds = window.layout_bounds(*child_layout_id);
            child_min = child_min.min(&child_bounds.origin);
            child_max = child_max.max(&child_bounds.bottom_right());
        }

        let popup_size = (child_max - child_min).into();
        let frame = frame_insets(
            window.window_decorations(),
            window.client_inset().unwrap_or(px(0.)),
        );
        let mut strategy = self.strategy;
        if let (Strategy::Corner { position, .. }, Some(tracked)) =
            (&mut strategy, &self.corner_position)
        {
            *position = tracked.get();
        }
        let position = resolve(
            strategy,
            popup_size,
            window.viewport_size(),
            frame.map(|inset| *inset + self.margin),
        );
        if let Some(callback) = &self.on_position {
            callback(position);
        }
        // Ahead of the children so it blocks what is behind the popup without
        // blocking the popup's own content.
        if self.occlude {
            window.insert_hitbox(position.bounds, HitboxBehavior::BlockMouse);
        }

        let offset = position.bounds.origin - bounds.origin;
        let offset = point(offset.x.round(), offset.y.round());

        window.with_element_offset(offset, |window| {
            for child in &mut self.children {
                child.prepaint(window, cx);
            }
        });
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        for child in &mut self.children {
            child.paint(window, cx);
        }
    }
}

impl IntoElement for Positioner {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}
