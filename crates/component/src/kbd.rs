use gpui::{
    Action, AsKeystroke, FocusHandle, Half, InteractiveElement as _, IntoElement, KeyBinding,
    KeyContext, Keystroke, ParentElement as _, RenderOnce, SharedString, StyleRefinement, Styled,
    Window, div, prelude::FluentBuilder as _, relative, rems,
};

use crate::{ActiveTheme, StyledExt};

/// Keyboard or pointer combinations, with a separate keycap for each key.
#[derive(IntoElement, Clone, Debug)]
pub struct ShortcutHint {
    style: StyleRefinement,
    alternatives: Vec<Vec<SharedString>>,
}

impl ShortcutHint {
    pub fn new(keys: impl IntoIterator<Item = impl Into<SharedString>>) -> Self {
        Self {
            style: StyleRefinement::default(),
            alternatives: vec![keys.into_iter().map(Into::into).collect()],
        }
    }

    pub fn with_alternative(
        mut self,
        keys: impl IntoIterator<Item = impl Into<SharedString>>,
    ) -> Self {
        self.alternatives
            .push(keys.into_iter().map(Into::into).collect());
        self
    }
}

impl Styled for ShortcutHint {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for ShortcutHint {
    fn render(self, _: &mut Window, cx: &mut gpui::App) -> impl IntoElement {
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_end()
            .gap_1()
            .line_height(relative(1.2))
            .refine_style(&self.style)
            .children(
                self.alternatives
                    .into_iter()
                    .filter(|keys| !keys.is_empty())
                    .enumerate()
                    .map(|(index, keys)| {
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .when(index > 0, |this| this.child("or"))
                            .child(div().flex().items_center().gap_0p5().children(
                                keys.into_iter().map(|key| {
                                    div()
                                        .flex_shrink_0()
                                        .bg(cx.theme().muted)
                                        .border_1()
                                        .border_color(cx.theme().foreground.opacity(0.4))
                                        .rounded(cx.theme().radius.half())
                                        .px_0p5()
                                        .text_center()
                                        .child(key)
                                }),
                            ))
                    }),
            )
    }
}

#[derive(IntoElement, Clone, Debug)]
pub struct Kbd {
    style: StyleRefinement,
    stroke: Keystroke,
    alternatives: Vec<Keystroke>,
    appearance: bool,
    outline: bool,
}

impl From<Keystroke> for Kbd {
    fn from(stroke: Keystroke) -> Self {
        Self {
            style: StyleRefinement::default(),
            stroke,
            alternatives: Vec::new(),
            appearance: true,
            outline: false,
        }
    }
}

impl Kbd {
    pub fn new(stroke: Keystroke) -> Self {
        Self {
            style: StyleRefinement::default(),
            stroke,
            alternatives: Vec::new(),
            appearance: true,
            outline: false,
        }
    }

    pub fn appearance(mut self, appearance: bool) -> Self {
        self.appearance = appearance;
        self
    }

    pub fn outline(mut self) -> Self {
        self.outline = true;
        self
    }

    pub fn binding_for_action(
        action: &dyn Action,
        context: Option<&str>,
        window: &Window,
    ) -> Option<Self> {
        let key_context = context.and_then(|context| KeyContext::parse(context).ok());
        let binding = match key_context {
            Some(context) => window.bindings_for_action_in_context(action, context),
            None => window.bindings_for_action(action),
        };

        Self::from_bindings(binding)
    }

    pub fn binding_for_action_in(
        action: &dyn Action,
        focus_handle: &FocusHandle,
        window: &Window,
    ) -> Option<Self> {
        Self::from_bindings(window.bindings_for_action_in(action, focus_handle))
    }

    pub fn global_binding_for_action(action: &dyn Action, window: &Window) -> Option<Self> {
        Self::from_bindings(window.bindings_for_action_in_context(action, KeyContext::default()))
    }

    fn from_bindings(bindings: Vec<KeyBinding>) -> Option<Self> {
        let primary = bindings
            .iter()
            .rev()
            .find(|binding| binding.keystrokes().len() == 1)?
            .keystrokes()[0]
            .as_keystroke()
            .clone();
        let mut result = Self::new(primary);
        for binding in bindings.iter().rev() {
            // Multi-stroke sequences are not simultaneous key combinations.
            if binding.keystrokes().len() != 1 {
                continue;
            }
            let key = binding.keystrokes()[0].as_keystroke();
            // The preferred binding follows the browser platform. Hide the
            // other platform's aliases, and collapse duplicate registrations.
            let other_platform = if result.stroke.modifiers.platform {
                key.modifiers.control && !key.modifiers.platform
            } else {
                key.modifiers.platform
            };
            if !other_platform && key != &result.stroke && !result.alternatives.contains(key) {
                result.alternatives.push(key.clone());
            }
        }
        Some(result)
    }

    pub fn format(key: &Keystroke) -> String {
        #[cfg(target_os = "macos")]
        const SEPARATOR: &str = "";
        #[cfg(not(target_os = "macos"))]
        const SEPARATOR: &str = "+";

        Self::key_labels(key)
            .iter()
            .map(|label| label.as_ref())
            .collect::<Vec<_>>()
            .join(SEPARATOR)
    }

    fn key_labels(key: &Keystroke) -> Vec<SharedString> {
        let mut parts = vec![];

        // The key map order in macOS is: ⌃⌥⇧⌘
        // And in Windows is: Ctrl+Alt+Shift+Win

        if key.modifiers.control {
            #[cfg(target_os = "macos")]
            parts.push("⌃");

            #[cfg(not(target_os = "macos"))]
            parts.push("Ctrl");
        }

        if key.modifiers.alt {
            #[cfg(target_os = "macos")]
            parts.push("⌥");

            #[cfg(not(target_os = "macos"))]
            parts.push("Alt");
        }

        if key.modifiers.shift {
            #[cfg(target_os = "macos")]
            parts.push("⇧");

            #[cfg(not(target_os = "macos"))]
            parts.push("Shift");
        }

        if key.modifiers.platform {
            #[cfg(target_os = "macos")]
            parts.push("⌘");

            #[cfg(target_family = "wasm")]
            parts.push("Cmd");

            #[cfg(not(any(target_os = "macos", target_family = "wasm")))]
            parts.push("Win");
        }

        let mut keys = String::new();
        let key_str = key.key.as_str();
        match key_str {
            #[cfg(target_os = "macos")]
            "ctrl" => keys.push('⌃'),
            #[cfg(not(target_os = "macos"))]
            "ctrl" => keys.push_str("Ctrl"),
            #[cfg(target_os = "macos")]
            "alt" => keys.push('⌥'),
            #[cfg(not(target_os = "macos"))]
            "alt" => keys.push_str("Alt"),
            #[cfg(target_os = "macos")]
            "shift" => keys.push('⇧'),
            #[cfg(not(target_os = "macos"))]
            "shift" => keys.push_str("Shift"),
            #[cfg(target_os = "macos")]
            "cmd" => keys.push('⌘'),
            #[cfg(target_family = "wasm")]
            "cmd" => keys.push_str("Cmd"),
            #[cfg(not(any(target_os = "macos", target_family = "wasm")))]
            "cmd" => keys.push_str("Win"),
            #[cfg(target_os = "macos")]
            "space" => keys.push_str("Space"),
            #[cfg(target_os = "macos")]
            "backspace" => keys.push('⌫'),
            #[cfg(not(target_os = "macos"))]
            "backspace" => keys.push_str("Backspace"),
            #[cfg(target_os = "macos")]
            "delete" => keys.push('⌫'),
            #[cfg(not(target_os = "macos"))]
            "delete" => keys.push_str("Delete"),
            #[cfg(target_os = "macos")]
            "escape" => keys.push('⎋'),
            #[cfg(not(target_os = "macos"))]
            "escape" => keys.push_str("Esc"),
            #[cfg(target_os = "macos")]
            "enter" => keys.push('⏎'),
            #[cfg(not(target_os = "macos"))]
            "enter" => keys.push_str("Enter"),
            "pagedown" => keys.push_str("Page Down"),
            "pageup" => keys.push_str("Page Up"),
            #[cfg(target_os = "macos")]
            "left" => keys.push('←'),
            #[cfg(not(target_os = "macos"))]
            "left" => keys.push_str("Left"),
            #[cfg(target_os = "macos")]
            "right" => keys.push('→'),
            #[cfg(not(target_os = "macos"))]
            "right" => keys.push_str("Right"),
            #[cfg(target_os = "macos")]
            "up" => keys.push('↑'),
            #[cfg(not(target_os = "macos"))]
            "up" => keys.push_str("Up"),
            #[cfg(target_os = "macos")]
            "down" => keys.push('↓'),
            #[cfg(not(target_os = "macos"))]
            "down" => keys.push_str("Down"),
            _ => {
                if key_str.len() == 1 {
                    keys.push_str(&key_str.to_uppercase());
                } else {
                    let mut chars = key_str.chars();
                    if let Some(first_char) = chars.next() {
                        keys.push_str(&format!(
                            "{}{}",
                            first_char.to_uppercase(),
                            chars.collect::<String>()
                        ));
                    } else {
                        keys.push_str(&key_str);
                    }
                }
            }
        }

        parts.push(&keys);
        parts.into_iter().map(SharedString::new).collect()
    }
}

impl Styled for Kbd {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Kbd {
    fn render(self, _: &mut gpui::Window, cx: &mut gpui::App) -> impl gpui::IntoElement {
        if !self.appearance {
            return std::iter::once(&self.stroke)
                .chain(&self.alternatives)
                .map(Self::format)
                .collect::<Vec<_>>()
                .join(" or ")
                .into_any_element();
        }

        let hint = self.alternatives.iter().fold(
            ShortcutHint::new(Self::key_labels(&self.stroke)),
            |hint, key| hint.with_alternative(Self::key_labels(key)),
        );
        div()
            // Lets a test ask whether a given shortcut hint was painted this
            // frame; a no-op outside test-support builds.
            .debug_selector(|| format!("kbd:{}", self.stroke.unparse()))
            .text_color(cx.theme().muted_foreground)
            .when(self.outline, |this| this.bg(cx.theme().tokens.background))
            .text_size(rems(0.6875))
            .flex_shrink_0()
            .refine_style(&self.style)
            .child(hint)
            .into_any_element()
    }
}
