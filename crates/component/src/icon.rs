use std::sync::Arc;

use crate::{ActiveTheme, Sizable, Size};
use gpui::{
    AnyElement, App, AppContext, Context, Entity, Hsla, IntoElement, Pixels, Radians, Render,
    RenderOnce, SharedString, StyleRefinement, Styled, Svg, Transformation, Window,
    prelude::FluentBuilder as _, svg,
};
pub use gpui_kit_assets::IconNamed;

// Preserve the original enum (including exhaustive matches and inherent view)
// while the complete, shared catalog is owned by gpui-kit-assets.
macro_rules! component_icon_names {
    ($($name:ident => $path:literal,)*) => {

        #[derive(Clone, IntoElement)]
        pub enum IconName {
            $($name,)*
        }

        impl From<IconName> for gpui_kit_assets::IconName {
            fn from(name: IconName) -> Self {
                match name {
                    $(IconName::$name => Self::$name,)*
                }
            }
        }

        impl IconNamed for IconName {
            fn path(self) -> SharedString {
                match self { $(Self::$name => $path,)* }.into()
            }
        }
    };
}

gpui_kit_assets::__component_icon_names!(component_icon_names);

impl IconName {
    pub fn view(self, cx: &mut App) -> Entity<Icon> {
        Icon::build(self).view(cx)
    }
}

impl From<IconName> for AnyElement {
    fn from(name: IconName) -> Self {
        Icon::build(name).into_any_element()
    }
}

impl RenderOnce for IconName {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        Icon::build(self)
    }
}

impl<T: IconNamed> From<T> for Icon {
    fn from(value: T) -> Self {
        Icon::build(value)
    }
}

pub trait IconNameExt {
    fn view(self, cx: &mut App) -> Entity<Icon>;
}

impl IconNameExt for gpui_kit_assets::IconName {
    fn view(self, cx: &mut App) -> Entity<Icon> {
        Icon::build(self).view(cx)
    }
}

#[derive(Clone)]
pub(crate) enum IconSource {
    Path(SharedString),
    Data(Arc<[u8]>),
}

#[derive(Clone, IntoElement)]
pub struct Icon {
    style: StyleRefinement,
    source: IconSource,
    text_color: Option<Hsla>,
    size: Option<Size>,
    transformation: Option<Transformation>,
}

impl Default for Icon {
    fn default() -> Self {
        Self {
            style: StyleRefinement::default(),
            source: IconSource::Path("".into()),
            text_color: None,
            size: None,
            transformation: None,
        }
    }
}

impl Icon {
    pub fn new(icon: impl Into<Icon>) -> Self {
        icon.into()
    }

    fn build(name: impl IconNamed) -> Self {
        Self::default().path(name.path())
    }

    pub fn path(mut self, path: impl Into<SharedString>) -> Self {
        self.source = IconSource::Path(path.into());
        self
    }

    pub fn data(mut self, data: &[u8]) -> Self {
        self.source = IconSource::Data(Arc::from(data));
        self
    }

    pub fn view(self, cx: &mut App) -> Entity<Icon> {
        cx.new(|_| self)
    }

    pub fn transform(mut self, transformation: gpui::Transformation) -> Self {
        self.transformation = Some(transformation);
        self
    }

    pub fn empty() -> Self {
        Self::default()
    }

    pub fn rotate(mut self, radians: impl Into<Radians>) -> Self {
        self.transformation = Some(Transformation::rotate(radians));
        self
    }

    fn into_svg(self, text_size: Pixels, fallback_color: Hsla) -> Svg {
        let text_color = self.text_color.unwrap_or(fallback_color);
        let has_base_size = self.style.size.width.is_some() || self.style.size.height.is_some();

        svg()
            .map(|mut this| {
                *this.style() = self.style;
                this
            })
            .flex_shrink_0()
            .text_color(text_color)
            .when(!has_base_size, |this| this.size(text_size))
            .when_some(self.size, |this, size| match size {
                Size::Size(px) => this.size(px),
                Size::XSmall => this.size_3(),
                Size::Small => this.size_3p5(),
                Size::Medium => this.size_4(),
                Size::Large => this.size_6(),
            })
            .map(|this| match self.source {
                IconSource::Path(path) => this.path(path),
                IconSource::Data(data) => this.data(&data),
            })
            .when_some(self.transformation, |this, transformation| {
                this.with_transformation(transformation)
            })
    }
}

impl Styled for Icon {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }

    fn text_color(mut self, color: impl Into<Hsla>) -> Self {
        self.text_color = Some(color.into());
        self
    }
}

impl Sizable for Icon {
    fn with_size(mut self, size: impl Into<Size>) -> Self {
        self.size = Some(size.into());
        self
    }
}

impl RenderOnce for Icon {
    fn render(self, window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let text_size = window.text_style().font_size.to_pixels(window.rem_size());
        self.into_svg(text_size, window.text_style().color)
    }
}

impl From<Icon> for AnyElement {
    fn from(val: Icon) -> Self {
        val.into_any_element()
    }
}

impl Render for Icon {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let text_size = window.text_style().font_size.to_pixels(window.rem_size());
        self.clone().into_svg(text_size, cx.theme().foreground)
    }
}
