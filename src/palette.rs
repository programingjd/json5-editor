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
