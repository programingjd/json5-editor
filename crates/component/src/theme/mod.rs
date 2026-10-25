use crate::{highlighter::HighlightTheme, list::ListSettings, scroll::ScrollbarMode};
use gpui::{
    App, Global, Hsla, IsZero as _, Pixels, SharedString, Window, WindowAppearance,
    prelude::FluentBuilder as _, px,
};
pub use gpui_base::{
    ColorTokens, RadiusTokens, SemanticThemeTokens, ShadowTokens, SpacingTokens, TextStyleToken,
    TypographyTokens,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    ops::{Deref, DerefMut},
    rc::Rc,
    sync::Arc,
    time::Duration,
};

mod color;
mod mono_font;
mod motion;
mod registry;
mod schema;
mod system_font;
mod theme_color;

pub use color::*;
pub use motion::*;
pub use registry::*;
pub use schema::*;
pub use theme_color::*;

pub fn init(cx: &mut App) {
    registry::init(cx);

    // Ensure theme is loaded directly on startup for WASM compatibility
    Theme::change(ThemeMode::Light, None, cx);
    Theme::sync_scrollbar_appearance(cx);
}

pub trait ActiveTheme {
    fn theme(&self) -> &Theme;
}

impl ActiveTheme for App {
    #[inline(always)]
    fn theme(&self) -> &Theme {
        Theme::global(self)
    }
}

fn default_true() -> bool {
    true
}

const RADIUS_FULL: Pixels = px(9999.);

const SCROLLBAR_IDLE: Duration = Duration::from_secs(2);

const SCROLLBAR_ENTER: Duration = Duration::from_millis(300);

const SCROLLBAR_EXIT: Duration = Duration::from_millis(500);

const SCROLLBAR_EXPAND: Duration = Duration::from_millis(300);

const MOBILE_SCROLLBAR_THUMB_WIDTH: Pixels = px(3.);

const MOBILE_SCROLLBAR_THUMB_INSET: Pixels = px(2.);

const SCROLLBAR_THUMB_HOVER_WIDTH: Pixels = px(6.);

const SCROLLBAR_THUMB_ACTIVE_WIDTH: Pixels = px(8.);

const SCROLLBAR_THUMB_INSET: Pixels = px(4.);

fn scrollbar_motion(mode: ScrollbarMode) -> gpui_base::ScrollbarMotion {
    gpui_base::ScrollbarMotion::default()
        .with_idle(SCROLLBAR_IDLE)
        .with_enter(SCROLLBAR_ENTER)
        .with_exit(SCROLLBAR_EXIT)
        .with_expand(SCROLLBAR_EXPAND)
        .with_entrance(gpui_base::ScrollbarEntrance::Fade)
        .with_thumb_hover_entrance(match mode {
            ScrollbarMode::Scrolling | ScrollbarMode::Always => gpui_base::ScrollbarEntrance::Fade,
            ScrollbarMode::Hover => gpui_base::ScrollbarEntrance::SlideAndFade,
        })
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Theme {
    pub colors: ThemeColor,

    #[serde(default)]
    pub tokens: ThemeTokens,
    pub highlight_theme: Arc<HighlightTheme>,
    pub light_theme: Rc<ThemeConfig>,
    pub dark_theme: Rc<ThemeConfig>,

    pub mode: ThemeMode,

    pub font_family: SharedString,

    pub font_size: Pixels,

    pub mono_font_family: SharedString,

    pub mono_font_size: Pixels,

    pub radius: Pixels,

    pub radius_lg: Pixels,
    pub shadow: bool,

    #[serde(default = "default_true")]
    pub focus_ring: bool,
    pub transparent: Hsla,

    #[serde(alias = "scrollbar_show")]
    pub scrollbar_mode: ScrollbarMode,

    pub list: ListSettings,

    #[serde(skip)]
    pub motion: MotionTokens,
}

impl Default for Theme {
    fn default() -> Self {
        Self::from(&ThemeColor::default())
    }
}

impl Deref for Theme {
    type Target = ThemeColor;

    fn deref(&self) -> &Self::Target {
        &self.colors
    }
}

impl DerefMut for Theme {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.colors
    }
}

impl Global for Theme {}

impl Theme {
    #[inline(always)]
    pub fn global(cx: &App) -> &Theme {
        cx.global::<Theme>()
    }

    #[inline(always)]
    pub fn global_mut(cx: &mut App) -> &mut Theme {
        cx.global_mut::<Theme>()
    }

    pub fn update<R>(cx: &mut App, edit: impl FnOnce(&mut Theme) -> R) -> R {
        Self::edit(cx, false, edit)
    }

    fn edit<R>(cx: &mut App, reload_mode: bool, edit: impl FnOnce(&mut Theme) -> R) -> R {
        let theme = Theme::global_mut(cx);
        let colors_before = theme.colors;
        let tokens_before = theme.tokens;
        let mode_before = theme.mode;
        let light_before = theme.light_theme.clone();
        let dark_before = theme.dark_theme.clone();
        let fonts_before = (theme.font_family.clone(), theme.mono_font_family.clone());

        let result = edit(theme);

        theme
            .tokens
            .reconcile(&mut theme.colors, &colors_before, &tokens_before);
        let mode_changed = theme.mode != mode_before;
        let (config, config_before) = if theme.mode.is_dark() {
            (&theme.dark_theme, &dark_before)
        } else {
            (&theme.light_theme, &light_before)
        };
        // `apply_config` registers the file it applies and switches to its
        // mode, so a mode change that arrives with a newly registered config
        // has already loaded it. Loading it again would put the file's radius,
        // fonts and colors back over whatever the closure edited after it.
        let installed_by_edit = mode_changed && !Rc::ptr_eq(config, config_before);
        if (mode_changed || reload_mode) && !installed_by_edit {
            let config = config.clone();
            theme.apply_config(&config);
        }
        let fonts_changed =
            (&theme.font_family, &theme.mono_font_family) != (&fonts_before.0, &fonts_before.1);
        if mode_changed || reload_mode || fonts_changed {
            system_font::resolve_default_font(cx);
            mono_font::resolve_default_mono_font(cx);
        }
        Self::sync_base(cx);
        cx.refresh_windows();
        result
    }

    #[inline(always)]
    pub fn is_dark(&self) -> bool {
        self.mode.is_dark()
    }

    pub fn theme_name(&self) -> &SharedString {
        if self.is_dark() {
            &self.dark_theme.name
        } else {
            &self.light_theme.name
        }
    }

    pub fn sync_system_appearance(window: Option<&mut Window>, cx: &mut App) {
        // Better use window.appearance() for avoid error on Linux.
        // https://github.com/longbridge/gpui-kit/issues/104
        let appearance = window
            .as_ref()
            .map(|window| window.appearance())
            .unwrap_or_else(|| cx.window_appearance());

        Self::change(appearance, window, cx);
    }

    pub fn sync_scrollbar_appearance(cx: &mut App) {
        let mode = if cx.should_auto_hide_scrollbars() {
            ScrollbarMode::Scrolling
        } else {
            ScrollbarMode::Hover
        };
        Self::set_scrollbar_mode(mode, cx);
    }

    pub fn set_scrollbar_mode(mode: ScrollbarMode, cx: &mut App) {
        Self::update(cx, |theme| theme.scrollbar_mode = mode);
    }

    pub fn change(mode: impl Into<ThemeMode>, _window: Option<&mut Window>, cx: &mut App) {
        let mode = mode.into();
        if !cx.has_global::<Theme>() {
            let mut theme = Theme::default();
            theme.light_theme = ThemeRegistry::global(cx).default_light_theme().clone();
            theme.dark_theme = ThemeRegistry::global(cx).default_dark_theme().clone();
            cx.set_global(theme);
        }

        Self::edit(cx, true, |theme| theme.mode = mode);
    }

    fn base_theme(&self) -> gpui_base::Theme {
        gpui_base::Theme {
            appearance: if self.mode.is_dark() {
                gpui_base::ThemeAppearance::Dark
            } else {
                gpui_base::ThemeAppearance::Light
            },
            tokens: self.semantic_tokens(),
            scrollbar: gpui_base::ScrollbarTheme::new()
                .with_mode(self.scrollbar_mode)
                .with_motion(scrollbar_motion(self.scrollbar_mode))
                .with_styles(
                    gpui_base::ScrollbarStyles::default()
                        .track(|style| style.bg(self.scrollbar))
                        .track_hover(|style| style.bg(self.scrollbar))
                        .track_active(|style| style.bg(self.scrollbar).border_color(self.border))
                        .thumb(|style| {
                            style
                                .bg(self.tokens.scrollbar_thumb)
                                .radius(self.radius)
                                .when(gpui_base::is_mobile(), |style| {
                                    style
                                        .width(MOBILE_SCROLLBAR_THUMB_WIDTH)
                                        .inset(MOBILE_SCROLLBAR_THUMB_INSET)
                                        .radius(RADIUS_FULL)
                                })
                        })
                        .thumb_hover(|style| {
                            style
                                .bg(self.tokens.scrollbar_thumb_hover)
                                .radius(self.radius)
                                .when(gpui_base::is_mobile(), |style| {
                                    style
                                        .width(SCROLLBAR_THUMB_HOVER_WIDTH)
                                        .inset(SCROLLBAR_THUMB_INSET)
                                })
                        })
                        .thumb_active(|style| {
                            style
                                .bg(self.tokens.scrollbar_thumb_hover)
                                .radius(self.radius)
                                .when(gpui_base::is_mobile(), |style| {
                                    style
                                        .width(SCROLLBAR_THUMB_ACTIVE_WIDTH)
                                        .inset(SCROLLBAR_THUMB_INSET)
                                })
                        }),
                ),
            resizable: gpui_base::ResizableTheme {
                handle: Some(self.border),
                active_handle: Some(self.drag_border),
            },
        }
    }

    pub fn sync_base(cx: &mut App) {
        let theme = Theme::global(cx).clone();
        let base_theme = theme.base_theme();
        cx.set_global(base_theme);
        crate::text::install_text_view_defaults(&theme, cx);
    }

    #[inline]
    pub fn input_background(&self) -> Hsla {
        if self.is_dark() {
            self.input.mix_oklab(self.transparent, 0.3)
        } else {
            self.background
        }
    }

    #[inline]
    pub(crate) fn editor_background(&self) -> Hsla {
        self.highlight_theme
            .style
            .editor_background
            .unwrap_or_else(|| self.input_background())
    }

    pub fn semantic_tokens(&self) -> SemanticThemeTokens {
        SemanticThemeTokens {
            colors: self.color_tokens(),
            radius: self.radius_tokens(),
            spacing: self.spacing_tokens(),
            typography: self.typography_tokens(),
            shadow: self.shadow_tokens(),
        }
    }

    pub fn motion_tokens(&self) -> &MotionTokens {
        &self.motion
    }

    pub fn color_tokens(&self) -> ColorTokens {
        ColorTokens {
            background: self.background,
            foreground: self.foreground,
            surface: self.popover,
            surface_foreground: self.popover_foreground,
            primary: self.primary,
            primary_foreground: self.primary_foreground,
            secondary: self.secondary,
            secondary_foreground: self.secondary_foreground,
            muted: self.muted,
            muted_foreground: self.muted_foreground,
            accent: self.accent,
            accent_foreground: self.accent_foreground,
            destructive: self.danger,
            destructive_foreground: self.danger_foreground,
            border: self.border,
            input: self.input,
            ring: self.ring,
            selection: self.selection,
        }
    }

    pub fn radius_full(&self) -> Pixels {
        if self.radius.is_zero() {
            px(0.)
        } else {
            RADIUS_FULL
        }
    }

    pub fn radius_2xl(&self) -> Pixels {
        self.radius * 2.5
    }

    pub fn radius_3xl(&self) -> Pixels {
        self.radius * 3.
    }

    pub fn radius_4xl(&self) -> Pixels {
        self.radius * 3.5
    }

    pub fn radius_tokens(&self) -> RadiusTokens {
        RadiusTokens {
            none: px(0.),
            sm: self.radius / 2.,
            md: self.radius,
            lg: self.radius_lg,
            xl: self.radius * 2.,
            full: self.radius_full(),
        }
    }

    pub fn spacing_tokens(&self) -> SpacingTokens {
        SpacingTokens::default()
    }

    pub fn typography_tokens(&self) -> TypographyTokens {
        let mut tokens = TypographyTokens::default();
        tokens.sans = self.font_family.clone();
        tokens.mono = self.mono_font_family.clone();
        tokens.md.size = self.font_size;
        tokens.mono_md.size = self.mono_font_size;
        tokens
    }

    pub fn shadow_tokens(&self) -> ShadowTokens {
        if self.shadow {
            ShadowTokens::elevations(self.transparent.alpha(0.18))
        } else {
            ShadowTokens::default()
        }
    }

    pub fn apply_semantic_tokens(&mut self, tokens: &SemanticThemeTokens) {
        let colors = tokens.colors;
        self.background = colors.background;
        self.foreground = colors.foreground;
        self.popover = colors.surface;
        self.popover_foreground = colors.surface_foreground;
        self.primary = colors.primary;
        self.primary_foreground = colors.primary_foreground;
        self.secondary = colors.secondary;
        self.secondary_foreground = colors.secondary_foreground;
        self.muted = colors.muted;
        self.muted_foreground = colors.muted_foreground;
        self.accent = colors.accent;
        self.accent_foreground = colors.accent_foreground;
        self.danger = colors.destructive;
        self.danger_foreground = colors.destructive_foreground;
        self.border = colors.border;
        self.input = colors.input;
        self.ring = colors.ring;

        self.tokens.background = colors.background.into();
        self.tokens.popover = colors.surface.into();
        self.tokens.primary = colors.primary.into();
        self.tokens.secondary = colors.secondary.into();
        self.tokens.muted = colors.muted.into();
        self.tokens.accent = colors.accent.into();
        self.tokens.danger = colors.destructive.into();

        self.radius = tokens.radius.md;
        self.radius_lg = tokens.radius.lg;
        self.font_family = tokens.typography.sans.clone();
        self.mono_font_family = tokens.typography.mono.clone();
        self.font_size = tokens.typography.md.size;
        self.mono_font_size = tokens.typography.mono_md.size;
        self.shadow = !tokens.shadow.sm.is_empty()
            || !tokens.shadow.md.is_empty()
            || !tokens.shadow.lg.is_empty();
    }

    pub fn resolve_semantic_config(&self, config: &SemanticThemeConfig) -> SemanticThemeTokens {
        let mut tokens = self.semantic_tokens();
        config.apply_to(&mut tokens);
        tokens
    }

    pub fn apply_semantic_config(&mut self, config: &SemanticThemeConfig) -> SemanticThemeTokens {
        let tokens = self.resolve_semantic_config(config);
        self.apply_semantic_tokens(&tokens);
        tokens
    }

    pub fn apply_semantic_config_str(
        &mut self,
        content: &str,
    ) -> anyhow::Result<SemanticThemeTokens> {
        let config = serde_json::from_str::<SemanticThemeConfigFile>(content)?;
        Ok(self.apply_semantic_config(&config.tokens))
    }
}

impl From<&ThemeColor> for Theme {
    fn from(colors: &ThemeColor) -> Self {
        Theme {
            mode: ThemeMode::default(),
            transparent: Hsla::transparent_black(),
            font_family: ".SystemUIFont".into(),
            font_size: px(16.),
            mono_font_family: mono_font::default_mono_font_family(),
            mono_font_size: px(13.),
            radius: px(6.),
            radius_lg: px(8.),
            shadow: true,
            focus_ring: true,
            scrollbar_mode: ScrollbarMode::default(),
            list: ListSettings::default(),
            colors: *colors,
            tokens: ThemeTokens::from(colors),
            light_theme: Rc::new(ThemeConfig::default()),
            dark_theme: Rc::new(ThemeConfig::default()),
            highlight_theme: HighlightTheme::default_light(),
            motion: MotionTokens::default(),
        }
    }
}

#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    PartialOrd,
    Eq,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    #[default]
    Light,
    Dark,
}

impl ThemeMode {
    #[inline(always)]
    pub fn is_dark(&self) -> bool {
        matches!(self, Self::Dark)
    }

    pub fn name(&self) -> &'static str {
        match self {
            ThemeMode::Light => "light",
            ThemeMode::Dark => "dark",
        }
    }
}

impl From<WindowAppearance> for ThemeMode {
    fn from(appearance: WindowAppearance) -> Self {
        match appearance {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Self::Dark,
            WindowAppearance::Light | WindowAppearance::VibrantLight => Self::Light,
        }
    }
}
