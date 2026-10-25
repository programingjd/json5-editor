pub use crate::component_traits::{Collapsible, Disableable, Selectable};
pub use crate::sizing::{Sizable, Size, StyleSized};
use gpui::{
    App, BoxShadow, Corners, Edges, Hsla, InteractiveElement as _, ParentElement, Pixels,
    StyleRefinement, Styled, Window, div, hsla, prelude::FluentBuilder as _, px,
};
pub use gpui_base::{FocusableExt, RoleOverride, StyledExt, box_shadow, h_flex, v_flex};

use crate::ActiveTheme as _;

const FOCUS_RING_WIDTH: Pixels = px(3.);
const FOCUS_RING_OPACITY: f32 = 0.5;

const FOCUS_LINE_GAP: f32 = 0.125;

const SURFACE_SHADOW_INK: f32 = 0.1;

const POPOVER_RING_INK: f32 = 0.1;

pub(crate) fn popover_ring(cx: &App) -> Hsla {
    cx.theme().foreground.alpha(POPOVER_RING_INK)
}

pub(crate) fn popover_shadow(ring: Hsla, strength: f32) -> Vec<BoxShadow> {
    let strength = strength.clamp(0., 1.);
    let ink = hsla(0., 0., 0., SURFACE_SHADOW_INK * strength);
    vec![
        // The ring, sitting in the 1px band outside the surface. No blur, so it
        // takes the shader's crisp path rather than the gaussian one.
        BoxShadow::new(px(0.), px(0.), ring.alpha(ring.a * strength))
            .blur_radius(px(0.))
            .spread_radius(px(1.)),
        BoxShadow::new(px(0.), px(4.), ink)
            .blur_radius(px(3.))
            .spread_radius(px(-1.)),
        BoxShadow::new(px(0.), px(2.), ink)
            .blur_radius(px(2.))
            .spread_radius(px(-2.)),
    ]
}

pub trait ThemeStyled: Styled + Sized {
    fn focus_ring_style(self, window: &Window, cx: &App) -> Self
    where
        Self: ParentElement;

    fn popover_style(self, cx: &App) -> Self;

    fn rounded_full_style(self, cx: &App) -> Self {
        self.rounded(cx.theme().radius_full())
    }
}

impl<T: Styled + Sized> ThemeStyled for T {
    fn focus_ring_style(self, window: &Window, cx: &App) -> Self
    where
        Self: ParentElement,
    {
        focus_style(self, FocusLine::Edge, window, cx)
    }

    fn popover_style(self, cx: &App) -> Self {
        let theme = cx.theme();
        // No border: the edge is the ring inside `popover_shadow`, which is how
        // shadcn draws it and the only way the shadow can show through it.
        self.font_family(theme.font_family.clone())
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .shadow(popover_shadow(popover_ring(cx), 1.))
            .rounded(theme.radius)
    }
}

fn border_widths(style: &StyleRefinement, rem_size: Pixels) -> Edges<Pixels> {
    let width = |value: Option<gpui::AbsoluteLength>| {
        value.map(|v| v.to_pixels(rem_size)).unwrap_or_default()
    };
    let widths = &style.border_widths;
    Edges {
        top: width(widths.top),
        bottom: width(widths.bottom),
        left: width(widths.left),
        right: width(widths.right),
    }
}

fn corner_radii(style: &StyleRefinement, rem_size: Pixels) -> Corners<Pixels> {
    let radius = |value: Option<gpui::AbsoluteLength>| {
        value.map(|v| v.to_pixels(rem_size)).unwrap_or_default()
    };
    let radii = &style.corner_radii;
    Corners {
        top_left: radius(radii.top_left),
        top_right: radius(radii.top_right),
        bottom_left: radius(radii.bottom_left),
        bottom_right: radius(radii.bottom_right),
    }
}

fn corner_radii_refinement(radius: Corners<Pixels>) -> StyleRefinement {
    let mut style = StyleRefinement::default();
    style.corner_radii.top_left = Some(radius.top_left.into());
    style.corner_radii.top_right = Some(radius.top_right.into());
    style.corner_radii.bottom_left = Some(radius.bottom_left.into());
    style.corner_radii.bottom_right = Some(radius.bottom_right.into());
    style
}

#[derive(Clone, Copy)]
pub(crate) enum FocusLine {
    Edge,

    Inside(Hsla),

    Outside,
}

pub(crate) fn focus_style<T: Styled + ParentElement>(
    mut element: T,
    line: FocusLine,
    window: &Window,
    cx: &App,
) -> T {
    let theme = cx.theme();
    if theme.focus_ring {
        return focus_ring(
            element.border_color(theme.ring),
            window,
            theme.ring.alpha(FOCUS_RING_OPACITY),
        );
    }

    // The ring is painted outside the border, so a clipping ancestor cuts it
    // off. An application whose layout clips heavily turns it off in the theme
    // and keeps the tinted border, which takes no space.
    let rem_size = window.rem_size();
    if border_widths(element.style(), rem_size).any(|width| *width > Pixels::ZERO) {
        return element.border_color(theme.ring);
    }

    let gap = rem_size * FOCUS_LINE_GAP;
    let (color, inset) = match line {
        FocusLine::Edge => (theme.ring, Pixels::ZERO),
        FocusLine::Inside(color) => (color, gap),
        FocusLine::Outside => (theme.ring, -gap),
    };
    // Shrinking or growing the box by `inset` keeps the line concentric with
    // the element's own corners.
    let radius =
        corner_radii(element.style(), rem_size).map(|value| (*value - inset).max(Pixels::ZERO));
    element.child(
        div()
            .when(cfg!(test), |this| {
                this.debug_selector(|| "focus-ring".into())
            })
            .flex_none()
            .absolute()
            .top(inset)
            .left(inset)
            .right(inset)
            .bottom(inset)
            .border_1()
            .border_color(color)
            .refine_style(&corner_radii_refinement(radius)),
    )
}

pub(crate) fn focus_ring<T: Styled + ParentElement>(
    mut element: T,
    window: &Window,
    color: Hsla,
) -> T {
    let rem_size = window.rem_size();
    let border_widths = border_widths(element.style(), rem_size);
    let radius = corner_radii(element.style(), rem_size).map(|value| *value + FOCUS_RING_WIDTH);
    let ring_style = corner_radii_refinement(radius);
    let inset = FOCUS_RING_WIDTH;

    element.child(
        div()
            .when(cfg!(test), |this| {
                this.debug_selector(|| "focus-ring".into())
            })
            .flex_none()
            .absolute()
            .top(-(inset + border_widths.top))
            .left(-(inset + border_widths.left))
            .right(-(inset + border_widths.right))
            .bottom(-(inset + border_widths.bottom))
            .border(FOCUS_RING_WIDTH)
            .border_color(color)
            .refine_style(&ring_style),
    )
}
