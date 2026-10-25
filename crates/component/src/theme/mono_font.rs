//! Keeps the theme's default monospace family on a font the machine has.
//!
//! GPUI panics on the first line it lays out in a family it cannot find, and
//! `Font::fallbacks` does not help there: it is a cascade list for missing
//! glyphs, consulted only after the family itself has loaded. The theme's
//! platform default is a bare name, so a machine without that font would crash
//! on its first code block. This probe swaps the default for an installed
//! alternative before any text is laid out.
//!
//! Only the platform default is probed. A family the application or a theme
//! file chose explicitly is used as-is: it is usually embedded through
//! `add_fonts`, and Windows lists families under their localized names, so an
//! English name for a CJK font would look missing when it is not.

use gpui::{App, SharedString};
use gpui_base::TypographyTokens;
use std::sync::OnceLock;

const MONO_FONT_ALTERNATES: &[&str] = if cfg!(target_os = "macos") {
    &["Monaco", "Courier New"]
} else if cfg!(target_os = "windows") {
    &["Cascadia Mono", "Courier New"]
} else {
    &["Noto Sans Mono", "Liberation Mono", "Ubuntu Mono"]
};

const SYSTEM_UI_FONT: &str = ".SystemUIFont";

pub(super) fn default_mono_font_family() -> SharedString {
    TypographyTokens::default().mono
}

pub(super) fn resolve_default_mono_font(cx: &mut App) {
    if cx.global::<super::Theme>().mono_font_family != default_mono_font_family() {
        return;
    }
    // Web startup registers downloaded fonts after component initialization.
    // There is no usable fallback yet, so leave the default unresolved until
    // fonts exist or the application selects its explicit family.
    if installed_font_names(cx).is_empty() {
        return;
    }
    let family = installed_default_mono_font_family(cx);
    cx.global_mut::<super::Theme>().mono_font_family = family;
}

fn installed_default_mono_font_family(cx: &App) -> SharedString {
    static RESOLVED: OnceLock<SharedString> = OnceLock::new();
    RESOLVED
        .get_or_init(|| {
            let default = default_mono_font_family();
            let family = first_installed(&default, MONO_FONT_ALTERNATES, installed_font_names(cx));
            if family != default {
                tracing::warn!(
                    "Monospace font {default:?} is not installed, using {family:?} instead."
                );
            }
            family
        })
        .clone()
}

pub(super) fn installed_font_names(cx: &App) -> &'static [String] {
    static NAMES: OnceLock<Vec<String>> = OnceLock::new();
    if let Some(names) = NAMES.get() {
        return names;
    }
    let names = cx.text_system().all_font_names();
    if names.is_empty() {
        // An empty web text system is temporary, not the installed font list.
        return &[];
    }
    NAMES.get_or_init(|| names)
}

fn first_installed(default: &str, alternates: &[&str], installed: &[String]) -> SharedString {
    std::iter::once(default)
        .chain(alternates.iter().copied())
        .find(|candidate| installed.iter().any(|name| name == candidate))
        .unwrap_or(SYSTEM_UI_FONT)
        .to_string()
        .into()
}
