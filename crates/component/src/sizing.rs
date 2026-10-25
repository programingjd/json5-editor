use gpui::{Edges, Pixels, Styled, px};
use serde::{Deserialize, Serialize};

#[derive(Clone, Default, Copy, PartialEq, Eq, Debug, Deserialize, Serialize)]
pub enum Size {
    Size(Pixels),
    XSmall,
    Small,
    #[default]
    Medium,
    Large,
}

impl Size {
    fn as_f32(&self) -> f32 {
        match self {
            Size::Size(val) => val.as_f32(),
            Size::XSmall => 0.,
            Size::Small => 1.,
            Size::Medium => 2.,
            Size::Large => 3.,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Size::XSmall => "xs",
            Size::Small => "sm",
            Size::Medium => "md",
            Size::Large => "lg",
            Size::Size(_) => "custom",
        }
    }

    pub fn from_str(size: &str) -> Self {
        match size.to_lowercase().as_str() {
            "xs" | "xsmall" => Size::XSmall,
            "sm" | "small" => Size::Small,
            "md" | "medium" => Size::Medium,
            "lg" | "large" => Size::Large,
            _ => Size::Medium,
        }
    }

    #[inline]
    pub fn table_row_height(&self) -> Pixels {
        match self {
            Size::Size(size) => *size,
            Size::XSmall => px(26.),
            Size::Small => px(30.),
            Size::Large => px(40.),
            _ => px(32.),
        }
    }

    #[inline]
    pub fn table_cell_padding(&self) -> Edges<Pixels> {
        match self {
            Size::XSmall => Edges {
                top: px(2.),
                bottom: px(2.),
                left: px(4.),
                right: px(4.),
            },
            Size::Small => Edges {
                top: px(3.),
                bottom: px(3.),
                left: px(6.),
                right: px(6.),
            },
            Size::Large => Edges {
                top: px(8.),
                bottom: px(8.),
                left: px(12.),
                right: px(12.),
            },
            _ => Edges {
                top: px(4.),
                bottom: px(4.),
                left: px(8.),
                right: px(8.),
            },
        }
    }

    pub fn smaller(&self) -> Self {
        match self {
            Size::XSmall => Size::XSmall,
            Size::Small => Size::XSmall,
            Size::Medium => Size::Small,
            Size::Large => Size::Medium,
            Size::Size(val) => Size::Size(*val * 0.2),
        }
    }

    pub fn larger(&self) -> Self {
        match self {
            Size::XSmall => Size::Small,
            Size::Small => Size::Medium,
            Size::Medium => Size::Large,
            Size::Large => Size::Large,
            Size::Size(val) => Size::Size(*val * 1.2),
        }
    }

    pub fn max(&self, other: Self) -> Self {
        match (self, other) {
            (Size::Size(a), Size::Size(b)) => Size::Size(px(a.as_f32().min(b.as_f32()))),
            (Size::Size(a), _) => Size::Size(*a),
            (_, Size::Size(b)) => Size::Size(b),
            (a, b) if a.as_f32() < b.as_f32() => *a,
            _ => other,
        }
    }

    pub fn min(&self, other: Self) -> Self {
        match (self, other) {
            (Size::Size(a), Size::Size(b)) => Size::Size(px(a.as_f32().max(b.as_f32()))),
            (Size::Size(a), _) => Size::Size(*a),
            (_, Size::Size(b)) => Size::Size(b),
            (a, b) if a.as_f32() > b.as_f32() => *a,
            _ => other,
        }
    }

    pub fn input_px(&self) -> Pixels {
        match self {
            Self::Large => px(12.),
            Self::Medium => px(10.),
            Self::Small => px(8.),
            Self::XSmall => px(4.),
            _ => px(8.),
        }
    }

    pub fn input_py(&self) -> Pixels {
        match self {
            Size::Large => px(10.),
            Size::Medium => px(8.),
            Size::Small => px(2.),
            Size::XSmall => px(0.),
            _ => px(2.),
        }
    }

    pub(crate) fn list_px(&self) -> Pixels {
        self.input_px() + px(1.) - DROPDOWN_LIST_PADDING
    }
}

pub(crate) const DROPDOWN_LIST_PADDING: Pixels = px(4.);

impl From<Pixels> for Size {
    fn from(size: Pixels) -> Self {
        Size::Size(size)
    }
}

#[allow(patterns_in_fns_without_body)]
pub trait Sizable: Sized {
    fn with_size(mut self, size: impl Into<Size>) -> Self;

    #[doc(hidden)]
    fn prepare_for_toolbar(self) -> Self {
        self
    }

    #[inline(always)]
    fn xsmall(self) -> Self {
        self.with_size(Size::XSmall)
    }

    #[inline(always)]
    fn small(self) -> Self {
        self.with_size(Size::Small)
    }

    #[inline(always)]
    fn large(self) -> Self {
        self.with_size(Size::Large)
    }
}

#[allow(unused)]
pub trait StyleSized<T: Styled> {
    fn input_text_size(self, size: Size) -> Self;
    fn input_size(self, size: Size) -> Self;
    fn input_pl(self, size: Size) -> Self;
    fn input_pr(self, size: Size) -> Self;
    fn input_px(self, size: Size) -> Self;
    fn input_py(self, size: Size) -> Self;
    fn input_h(self, size: Size) -> Self;
    fn list_size(self, size: Size) -> Self;
    fn list_px(self, size: Size) -> Self;
    fn list_py(self, size: Size) -> Self;

    fn size_with(self, size: Size) -> Self;

    fn table_cell_size(self, size: Size) -> Self;
    fn button_text_size(self, size: Size) -> Self;
}

impl<T: Styled> StyleSized<T> for T {
    #[inline]
    fn input_text_size(self, size: Size) -> Self {
        match size {
            Size::XSmall => self.text_xs(),
            Size::Small => self.text_sm(),
            Size::Medium => self.text_sm(),
            Size::Large => self.text_base(),
            Size::Size(size) => self.text_size(size * 0.875),
        }
    }

    #[inline]
    fn input_size(self, size: Size) -> Self {
        self.input_px(size).input_py(size).input_h(size)
    }

    #[inline]
    fn input_pl(self, size: Size) -> Self {
        self.pl(size.input_px())
    }

    #[inline]
    fn input_pr(self, size: Size) -> Self {
        self.pr(size.input_px())
    }

    #[inline]
    fn input_px(self, size: Size) -> Self {
        self.px(size.input_px())
    }

    #[inline]
    fn input_py(self, size: Size) -> Self {
        self.py(size.input_py())
    }

    #[inline]
    fn input_h(self, size: Size) -> Self {
        match size {
            Size::Large => self.h_11(),
            Size::Medium => self.h_8(),
            Size::Small => self.h_6(),
            Size::XSmall => self.h_5(),
            _ => self.h_6(),
        }
    }

    #[inline]
    fn list_size(self, size: Size) -> Self {
        self.list_px(size).list_py(size).input_text_size(size)
    }

    #[inline]
    fn list_px(self, size: Size) -> Self {
        self.px(size.list_px())
    }

    #[inline]
    fn list_py(self, size: Size) -> Self {
        match size {
            Size::Large => self.py_2(),
            Size::Medium => self.py_1(),
            Size::Small => self.py_0p5(),
            _ => self.py_1(),
        }
    }

    #[inline]
    fn size_with(self, size: Size) -> Self {
        match size {
            Size::Large => self.size_11(),
            Size::Medium => self.size_8(),
            Size::Small => self.size_5(),
            Size::XSmall => self.size_4(),
            Size::Size(size) => self.size(size),
        }
    }

    #[inline]
    fn table_cell_size(self, size: Size) -> Self {
        let padding = size.table_cell_padding();
        match size {
            Size::XSmall => self.text_xs(),
            Size::Small | Size::Medium => self.text_sm(),
            Size::Large => self.text_base(),
            _ => self,
        }
        .pl(padding.left)
        .pr(padding.right)
        .pt(padding.top)
        .pb(padding.bottom)
    }

    fn button_text_size(self, size: Size) -> Self {
        match size {
            Size::XSmall => self.text_xs(),
            Size::Small | Size::Medium => self.text_sm(),
            _ => self.text_base(),
        }
    }
}
