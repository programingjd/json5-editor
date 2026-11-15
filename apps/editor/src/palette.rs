use gpui_kit::base::input::{HighlightStyleResolver, SharedHighlightStyleResolver};
use gpui_kit::component::highlighter::HighlightTheme;
use gpui_kit::{App, Global, HighlightStyle, Hsla};
use std::sync::Arc;

#[derive(serde::Deserialize)]
pub(super) struct Palette {
    pub(super) background: Hsla,
    pub(super) foreground: Hsla,
    pub(super) toolbar: Hsla,
    pub(super) popover: Hsla,
    pub(super) ui_foreground: Hsla,
    pub(super) icon: Hsla,
    pub(super) hover: Hsla,
    pub(super) active: Hsla,
    pub(super) border: Hsla,
    pub(super) separator: Hsla,
    pub(super) selection: Hsla,
    pub(super) caret: Hsla,
    pub(super) link: Hsla,
    pub(super) error: Hsla,
    pub(super) error_marker: Hsla,
    pub(super) search: Hsla,
    pub(super) bracket_foreground: Hsla,
    pub(super) bracket_background: Hsla,
    pub(super) fold_foreground: Hsla,
    pub(super) fold_background: Hsla,
    pub(super) syntax: HighlightTheme,
}
struct ActivePalette(Arc<Palette>);
impl Global for ActivePalette {}

pub(super) fn install(dark: bool, cx: &mut App) -> Arc<Palette> {
    let palette: Palette = serde_json::from_str(if dark {
        include_str!("dark-theme.json")
    } else {
        include_str!("light-theme.json")
    })
    .expect("valid bundled editor palette");
    let palette = Arc::new(palette);
    cx.set_global(ActivePalette(palette.clone()));
    palette
}
pub(super) fn get(cx: &App) -> &Palette {
    &cx.global::<ActivePalette>().0
}
pub(super) fn styles(cx: &App) -> SharedHighlightStyleResolver {
    cx.global::<ActivePalette>().0.clone()
}

impl HighlightStyleResolver for Palette {
    fn style(&self, name: &str) -> Option<HighlightStyle> {
        match name {
            "folded" => Some(HighlightStyle {
                color: Some(self.fold_foreground),
                ..Default::default()
            }),
            "folded.background" => Some(HighlightStyle {
                color: Some(self.fold_background),
                ..Default::default()
            }),
            _ => self.syntax.style.syntax.style(name),
        }
    }
}

pub(super) fn apply_theme(dark: bool, cx: &mut App) -> Arc<Palette> {
    use gpui_kit::component::theme::{Theme, ThemeMode};

    let mode = if dark {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    };
    Theme::update(cx, |theme| {
        theme.font_family = "Inter".into();
        theme.mono_font_family = "CommitMono".into();
    });
    Theme::change(mode, None, cx);
    let palette = install(dark, cx);
    Theme::update(cx, |theme| {
        theme.colors.background = palette.background;
        theme.colors.foreground = palette.foreground;
        theme.colors.muted = palette.toolbar;
        theme.colors.muted_foreground = palette.icon;
        theme.colors.popover = palette.popover;
        theme.colors.popover_foreground = palette.ui_foreground;
        theme.colors.link = palette.link;
        theme.colors.border = palette.border;
        theme.colors.selection = palette.selection;
        theme.colors.caret = palette.caret;
        theme.colors.input = palette.background;
        if !dark {
            theme.colors.red = palette.error;
            theme.colors.danger = palette.error;
        }
        theme.highlight_theme = std::sync::Arc::new(palette.syntax.clone());
    });
    palette
}
