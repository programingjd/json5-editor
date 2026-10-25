use gpui::{App, Global};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{ScrollbarMode, ScrollbarMotion, ScrollbarStyles, SemanticThemeTokens};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ThemeAppearance {
    #[default]
    Light,
    Dark,
}

#[derive(Clone, Default)]
pub struct Theme {
    pub appearance: ThemeAppearance,
    pub tokens: SemanticThemeTokens,
    pub scrollbar: ScrollbarTheme,
    pub resizable: ResizableTheme,
}

impl Global for Theme {}

impl Theme {
    pub fn global(cx: &App) -> Self {
        cx.try_global::<Self>().cloned().unwrap_or_default()
    }

    pub fn global_mut(cx: &mut App) -> &mut Self {
        if !cx.has_global::<Self>() {
            cx.set_global(Self::default());
        }
        cx.global_mut::<Self>()
    }
}

pub(crate) trait ActiveTheme {
    fn theme(&self) -> Theme;
}

impl ActiveTheme for App {
    #[inline(always)]
    fn theme(&self) -> Theme {
        Theme::global(self)
    }
}

#[derive(Clone, Default)]
pub struct ScrollbarTheme {
    mode: ScrollbarMode,
    motion: ScrollbarMotion,
    styles: ScrollbarStyles,
}

impl ScrollbarTheme {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_mode(mut self, mode: ScrollbarMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn with_motion(mut self, motion: ScrollbarMotion) -> Self {
        self.motion = motion;
        self
    }

    pub fn with_styles(mut self, styles: ScrollbarStyles) -> Self {
        self.styles = styles;
        self
    }

    pub fn mode(&self) -> ScrollbarMode {
        self.mode
    }

    pub fn motion(&self) -> ScrollbarMotion {
        self.motion
    }

    pub fn styles(&self) -> &ScrollbarStyles {
        &self.styles
    }
}

#[derive(Clone, Copy, Default)]
pub struct ResizableTheme {
    pub handle: Option<gpui::Hsla>,
    pub active_handle: Option<gpui::Hsla>,
}
