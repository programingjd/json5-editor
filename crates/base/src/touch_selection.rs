//! Touch selection geometry shared by the input engine and window text
//! selection.
//!
//! A touch selection is one made by a long press. It differs from a pointer
//! selection in what the user needs afterwards: a grab handle at each end to
//! adjust it, since a finger cannot hover an I-beam, and an edit menu next to
//! it, since there is no right click. Base owns the gesture, the handle drag
//! and the menu lifecycle; this module carries what a presentation layer needs
//! to draw them. The handles and the menu themselves are drawn by the styled
//! layer, which also decides how large a handle's touch target is.

use gpui::{Bounds, Hsla, Pixels, Point, Window, fill, point, px, size};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SelectionEdge {
    Start,

    End,
}

impl SelectionEdge {
    pub const fn opposite(self) -> Self {
        match self {
            Self::Start => Self::End,
            Self::End => Self::Start,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TouchSelectionSnapshot {
    start: Bounds<Pixels>,
    end: Bounds<Pixels>,

    start_visible: bool,
    end_visible: bool,
    menu_open: bool,
    dragging: Option<SelectionEdge>,
}

impl TouchSelectionSnapshot {
    pub(crate) const fn new(start: Bounds<Pixels>, end: Bounds<Pixels>) -> Self {
        Self {
            start,
            end,
            start_visible: true,
            end_visible: true,
            menu_open: false,
            dragging: None,
        }
    }

    pub(crate) const fn with_edge_visible(mut self, edge: SelectionEdge, visible: bool) -> Self {
        match edge {
            SelectionEdge::Start => self.start_visible = visible,
            SelectionEdge::End => self.end_visible = visible,
        }
        self
    }

    pub(crate) const fn with_menu_open(mut self, menu_open: bool) -> Self {
        self.menu_open = menu_open;
        self
    }

    pub(crate) const fn with_dragging(mut self, dragging: Option<SelectionEdge>) -> Self {
        self.dragging = dragging;
        self
    }

    pub const fn start(&self) -> Bounds<Pixels> {
        self.start
    }

    pub const fn end(&self) -> Bounds<Pixels> {
        self.end
    }

    pub const fn edge(&self, edge: SelectionEdge) -> Bounds<Pixels> {
        match edge {
            SelectionEdge::Start => self.start,
            SelectionEdge::End => self.end,
        }
    }

    pub const fn is_edge_visible(&self, edge: SelectionEdge) -> bool {
        match edge {
            SelectionEdge::Start => self.start_visible,
            SelectionEdge::End => self.end_visible,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    pub fn bounds(&self) -> Option<Bounds<Pixels>> {
        match (self.start_visible, self.end_visible) {
            (true, true) => Some(self.start.union(&self.end)),
            (true, false) => Some(self.start),
            (false, true) => Some(self.end),
            (false, false) => None,
        }
    }

    pub const fn is_menu_open(&self) -> bool {
        self.menu_open
    }

    pub const fn dragging(&self) -> Option<SelectionEdge> {
        self.dragging
    }
}

pub struct TouchHandle;

impl TouchHandle {
    pub const HIT_SIZE: Pixels = px(44.);

    pub const KNOB_SIZE: Pixels = px(10.);

    pub const BAR_WIDTH: Pixels = px(2.);

    pub const EXTENT: Pixels = px(12.);

    pub fn hit_bounds(edge: SelectionEdge, caret: Bounds<Pixels>) -> Bounds<Pixels> {
        let top = match edge {
            SelectionEdge::Start => caret.top() - Self::EXTENT,
            SelectionEdge::End => caret.top(),
        };
        Bounds::new(
            point(caret.left() - Self::HIT_SIZE / 2., top),
            size(Self::HIT_SIZE, caret.size.height + Self::EXTENT),
        )
    }

    pub fn paint(edge: SelectionEdge, caret: Bounds<Pixels>, color: Hsla, window: &mut Window) {
        let bar = Bounds::new(
            point(caret.left() - Self::BAR_WIDTH / 2., caret.top()),
            size(Self::BAR_WIDTH, caret.size.height),
        );
        let knob_top = match edge {
            SelectionEdge::Start => caret.top() - Self::KNOB_SIZE,
            SelectionEdge::End => caret.bottom(),
        };
        let knob = Bounds::new(
            point(caret.left() - Self::KNOB_SIZE / 2., knob_top),
            size(Self::KNOB_SIZE, Self::KNOB_SIZE),
        );
        window.paint_quad(fill(bar, color));
        window.paint_quad(fill(knob, color).corner_radii(Self::KNOB_SIZE / 2.));
    }
}

pub(crate) fn caret_line_box(position: Point<Pixels>, line_height: Pixels) -> Bounds<Pixels> {
    Bounds::new(position, size(px(0.), line_height))
}

pub(crate) fn caret_in_view(caret: Bounds<Pixels>, viewport: Bounds<Pixels>) -> bool {
    caret.bottom() > viewport.top()
        && caret.top() < viewport.bottom()
        && caret.left() >= viewport.left()
        && caret.left() <= viewport.right()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct EdgeDrag {
    edge: SelectionEdge,
    offset: Point<Pixels>,
}

impl EdgeDrag {
    pub(crate) fn begin(edge: SelectionEdge, caret: Bounds<Pixels>, finger: Point<Pixels>) -> Self {
        Self {
            edge,
            offset: caret.center() - finger,
        }
    }

    pub(crate) const fn edge(&self) -> SelectionEdge {
        self.edge
    }

    pub(crate) fn set_edge(&mut self, edge: SelectionEdge) {
        self.edge = edge;
    }

    pub(crate) fn text_position(&self, finger: Point<Pixels>) -> Point<Pixels> {
        point(finger.x + self.offset.x, finger.y + self.offset.y)
    }
}
