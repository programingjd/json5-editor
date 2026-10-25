use gpui::{
    Anchor, AnyElement, App, Bounds, Context, ElementId, FocusHandle, InteractiveElement as _,
    IntoElement, MouseButton, ParentElement, PathBuilder, Pixels, Point, RenderOnce,
    StyleRefinement, Styled, Window, canvas, point, prelude::FluentBuilder as _, px,
};
use std::{cell::Cell, rc::Rc};

use crate::{ActiveTheme as _, ThemeStyled as _};
use crate::{Selectable, StyledExt as _, styled::popover_ring, v_flex};
use gpui_base::Placement;
use gpui_base::Popover as BasePopover;
pub use gpui_base::PopoverState;

pub(crate) fn init(_: &mut App) {}

#[derive(IntoElement)]
pub struct Popover {
    id: ElementId,
    style: StyleRefinement,
    anchor: Anchor,
    offset: Option<Pixels>,
    arrow: bool,
    default_open: bool,
    open: Option<bool>,
    tracked_focus_handle: Option<FocusHandle>,
    trigger: Option<Box<dyn FnOnce(bool, &Window, &App) -> AnyElement + 'static>>,
    content: Option<
        Rc<
            dyn Fn(&mut PopoverState, &mut Window, &mut Context<PopoverState>) -> AnyElement
                + 'static,
        >,
    >,
    children: Vec<AnyElement>,

    trigger_style: Option<StyleRefinement>,
    mouse_button: MouseButton,
    appearance: bool,
    overlay_closable: bool,
    on_open_change: Option<Rc<dyn Fn(&bool, &mut Window, &mut App)>>,
}

impl Popover {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            style: StyleRefinement::default(),
            anchor: Anchor::TopLeft,
            offset: None,
            arrow: false,
            trigger: None,
            trigger_style: None,
            content: None,
            tracked_focus_handle: None,
            children: vec![],
            mouse_button: MouseButton::Left,
            appearance: true,
            overlay_closable: true,
            default_open: false,
            open: None,
            on_open_change: None,
        }
    }

    pub fn anchor(mut self, anchor: impl Into<Anchor>) -> Self {
        self.anchor = anchor.into();
        self
    }

    pub fn offset(mut self, offset: impl Into<Pixels>) -> Self {
        self.offset = Some(offset.into());
        self
    }

    pub fn arrow(mut self, arrow: bool) -> Self {
        self.arrow = arrow;
        self
    }

    pub fn mouse_button(mut self, mouse_button: MouseButton) -> Self {
        self.mouse_button = mouse_button;
        self
    }

    pub fn trigger<T>(mut self, trigger: T) -> Self
    where
        T: Selectable + IntoElement + 'static,
    {
        self.trigger = Some(Box::new(|is_open, _, _| {
            let open = trigger.is_open();
            trigger.open(open || is_open).into_any_element()
        }));
        self
    }

    pub fn default_open(mut self, open: bool) -> Self {
        self.default_open = open;
        self
    }

    pub fn open(mut self, open: bool) -> Self {
        self.open = Some(open);
        self
    }

    pub fn on_open_change<F>(mut self, callback: F) -> Self
    where
        F: Fn(&bool, &mut Window, &mut App) + 'static,
    {
        self.on_open_change = Some(Rc::new(callback));
        self
    }

    pub fn trigger_style(mut self, style: StyleRefinement) -> Self {
        self.trigger_style = Some(style);
        self
    }

    pub fn overlay_closable(mut self, closable: bool) -> Self {
        self.overlay_closable = closable;
        self
    }

    pub fn content<F, E>(mut self, content: F) -> Self
    where
        E: IntoElement,
        F: Fn(&mut PopoverState, &mut Window, &mut Context<PopoverState>) -> E + 'static,
    {
        self.content = Some(Rc::new(move |state, window, cx| {
            content(state, window, cx).into_any_element()
        }));
        self
    }

    pub fn appearance(mut self, appearance: bool) -> Self {
        self.appearance = appearance;
        self
    }

    pub fn track_focus(mut self, handle: &FocusHandle) -> Self {
        self.tracked_focus_handle = Some(handle.clone());
        self
    }
}

impl ParentElement for Popover {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for Popover {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl Popover {}

impl RenderOnce for Popover {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let anchor = self.anchor;
        let arrow_size = if self.arrow {
            window.rem_size() * 0.375
        } else {
            px(0.)
        };
        let offset = self.offset.unwrap_or(window.rem_size() * 0.25) + arrow_size;
        let resolved = Rc::new(Cell::new(None));
        let arrow_position = resolved.clone();
        let arrow = self.arrow;
        let background = self
            .style
            .background
            .as_ref()
            .and_then(gpui::Fill::color)
            .unwrap_or_else(|| cx.theme().popover.into());
        let ring = popover_ring(cx);
        let radius = cx.theme().radius;
        let appearance = self.appearance;
        let style = self.style;
        let children = self.children;
        let content = self.content;
        let trigger_style = self.trigger_style;

        BasePopover::new(self.id)
            .anchor(self.anchor)
            .offset(offset)
            .on_position(move |position, trigger| resolved.set(Some((position, trigger))))
            .mouse_button(self.mouse_button)
            .default_open(self.default_open)
            .overlay_closable(self.overlay_closable)
            .content(move |state, window, cx| {
                v_flex()
                    .id("content")
                    .occlude()
                    .tab_group()
                    .when(appearance, |this| this.popover_style(cx).p_3())
                    .when_some(content, |this, content| {
                        this.child((content)(state, window, cx))
                    })
                    .children(children)
                    .refine_style(&style)
                    .when(arrow, |this| {
                        this.child(
                            canvas(
                                |_, _, _| {},
                                move |bounds, _, window, _| {
                                    let Some((_, trigger)) = arrow_position.get() else {
                                        return;
                                    };
                                    let (side, target) = arrow_anchor(anchor, trigger);
                                    let trigger = Bounds::new(target, gpui::size(px(0.), px(0.)));
                                    let points =
                                        arrow_points(bounds, trigger, side, arrow_size, radius);
                                    let mut fill = PathBuilder::fill();
                                    fill.move_to(points[0]);
                                    fill.line_to(points[1]);
                                    fill.line_to(points[2]);
                                    fill.close();
                                    if let Ok(path) = fill.build() {
                                        window.paint_path(path, background);
                                    }
                                    // The triangle ends exactly at the surface edge.
                                    // Cover the ring and the antialiased base on both
                                    // sides of that edge before drawing its two slopes.
                                    window.paint_quad(gpui::fill(
                                        arrow_join_bounds(points, side, px(1.)),
                                        background,
                                    ));
                                    if appearance {
                                        let mut outline = PathBuilder::stroke(px(1.));
                                        outline.move_to(points[0]);
                                        outline.line_to(points[1]);
                                        outline.line_to(points[2]);
                                        if let Ok(path) = outline.build() {
                                            window.paint_path(path, ring);
                                        }
                                    }
                                },
                            )
                            .absolute()
                            .inset_0()
                            .size_full(),
                        )
                    })
            })
            .when_some(self.trigger, |this, trigger| this.trigger_with(trigger))
            .when_some(trigger_style, |this, style| this.refine_style(&style))
            .when_some(self.open, |this, open| this.open(open))
            .when_some(self.tracked_focus_handle, |this, handle| {
                this.track_focus(&handle)
            })
            .when_some(self.on_open_change, |this, callback| {
                this.on_open_change(move |open, window, cx| callback(open, window, cx))
            })
            .into_any_element()
    }
}

fn arrow_anchor(anchor: Anchor, trigger: Bounds<Pixels>) -> (Placement, Point<Pixels>) {
    match anchor {
        Anchor::TopLeft => (Placement::Bottom, trigger.bottom_left()),
        Anchor::TopCenter => (Placement::Bottom, trigger.bottom_center()),
        Anchor::TopRight => (Placement::Bottom, trigger.bottom_right()),
        Anchor::BottomLeft => (Placement::Top, trigger.origin),
        Anchor::BottomCenter => (Placement::Top, trigger.top_center()),
        Anchor::BottomRight => (Placement::Top, trigger.top_right()),
        Anchor::LeftCenter => (Placement::Right, trigger.right_center()),
        Anchor::RightCenter => (Placement::Left, trigger.left_center()),
    }
}

fn arrow_points(
    surface: Bounds<Pixels>,
    trigger: Bounds<Pixels>,
    side: Placement,
    depth: Pixels,
    radius: Pixels,
) -> [Point<Pixels>; 3] {
    let horizontal = side.is_horizontal();
    let (start, end, target) = if horizontal {
        (surface.top(), surface.bottom(), trigger.center().y)
    } else {
        (surface.left(), surface.right(), trigger.center().x)
    };
    let half = depth.min((end - start) * 0.5);
    let inset = (radius + half).min((end - start) * 0.5);
    let center = target.clamp(start + inset, end - inset);
    match side {
        Placement::Bottom => [
            point(center - half, surface.top()),
            point(center, surface.top() - depth),
            point(center + half, surface.top()),
        ],
        Placement::Top => [
            point(center - half, surface.bottom()),
            point(center, surface.bottom() + depth),
            point(center + half, surface.bottom()),
        ],
        Placement::Right => [
            point(surface.left(), center - half),
            point(surface.left() - depth, center),
            point(surface.left(), center + half),
        ],
        Placement::Left => [
            point(surface.right(), center - half),
            point(surface.right() + depth, center),
            point(surface.right(), center + half),
        ],
    }
}

fn arrow_join_bounds(
    points: [Point<Pixels>; 3],
    side: Placement,
    stroke: Pixels,
) -> Bounds<Pixels> {
    // Inset by the stroke width so the patch remains inside the triangle's
    // slopes at the outer edge of the ring, including on very small surfaces.
    if side.is_horizontal() {
        let inset = stroke.min((points[2].y - points[0].y) * 0.5);
        Bounds::from_corners(
            point(points[0].x - stroke, points[0].y + inset),
            point(points[2].x + stroke, points[2].y - inset),
        )
    } else {
        let inset = stroke.min((points[2].x - points[0].x) * 0.5);
        Bounds::from_corners(
            point(points[0].x + inset, points[0].y - stroke),
            point(points[2].x - inset, points[2].y + stroke),
        )
    }
}
