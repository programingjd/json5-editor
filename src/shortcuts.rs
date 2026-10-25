use gpui_kit::component::kbd::ShortcutHint;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::{
    Action, AnyView, App, Div, FontWeight, KeyBinding, NoAction, ParentElement as _, SharedString,
    Styled as _, Window, div, px, rems,
};

pub(super) const OPEN: &str = "Ctrl/Cmd+O";
pub(super) const SAVE: &str = "Ctrl/Cmd+S";
pub(super) const CONVERT: &str = "Ctrl/Cmd+J";
pub(super) const COLLAPSE_ALL: &str = "Ctrl/Cmd+Shift+-";
pub(super) const EXPAND_ALL: &str = "Ctrl/Cmd+Shift++ or Ctrl/Cmd+Shift+=";
pub(super) const COLLAPSE_CHILDREN: &str = "Ctrl/Cmd+-";
pub(super) const EXPAND_CHILDREN: &str = "Ctrl/Cmd++ or Ctrl/Cmd+=";

fn display_keys(keys: &str, mac: bool) -> SharedString {
    let keys = keys.replace("Ctrl/Cmd", if mac { "Cmd" } else { "Ctrl" });
    if mac {
        keys.replace("Alt", "Option")
    } else {
        keys
    }
    .into()
}

fn key_hint(keys: &str) -> ShortcutHint {
    // Labels are authored here with complete alternatives. A final '+' is
    // the Plus key itself, rather than another separator.
    fn combination(label: &str) -> Vec<SharedString> {
        let mut keys: Vec<_> = label
            .split('+')
            .filter(|key| !key.is_empty())
            .map(SharedString::new)
            .collect();
        if label.ends_with('+') {
            keys.push("+".into());
        }
        keys
    }

    let mut alternatives = keys.split(" or ");
    let hint = ShortcutHint::new(combination(alternatives.next().unwrap_or_default()));
    alternatives.fold(hint, |hint, keys| hint.with_alternative(combination(keys)))
}

fn accelerator(cx: &mut App, key: &str, action: impl Action + Clone, context: &str) {
    // GPUI displays the last matching binding in menus. Prefer the modifier
    // for the browser's platform while keeping both accelerators available.
    let modifiers = if crate::is_mac_platform() {
        ["ctrl", "cmd"]
    } else {
        ["cmd", "ctrl"]
    };
    cx.bind_keys(modifiers.map(|modifier| {
        KeyBinding::new(&format!("{modifier}-{key}"), action.clone(), Some(context))
    }));
}

pub(super) fn init(cx: &mut App) {
    use gpui_kit::base::input::*;

    accelerator(cx, "f", Search, "Json5Editor");
    accelerator(cx, "r", crate::OpenReplace, "Json5Editor");
    accelerator(cx, "o", crate::OpenFile, "Json5Editor");
    accelerator(cx, "s", crate::SaveFile, "Json5Editor");
    accelerator(cx, "j", crate::ConvertToJson, "Json5Editor");
    accelerator(cx, "shift--", crate::CollapseAllScopes, "Json5Editor");
    accelerator(cx, "shift-+", crate::ExpandAllScopes, "Json5Editor");
    accelerator(cx, "-", crate::CollapseChildScopes, "Json5Editor");
    accelerator(cx, "+", crate::ExpandChildScopes, "Json5Editor");
    // The main keyboard reports '_' for Shift+Minus. '=' is the unshifted
    // counterpart of '+' on many keyboards; keypad '+' needs no alias.
    accelerator(cx, "shift-_", crate::CollapseAllScopes, "Json5Editor");
    accelerator(cx, "shift-=", crate::ExpandAllScopes, "Json5Editor");
    accelerator(cx, "=", crate::ExpandChildScopes, "Json5Editor");
    accelerator(cx, "d", crate::DuplicateSelection, "Json5Document");
    cx.bind_keys([
        KeyBinding::new("f3", crate::NextSearchMatch, Some("Json5Editor")),
        KeyBinding::new("shift-f3", crate::PreviousSearchMatch, Some("Json5Editor")),
    ]);

    // Register aliases first so menus show the standard editing shortcuts.
    accelerator(cx, "k", Cut, "Input");
    accelerator(cx, "u", Paste, "Input");
    accelerator(cx, "a", SelectAll, "Input");
    accelerator(cx, "c", Copy, "Input");
    accelerator(cx, "x", Cut, "Input");
    accelerator(cx, "v", Paste, "Input");
    accelerator(cx, "z", Undo, "Input");
    accelerator(cx, "y", Redo, "Input");
    accelerator(cx, "shift-z", Redo, "Input");
    accelerator(
        cx,
        "enter",
        Enter {
            secondary: true,
            shift: false,
        },
        "Input",
    );
    accelerator(
        cx,
        "shift-enter",
        Enter {
            secondary: true,
            shift: true,
        },
        "SearchField",
    );
    // Replace the framework's old indentation accelerators in this app.
    accelerator(cx, "[", NoAction, "Input");
    accelerator(cx, "]", NoAction, "Input");
    cx.bind_keys([KeyBinding::new("escape", Escape, Some("Json5Editor"))]);

    // WASM has target_os="unknown". Install native Mac navigation according
    // to the browser platform rather than relying on Base's compile-time cfg.
    if crate::is_mac_platform() {
        cx.bind_keys([
            KeyBinding::new("cmd-left", MoveHome, Some("Input")),
            KeyBinding::new("cmd-right", MoveEnd, Some("Input")),
            KeyBinding::new("cmd-up", MoveToStart, Some("Input")),
            KeyBinding::new("cmd-down", MoveToEnd, Some("Input")),
            KeyBinding::new("cmd-home", MoveToStart, Some("Input")),
            KeyBinding::new("cmd-end", MoveToEnd, Some("Input")),
            KeyBinding::new("cmd-shift-left", SelectToStartOfLine, Some("Input")),
            KeyBinding::new("cmd-shift-right", SelectToEndOfLine, Some("Input")),
            KeyBinding::new("cmd-shift-up", SelectToStart, Some("Input")),
            KeyBinding::new("cmd-shift-down", SelectToEnd, Some("Input")),
            KeyBinding::new("cmd-shift-home", SelectToStart, Some("Input")),
            KeyBinding::new("cmd-shift-end", SelectToEnd, Some("Input")),
            KeyBinding::new("alt-left", MoveToPreviousWord, Some("Input")),
            KeyBinding::new("alt-right", MoveToNextWord, Some("Input")),
            KeyBinding::new("alt-shift-left", SelectToPreviousWordStart, Some("Input")),
            KeyBinding::new("alt-shift-right", SelectToNextWordEnd, Some("Input")),
            KeyBinding::new("alt-backspace", DeleteToPreviousWordStart, Some("Input")),
            KeyBinding::new("alt-delete", DeleteToNextWordEnd, Some("Input")),
            KeyBinding::new("cmd-backspace", DeleteToBeginningOfLine, Some("Input")),
            KeyBinding::new("cmd-delete", DeleteToEndOfLine, Some("Input")),
            KeyBinding::new("cmd-alt-up", AddCursorAbove, Some("Input")),
            KeyBinding::new("cmd-alt-down", AddCursorBelow, Some("Input")),
        ]);
    }
}

pub(super) fn tooltip(
    label: &'static str,
    keys: &'static str,
    window: &mut Window,
    cx: &mut App,
) -> AnyView {
    let keys = display_keys(keys, crate::is_mac_platform());
    Tooltip::element(move |_, _| {
        div().flex().items_center().gap_2().child(label).child(
            key_hint(&keys)
                .text_size(rems(0.53125))
                .font_weight(FontWeight::NORMAL),
        )
    })
    .build(window, cx)
}

// Labels follow the same browser platform detection as the navigation bindings.
// A missing platform entry hides a shortcut that is unavailable on that system.
#[derive(Clone, Copy)]
struct Shortcut {
    label: &'static str,
    other: Option<&'static str>,
    mac: Option<&'static str>,
}

impl Shortcut {
    const fn new(label: &'static str, keys: &'static str) -> Self {
        Self {
            label,
            other: Some(keys),
            mac: Some(keys),
        }
    }

    const fn on_mac(mut self, keys: &'static str) -> Self {
        self.mac = Some(keys);
        self
    }

    fn keys(&self, mac: bool) -> Option<SharedString> {
        (if mac { self.mac } else { self.other }).map(|keys| display_keys(keys, mac))
    }
}

// Audited against Input's wasm32 bindings and mouse handlers, SearchField's
// Enter capture, the app's action captures, Root, and the controls used here.
const SECTIONS: &[(&str, &[u8], &[Shortcut])] = &[
    (
        "Files",
        include_bytes!("../assets/icons/file-set.svg"),
        &[
            Shortcut::new("Open file", OPEN),
            Shortcut::new("Download file", SAVE),
            Shortcut::new("Convert to JSON", CONVERT),
        ],
    ),
    (
        "Editing",
        include_bytes!("../assets/icons/edit.svg"),
        &[
            Shortcut::new("Undo", "Ctrl/Cmd+Z"),
            Shortcut::new("Redo", "Ctrl/Cmd+Y or Ctrl/Cmd+Shift+Z"),
            Shortcut::new("Duplicate selection or line", "Ctrl/Cmd+D"),
            Shortcut::new("Indent selection", "Tab"),
            Shortcut::new("Unindent selection", "Shift+Tab"),
        ],
    ),
    (
        "Caret/Cursor",
        include_bytes!("../assets/icons/cursor-text.svg"),
        &[
            Shortcut::new("Add caret at pointer", "Alt+Click"),
            Shortcut::new("Add caret above / below", "Alt+Shift+↑ or Alt+Shift+↓")
                .on_mac("Option+Shift+↑ or Option+Shift+↓ or Cmd+Option+↑ or Cmd+Option+↓"),
            Shortcut::new("Rectangular selection", "Alt+Drag or Alt+Shift+Drag"),
            Shortcut::new("Keep only the active caret (panels closed)", "Escape"),
        ],
    ),
    (
        "Find and replace",
        include_bytes!("../assets/icons/find.svg"),
        &[
            Shortcut::new("Open or focus Find", "Ctrl/Cmd+F"),
            Shortcut::new("Open Find and replace", "Ctrl/Cmd+R"),
            Shortcut::new("Next match", "F3"),
            Shortcut::new("Previous match", "Shift+F3"),
            Shortcut::new(
                "Replace current match (in Replace)",
                "Enter or Shift+Enter or Ctrl/Cmd+Enter",
            ),
            Shortcut::new("Insert newline in Find or Replace", "Ctrl/Cmd+Shift+Enter"),
            Shortcut::new("Close search, including from the editor", "Escape"),
        ],
    ),
    (
        "Folding",
        include_bytes!("../assets/icons/collapse-all.svg"),
        &[
            Shortcut::new("Collapse all scopes", COLLAPSE_ALL),
            Shortcut::new("Expand all scopes", EXPAND_ALL),
            Shortcut::new("Collapse current scope", COLLAPSE_CHILDREN),
            Shortcut::new("Expand current scope", EXPAND_CHILDREN),
        ],
    ),
];

pub(super) fn render(cx: &App) -> Div {
    let mac = crate::is_mac_platform();
    div()
        .flex()
        .flex_col()
        .gap_4()
        .children(SECTIONS.iter().map(|(title, icon, rows)| {
            div()
                .flex()
                .flex_col()
                .gap_4()
                .child(crate::info_dialogs::credits_separator(cx))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(gpui_kit::component::Icon::default().data(icon).size_4())
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(crate::editor_heading_color(cx))
                                .child(*title),
                        )
                        .children(rows.iter().filter_map(|shortcut| {
                            let keys = shortcut.keys(mac)?;
                            Some(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap_2()
                                    .min_h_6()
                                    .text_xs()
                                    .child(div().flex_1().min_w_0().child(shortcut.label))
                                    .child(
                                        key_hint(&keys)
                                            .text_size(px(11.))
                                            .max_w(gpui_kit::relative(0.6)),
                                    ),
                            )
                        })),
                )
        }))
}
