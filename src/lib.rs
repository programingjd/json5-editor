use gpui_kit::component::{
    ActiveTheme as _, Colorize as _, Icon, Sizable as _, Size,
    button::{Button, ButtonCustomVariant, ButtonRounded, ButtonVariants as _},
    input::{
        AutoClosingPair, BracketPair, Editor, EditorState, SyntaxContext, TextDecoration,
        language_config::LanguageConfig, set_language_config,
    },
    toolbar::{Toolbar, ToolbarGroup},
};
use gpui_kit::{
    App, AppContext as _, Application, ApplicationHandle, Context, Entity, Focusable as _, Global,
    HighlightStyle, InteractiveElement as _, IntoElement, ParentElement, Render, SharedString,
    Styled as _, Window, WindowOptions, div, px, rgb,
};
use std::{borrow::Cow, cell::RefCell, rc::Rc};
use wasm_bindgen::prelude::*;

mod convert_action;
mod error_scrollbar;
mod filename;
mod info_dialogs;
mod overlay_frame;
mod palette;
mod prepared_document;
mod save_overlay;
mod search_panel;
mod shortcuts;
mod status_bar;
mod streaming;
mod syntax_data;
mod tree_sitter_highlighter;

gpui_kit::actions!(
    json5_editor,
    [
        OpenReplace,
        NextSearchMatch,
        PreviousSearchMatch,
        OpenFile,
        SaveFile,
        ConvertToJson,
        CollapseAllScopes,
        ExpandAllScopes,
        CollapseChildScopes,
        ExpandChildScopes,
        DuplicateSelection
    ]
);

enum ScopeCommand {
    CollapseAll,
    ExpandAll,
    CollapseChildren,
    ExpandChildren,
}

const EXAMPLE_JSON5: &str = "{ unquotedKey: 'single-quoted strings are valid', numbers: [0xdecaf, +Infinity, NaN, .5,], nested: { enabled: true, trailingComma: 'yes', }, }";
const GUTTER_LINE_NUMBER_FONT_SIZE: gpui_kit::Pixels = px(8. * 96. / 72.);

thread_local! {
    static APPLICATION: RefCell<Option<ApplicationHandle>> = const { RefCell::new(None) };
}

struct Json5EditorGlobal {
    editor: Entity<EditorState>,
    filename: Entity<SharedString>,
    syntax: Rc<RefCell<tree_sitter_highlighter::ParseSnapshot>>,
    save_overlay: Entity<save_overlay::SaveOverlay>,
    convert_action: Entity<convert_action::ConvertAction>,
}
impl Global for Json5EditorGlobal {}

struct ColorScheme {
    dark: bool,
    overridden: bool,
}
impl Global for ColorScheme {}

struct Json5EditorApp {
    filename: Entity<filename::Filename>,
    editor: Entity<EditorState>,
    search_panel: Entity<search_panel::EditorSearchPanel>,
    status_bar: Entity<status_bar::EditorStatusBar>,
    save_overlay: Entity<save_overlay::SaveOverlay>,
    convert_action: Entity<convert_action::ConvertAction>,
    info_dialogs: Entity<info_dialogs::InfoDialogs>,
    _syntax_observer: gpui_kit::Subscription,
    _filename_observer: gpui_kit::Subscription,
    _layout_observer: gpui_kit::Subscription,
    _conversion_observer: gpui_kit::Subscription,
}

impl Json5EditorApp {
    fn new(window: &mut Window, sbom_url: SharedString, notice_url: SharedString, cx: &mut App) -> Entity<Self> {
        let filename = cx.new(|_| SharedString::from("example.json5"));
        let syntax = Rc::new(RefCell::new(
            tree_sitter_highlighter::ParseSnapshot::default(),
        ));
        syntax.borrow_mut().prepare_open();
        set_language_config(
            "json5",
            LanguageConfig::default()
                .brackets([BracketPair::new("{", "}"), BracketPair::new("[", "]")])
                .auto_closing_pairs(
                    [
                        AutoClosingPair::new("{", "}"),
                        AutoClosingPair::new("[", "]"),
                        AutoClosingPair::new("(", ")"),
                        AutoClosingPair::new("\"", "\""),
                        AutoClosingPair::new("'", "'"),
                    ]
                    .into_iter()
                    .map(|pair| pair.not_in([SyntaxContext::String, SyntaxContext::Comment])),
                ),
            cx,
        );

        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .language("json5")
                .searchable(false)
                .soft_wrap(true)
                .default_value("")
        });
        let editor_for_scrollbar = editor.downgrade();
        editor.update(cx, |state, cx| {
            let syntax_for_factory = syntax.clone();
            state.set_highlighter_factory(
                Rc::new(move |language| {
                    tree_sitter_highlighter::TreeSitterHighlighter::new(
                        language,
                        syntax_for_factory.clone(),
                    )
                    .map(|highlighter| Box::new(highlighter) as Box<_>)
                }),
                cx,
            );
            state.set_highlighter("json5", cx);
            state.set_search_match_background(Some(search_match_background(cx)), cx);
            state.set_fold_icon_hover_fade(Some(std::time::Duration::from_millis(150)), cx);
            state.set_gutter_left_padding_em(0.25, cx);
            state.set_gutter_reserved_digits(Some(7), cx);
            state.set_gutter_line_number_font_size(Some(GUTTER_LINE_NUMBER_FONT_SIZE), cx);
            state.set_indent_guide_offset(px(-3.), cx);
            state.set_scrollbar_renderer(
                Some(Rc::new(move |scrollbar, context, window, cx| {
                    error_scrollbar::render(&editor_for_scrollbar, scrollbar, context, window, cx)
                })),
                cx,
            );
        });

        let save_overlay = save_overlay::SaveOverlay::new(&editor, filename.clone(), window, cx);
        let convert_action = convert_action::ConvertAction::new(&editor, syntax.clone(), cx);
        let info_dialogs = info_dialogs::InfoDialogs::new(&editor, sbom_url, notice_url, cx);
        cx.set_global(Json5EditorGlobal {
            editor: editor.clone(),
            filename: filename.clone(),
            syntax: syntax.clone(),
            save_overlay: save_overlay.clone(),
            convert_action: convert_action.clone(),
        });

        let bracket_decorations = editor.update(cx, |state, cx| {
            state.create_decorations_collection(Vec::new(), cx)
        });
        let previous_brackets = Rc::new(RefCell::new(Vec::<TextDecoration>::new()));
        let previous_cursor = Rc::new(RefCell::new(None));
        let syntax_for_observer = syntax.clone();
        let previous_brackets_for_observer = previous_brackets.clone();
        let bracket_decorations_for_observer = bracket_decorations.clone();
        let syntax_observer = cx.observe(&editor, move |editor, cx| {
            let cursor = editor.read(cx).cursor();
            let snapshot = syntax_for_observer.borrow();
            let key = (cursor, snapshot.version, cx.theme().is_dark());
            let mut previous_cursor = previous_cursor.borrow_mut();
            // Collections track edits independently of this observer. A whole
            // document replacement can expand an old one-byte bracket range,
            // even when the newly computed bracket positions are identical.
            let current_ranges = bracket_decorations_for_observer.get_ranges(cx);
            if *previous_cursor == Some(key)
                && current_ranges.iter().eq(previous_brackets_for_observer
                    .borrow()
                    .iter()
                    .map(|decoration| &decoration.range))
            {
                return;
            }
            *previous_cursor = Some(key);
            let matched = snapshot.matching_brackets(cursor);
            drop(snapshot);
            let decorations: Vec<TextDecoration> = matched
                .map(|ranges| {
                    ranges
                        .into_iter()
                        .map(|range| {
                            TextDecoration::new(
                                range,
                                HighlightStyle {
                                    color: Some(palette::get(cx).bracket_foreground),
                                    background_color: Some(palette::get(cx).bracket_background),
                                    ..Default::default()
                                },
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();
            let mut previous = previous_brackets_for_observer.borrow_mut();
            if *previous != decorations
                || !current_ranges
                    .iter()
                    .eq(decorations.iter().map(|decoration| &decoration.range))
            {
                *previous = decorations.clone();
                bracket_decorations_for_observer.set(decorations, cx);
            }
        });

        let search_panel = search_panel::EditorSearchPanel::new(&editor, window, cx);
        let status_bar = status_bar::EditorStatusBar::new(&editor, cx);
        let filename_field = filename::Filename::new(filename.clone(), editor.clone(), cx);
        cx.new(|cx: &mut Context<Self>| {
            let filename_observer = cx.observe(&filename, |_, filename, cx| {
                update_document_name(filename.read(cx).as_ref());
            });
            let conversion_observer = cx.observe(&convert_action, |_, _, cx| cx.notify());
            let mut was_rewrapping = false;
            let layout_observer = cx.observe(&editor, move |_, editor, cx| {
                let busy = editor.read(cx).is_rewrapping();
                if busy != was_rewrapping {
                    was_rewrapping = busy;
                    set_layout_busy(busy);
                }
            });
            Self {
                filename: filename_field,
                editor,
                search_panel,
                status_bar,
                save_overlay,
                convert_action,
                info_dialogs,
                _syntax_observer: syntax_observer,
                _filename_observer: filename_observer,
                _layout_observer: layout_observer,
                _conversion_observer: conversion_observer,
            }
        })
    }

    fn open(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.info_dialogs.update(cx, |info, cx| {
            info.dismiss(window, cx);
        });
        open_file_picker();
    }

    fn save(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.filename.update(cx, |filename, cx| filename.commit(window, cx));
        self.info_dialogs.update(cx, |info, cx| {
            info.dismiss(window, cx);
        });
        self.save_overlay
            .update(cx, |save, cx| save.open(window, cx));
    }

    fn modal_open(&self, cx: &App) -> bool {
        self.save_overlay.read(cx).is_open() || self.info_dialogs.read(cx).is_open()
    }

    fn change_scopes(&self, command: ScopeCommand, cx: &mut Context<Self>) {
        if self.modal_open(cx) {
            return;
        }
        self.editor.update(cx, |editor, cx| match command {
            ScopeCommand::CollapseAll => editor.collapse_all(cx),
            ScopeCommand::ExpandAll => editor.expand_all(cx),
            ScopeCommand::CollapseChildren => editor.collapse_children(cx),
            ScopeCommand::ExpandChildren => editor.expand_children(cx),
        });
    }
}

impl Render for Json5EditorApp {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let button_style = toolbar_button_style(cx);
        let scheme = cx.global::<ColorScheme>();
        let dark = scheme.dark;
        let overridden = scheme.overridden;

        gpui_kit::component::v_flex()
            .size_full()
            .key_context("Json5Editor")
            .capture_key_down(cx.listener(|this, _, window, cx| {
                if this
                    .info_dialogs
                    .update(cx, |info, cx| info.block_background_key(window, cx))
                    || this
                        .save_overlay
                        .update(cx, |save, cx| save.block_background_key(window, cx))
                {
                    window.prevent_default();
                    cx.stop_propagation();
                }
            }))
            .capture_action(
                cx.listener(|this, _: &gpui_kit::base::input::Escape, window, cx| {
                    if this.filename.read(cx).is_editing() {
                        return;
                    }
                    if this
                        .info_dialogs
                        .update(cx, |info, cx| info.dismiss(window, cx))
                        || this
                            .save_overlay
                            .update(cx, |save, cx| save.dismiss(window, cx))
                        || this
                            .search_panel
                            .update(cx, |search, cx| search.dismiss(window, cx))
                    {
                        cx.stop_propagation();
                    }
                }),
            )
            .capture_action(
                cx.listener(|this, _: &gpui_kit::base::input::Search, window, cx| {
                    cx.stop_propagation();
                    if this.modal_open(cx) {
                        return;
                    }
                    this.search_panel
                        .update(cx, |search, cx| search.open(false, window, cx));
                }),
            )
            .capture_action(cx.listener(|this, _: &OpenReplace, window, cx| {
                cx.stop_propagation();
                if this.modal_open(cx) {
                    return;
                }
                this.search_panel
                    .update(cx, |search, cx| search.open(true, window, cx));
            }))
            .capture_action(cx.listener(|this, _: &NextSearchMatch, _, cx| {
                cx.stop_propagation();
                if !this.modal_open(cx) {
                    this.search_panel
                        .update(cx, |search, cx| search.navigate_match(false, cx));
                }
            }))
            .capture_action(cx.listener(|this, _: &PreviousSearchMatch, _, cx| {
                cx.stop_propagation();
                if !this.modal_open(cx) {
                    this.search_panel
                        .update(cx, |search, cx| search.navigate_match(true, cx));
                }
            }))
            .capture_action(cx.listener(|this, _: &OpenFile, window, cx| {
                cx.stop_propagation();
                if !this.modal_open(cx) {
                    this.open(window, cx);
                }
            }))
            .capture_action(cx.listener(|this, _: &SaveFile, window, cx| {
                cx.stop_propagation();
                if !this.modal_open(cx) {
                    this.save(window, cx);
                }
            }))
            .capture_action(cx.listener(|this, _: &ConvertToJson, _, cx| {
                cx.stop_propagation();
                if !this.modal_open(cx) {
                    this.convert_action
                        .update(cx, |action, cx| action.convert(cx));
                }
            }))
            .capture_action(cx.listener(|this, _: &CollapseAllScopes, _, cx| {
                cx.stop_propagation();
                this.change_scopes(ScopeCommand::CollapseAll, cx);
            }))
            .capture_action(cx.listener(|this, _: &ExpandAllScopes, _, cx| {
                cx.stop_propagation();
                this.change_scopes(ScopeCommand::ExpandAll, cx);
            }))
            .capture_action(cx.listener(|this, _: &CollapseChildScopes, _, cx| {
                cx.stop_propagation();
                this.change_scopes(ScopeCommand::CollapseChildren, cx);
            }))
            .capture_action(cx.listener(|this, _: &ExpandChildScopes, _, cx| {
                cx.stop_propagation();
                this.change_scopes(ScopeCommand::ExpandChildren, cx);
            }))
            .child(
                Toolbar::new("json5-editor-toolbar")
                    .w_full()
                    .border_b_1()
                    .border_color(editor_separator_color(cx))
                    .bg(cx.theme().muted)
                    .content(
                        ToolbarGroup::new("scope-actions")
                            .label("Editing, scopes and search")
                            .flex()
                            .flex_1()
                            .min_w_0()
                            .items_center()
                            .gap_1()
                            .content(
                                Button::new("undo-json5")
                                    .cursor_pointer()
                                    .with_size(Size::Small)
                                    .custom(button_style.clone())
                                    .rounded(ButtonRounded::Size(px(4.)))
                                    .icon(
                                        Icon::default()
                                            .data(include_bytes!("../assets/icons/undo.svg")),
                                    )
                                    .accessibility_label("Undo")
                                    .tooltip_with(|window, cx| {
                                        shortcuts::tooltip("Undo", "Ctrl/Cmd+Z", window, cx)
                                    })
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        if !this.modal_open(cx) {
                                            let focus = this.editor.read(cx).focus_handle(cx);
                                            focus.focus(window, cx);
                                            focus.dispatch_action(
                                                &gpui_kit::base::input::Undo,
                                                window,
                                                cx,
                                            );
                                        }
                                    })),
                            )
                            .content(
                                Button::new("redo-json5")
                                    .cursor_pointer()
                                    .with_size(Size::Small)
                                    .custom(button_style.clone())
                                    .rounded(ButtonRounded::Size(px(4.)))
                                    .icon(
                                        Icon::default()
                                            .data(include_bytes!("../assets/icons/redo.svg")),
                                    )
                                    .accessibility_label("Redo")
                                    .tooltip_with(|window, cx| {
                                        shortcuts::tooltip(
                                            "Redo",
                                            "Ctrl/Cmd+Y or Ctrl/Cmd+Shift+Z",
                                            window,
                                            cx,
                                        )
                                    })
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        if !this.modal_open(cx) {
                                            let focus = this.editor.read(cx).focus_handle(cx);
                                            focus.focus(window, cx);
                                            focus.dispatch_action(
                                                &gpui_kit::base::input::Redo,
                                                window,
                                                cx,
                                            );
                                        }
                                    })),
                            )
                            .content(
                                Button::new("collapse-all-json5")
                                    .cursor_pointer()
                                    .with_size(Size::Small)
                                    .custom(button_style.clone())
                                    .rounded(ButtonRounded::Size(px(4.)))
                                    .icon(Icon::default().data(include_bytes!(
                                        "../assets/icons/collapse-all.svg"
                                    )))
                                    .accessibility_label("Collapse all scopes")
                                    .tooltip_with(|window, cx| {
                                        shortcuts::tooltip(
                                            "Collapse all scopes",
                                            shortcuts::COLLAPSE_ALL,
                                            window,
                                            cx,
                                        )
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.change_scopes(ScopeCommand::CollapseAll, cx);
                                    })),
                            )
                            .content(
                                Button::new("expand-all-json5")
                                    .cursor_pointer()
                                    .with_size(Size::Small)
                                    .custom(button_style.clone())
                                    .rounded(ButtonRounded::Size(px(4.)))
                                    .icon(
                                        Icon::default().data(include_bytes!(
                                            "../assets/icons/expand-all.svg"
                                        )),
                                    )
                                    .accessibility_label("Expand all scopes")
                                    .tooltip_with(|window, cx| {
                                        shortcuts::tooltip(
                                            "Expand all scopes",
                                            shortcuts::EXPAND_ALL,
                                            window,
                                            cx,
                                        )
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.change_scopes(ScopeCommand::ExpandAll, cx);
                                    })),
                            )
                            .content(
                                Button::new("collapse-children-json5")
                                    .cursor_pointer()
                                    .with_size(Size::Small)
                                    .custom(button_style.clone())
                                    .rounded(ButtonRounded::Size(px(4.)))
                                    .icon(
                                        Icon::default().data(include_bytes!(
                                            "../assets/icons/collapse.svg"
                                        )),
                                    )
                                    .accessibility_label("Collapse current scope")
                                    .tooltip_with(|window, cx| {
                                        shortcuts::tooltip(
                                            "Collapse current scope",
                                            shortcuts::COLLAPSE_CHILDREN,
                                            window,
                                            cx,
                                        )
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.change_scopes(ScopeCommand::CollapseChildren, cx);
                                    })),
                            )
                            .content(
                                Button::new("expand-children-json5")
                                    .cursor_pointer()
                                    .with_size(Size::Small)
                                    .custom(button_style.clone())
                                    .rounded(ButtonRounded::Size(px(4.)))
                                    .icon(
                                        Icon::default()
                                            .data(include_bytes!("../assets/icons/expand.svg")),
                                    )
                                    .accessibility_label("Expand current scope")
                                    .tooltip_with(|window, cx| {
                                        shortcuts::tooltip(
                                            "Expand current scope",
                                            shortcuts::EXPAND_CHILDREN,
                                            window,
                                            cx,
                                        )
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.change_scopes(ScopeCommand::ExpandChildren, cx);
                                    })),
                            )
                            .content(
                                Button::new("search-json5")
                                    .cursor_pointer()
                                    .with_size(Size::Small)
                                    .custom(button_style.clone())
                                    .rounded(ButtonRounded::Size(px(4.)))
                                    .icon(
                                        Icon::default()
                                            .data(include_bytes!("../assets/icons/search.svg")),
                                    )
                                    .accessibility_label("Search")
                                    .tooltip_with(|window, cx| {
                                        shortcuts::tooltip("Search", "Ctrl/Cmd+F", window, cx)
                                    })
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        if !this.modal_open(cx) {
                                            this.search_panel.update(cx, |search, cx| {
                                                search.open(false, window, cx)
                                            });
                                        }
                                    })),
                            )
                            .content(
                                Button::new("replace-json5")
                                    .cursor_pointer()
                                    .with_size(Size::Small)
                                    .custom(button_style.clone())
                                    .rounded(ButtonRounded::Size(px(4.)))
                                    .icon(
                                        Icon::default().data(include_bytes!(
                                            "../assets/icons/replace.svg"
                                        )),
                                    )
                                    .accessibility_label("Replace")
                                    .tooltip_with(|window, cx| {
                                        shortcuts::tooltip("Replace", "Ctrl/Cmd+R", window, cx)
                                    })
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        if !this.modal_open(cx) {
                                            this.search_panel.update(cx, |search, cx| {
                                                search.open(true, window, cx)
                                            });
                                        }
                                    })),
                            ),
                    )
                    .content(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_center()
                            .text_xs()
                            .font_family(cx.theme().font_family.clone())
                            .text_color(cx.theme().popover_foreground)
                            .child(self.filename.clone()),
                    )
                    .content(
                        ToolbarGroup::new("file-actions")
                            .label("File actions and theme")
                            .flex()
                            .flex_1()
                            .min_w_0()
                            .items_center()
                            .justify_end()
                            .gap_1()
                            .content(
                                Button::new("open-json5")
                                    .cursor_pointer()
                                    .with_size(Size::Small)
                                    .custom(button_style.clone())
                                    .rounded(ButtonRounded::Size(px(4.)))
                                    .icon(
                                        Icon::default()
                                            .data(include_bytes!("../assets/icons/open.svg")),
                                    )
                                    .accessibility_label("Open file")
                                    .tooltip_with(|window, cx| {
                                        shortcuts::tooltip("Open file", shortcuts::OPEN, window, cx)
                                    })
                                    .on_click(
                                        cx.listener(|this, _, window, cx| this.open(window, cx)),
                                    ),
                            )
                            .content(
                                Button::new("revert-json5")
                                    .cursor_pointer()
                                    .with_size(Size::Small)
                                    .custom(button_style.clone())
                                    .rounded(ButtonRounded::Size(px(4.)))
                                    .icon(
                                        Icon::default()
                                            .data(include_bytes!("../assets/icons/revert.svg")),
                                    )
                                    .accessibility_label("Revert file")
                                    .tooltip("Revert to originally opened content")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        if !this.modal_open(cx) {
                                            this.info_dialogs.update(cx, |info, cx| {
                                                info.open(
                                                    info_dialogs::InfoPage::Revert,
                                                    window,
                                                    cx,
                                                )
                                            });
                                        }
                                    })),
                            )
                            .content(self.convert_action.clone())
                            .content(
                                Button::new("save-json5")
                                    .cursor_pointer()
                                    .with_size(Size::Small)
                                    .custom(button_style.clone())
                                    .rounded(ButtonRounded::Size(px(4.)))
                                    .icon(
                                        Icon::default().data(include_bytes!(
                                            "../assets/icons/download.svg"
                                        )),
                                    )
                                    .accessibility_label("Download file")
                                    .tooltip_with(|window, cx| {
                                        shortcuts::tooltip(
                                            "Download file",
                                            shortcuts::SAVE,
                                            window,
                                            cx,
                                        )
                                    })
                                    .on_click(
                                        cx.listener(|this, _, window, cx| this.save(window, cx)),
                                    ),
                            )
                            .content(
                                Button::new("theme-toggle")
                                    .cursor_pointer()
                                    .with_size(Size::Small)
                                    .custom(button_style.clone())
                                    .rounded(ButtonRounded::Size(px(4.)))
                                    .child(
                                        Icon::default()
                                            .data(if dark {
                                                include_bytes!("../assets/icons/dark-theme.svg")
                                                    .as_slice()
                                            } else {
                                                include_bytes!(
                                                    "../assets/icons/light-theme.svg"
                                                )
                                                .as_slice()
                                            })
                                            .size_4()
                                            .text_color(if overridden {
                                                rgb(0x3574f0).into()
                                            } else {
                                                toolbar_icon_color(cx)
                                            })
                                            .cursor_pointer(),
                                    )
                                    .accessibility_label(if dark {
                                        "Dark mode"
                                    } else {
                                        "Light mode"
                                    })
                                    .tooltip(if dark {
                                        "Use light mode"
                                    } else {
                                        "Use dark mode"
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if this.modal_open(cx) {
                                            return;
                                        }
                                        request_toggle_color_scheme();
                                    })),
                            )
                            .content(info_dialogs::InfoDialogs::menu(&self.info_dialogs, cx)),
                    ),
            )
            .child(self.search_panel.clone())
            .child(
                div()
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .on_children_prepainted(|bounds, _, _| {
                        if let Some(editor) = bounds.first() {
                            update_editor_bounds(
                                f32::from(editor.origin.x),
                                f32::from(editor.origin.y),
                                f32::from(editor.size.width),
                                f32::from(editor.size.height),
                            );
                        }
                    })
                    .child(
                        div()
                            .size_full()
                            .key_context("Json5Document")
                            .on_action(cx.listener(|this, _: &DuplicateSelection, window, cx| {
                                cx.stop_propagation();
                                if !this.modal_open(cx) {
                                    this.editor.update(cx, |editor, cx| {
                                        editor.duplicate_selection(window, cx);
                                    });
                                }
                            }))
                            .child(
                                Editor::new(&self.editor)
                                    .readonly(self.convert_action.read(cx).is_busy())
                                    .highlight_style_resolver(palette::styles(cx))
                                    .size_full()
                                    .rounded_none(),
                            ),
                    )
                    .child(self.save_overlay.clone())
                    .child(self.info_dialogs.clone()),
            )
            .child(self.status_bar.clone())
    }
}

fn toolbar_button_style(cx: &App) -> ButtonCustomVariant {
    ButtonCustomVariant::new(cx)
        .foreground(toolbar_icon_color(cx))
        .hover(palette::get(cx).hover)
        .active(palette::get(cx).active)
}

fn toolbar_icon_color(cx: &App) -> gpui_kit::Hsla {
    palette::get(cx).icon
}

fn safe_file_name(name: &str) -> String {
    let name = name
        .trim()
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(name)
        .trim();
    if name.is_empty() {
        String::new()
    } else if name.rfind('.').is_none() {
        format!("{name}.json5")
    } else {
        name.to_owned()
    }
}

fn web_application() -> Application {
    use gpui_kit::web::{CanvasFontFallback, WebBackendPreference, WebPlatform};
    use std::sync::Arc;

    let platform = Rc::new(WebPlatform::new_with_backend_and_font_fallback(
        false,
        WebBackendPreference::Auto,
        CanvasFontFallback::EmojiAndCjk,
    ));
    let http_client = Arc::new(platform.fetch_http_client());
    Application::with_platform(platform)
        .with_http_client(http_client)
        .with_assets(gpui_kit::assets::Assets::new(""))
}

fn search_match_background(cx: &App) -> gpui_kit::Hsla {
    palette::get(cx).search
}

fn editor_separator_color(cx: &App) -> gpui_kit::Hsla {
    palette::get(cx).separator
}

fn editor_overlay_background(cx: &App) -> gpui_kit::Hsla {
    cx.theme()
        .highlight_theme
        .style
        .editor_background
        .unwrap_or(cx.theme().background)
}

fn editor_heading_color(cx: &App) -> gpui_kit::Hsla {
    cx.theme()
        .popover_foreground
        .mix(cx.theme().foreground, 0.65)
}

fn bordered_action_button(id: &'static str, label: &'static str, cx: &App) -> Button {
    Button::new(id)
        .cursor_pointer()
        .with_size(Size::XSmall)
        .custom(toolbar_button_style(cx).foreground(cx.theme().popover_foreground))
        .rounded(ButtonRounded::Small)
        .border_1()
        .border_color(cx.theme().border.opacity(0.5))
        .px_3()
        .accessibility_label(label)
        .child(
            div()
                .text_size(gpui_kit::rems(0.6875))
                .font_weight(gpui_kit::FontWeight::NORMAL)
                .child(label),
        )
}

fn set_theme(dark: bool, cx: &mut App) {
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
    let palette = palette::install(dark, cx);
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
    if let Some(editor) = cx
        .try_global::<Json5EditorGlobal>()
        .map(|global| global.editor.clone())
    {
        editor.update(cx, |editor, cx| {
            editor.set_search_match_background(Some(palette.search), cx);
            cx.notify();
        });
    }
    update_overlay_colors(
        &palette.toolbar.to_hex(),
        &palette.background.to_hex(),
        &editor_heading_color(cx).to_hex(),
        &palette.ui_foreground.to_hex(),
        &palette.border.to_hex(),
        &palette.hover.to_hex(),
        &palette.active.to_hex(),
    );
}

#[wasm_bindgen]
pub fn run(
    dark: bool,
    overridden: bool,
    inter_woff2: Vec<u8>,
    mono_woff2: Vec<u8>,
    inter_italic_woff2: Vec<u8>,
    sbom_url: String,
    notice_url: String,
) -> Result<(), JsValue> {
    console_error_panic_hook::set_once();
    let fonts = vec![
        decode_font("Inter", &inter_woff2)?,
        decode_font("Inter Italic", &inter_italic_woff2)?,
        decode_font("CommitMono", &mono_woff2)?,
    ];
    console_log::init_with_level(log::Level::Info).expect("Failed to initialize logger");
    tracing_wasm::set_as_global_default();
    gpui_kit::platform::web_init();

    let app = web_application();
    let launch = move |cx: &mut App| {
        gpui_kit::init(cx);
        shortcuts::init(cx);
        cx.text_system()
            .add_fonts(fonts)
            .expect("Failed to register downloaded fonts");
        cx.set_global(ColorScheme { dark, overridden });
        set_theme(dark, cx);
        gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
            Json5EditorApp::new(window, sbom_url.clone().into(), notice_url.clone().into(), cx)
        })
        .expect("Failed to open JSON5 editor window");
        cx.activate(true);
        request_open_document("example.json5", EXAMPLE_JSON5);
    };

    APPLICATION.with(|application| {
        *application.borrow_mut() = Some(app.run_embedded(launch));
    });
    Ok(())
}

fn decode_font(family: &str, woff2: &[u8]) -> Result<Cow<'static, [u8]>, JsValue> {
    wuff::decompress_woff2(woff2)
        .map(Cow::Owned)
        .map_err(|error| {
            JsValue::from_str(&format!("Failed to decode {family} WOFF2 font: {error:?}"))
        })
}

// CSS is the source of truth; this only updates GPUI's render snapshot.
#[wasm_bindgen]
pub fn set_color_scheme(dark: bool, overridden: bool) {
    APPLICATION.with(|application| {
        if let Some(handle) = application.borrow().as_ref() {
            handle.update(|cx| {
                let Some(current) = cx.try_global::<ColorScheme>() else {
                    return;
                };
                if current.dark == dark && current.overridden == overridden {
                    return;
                }
                let palette_changed = current.dark != dark;
                cx.set_global(ColorScheme { dark, overridden });
                if palette_changed {
                    set_theme(dark, cx);
                }
                cx.refresh_windows();
            });
        }
    });
}

#[wasm_bindgen]
pub fn finish_json_conversion(revision: u32, content: String, error: String) {
    APPLICATION.with(|application| {
        let application = application.borrow();
        let Some(handle) = application.as_ref() else {
            return;
        };
        handle.update(|cx| {
            let Some(window) = cx.windows().into_iter().next() else {
                return;
            };
            let _ = cx.update_window(window, |_, window, cx| {
                let Some(action) = cx
                    .try_global::<Json5EditorGlobal>()
                    .map(|global| global.convert_action.clone())
                else {
                    return;
                };
                action.update(cx, |action, cx| {
                    action.finish(revision, content, error, window, cx)
                });
            });
        });
    });
}

#[wasm_bindgen]
pub fn finish_save(name: String, error: String) {
    APPLICATION.with(|application| {
        let application = application.borrow();
        let Some(handle) = application.as_ref() else {
            return;
        };
        handle.update(|cx| {
            let Some(window) = cx.windows().into_iter().next() else {
                return;
            };
            let _ = cx.update_window(window, |_, window, cx| {
                let Some(save) = cx
                    .try_global::<Json5EditorGlobal>()
                    .map(|global| global.save_overlay.clone())
                else {
                    return;
                };
                save.update(cx, |save, cx| save.finish(name, error, window, cx));
            });
        });
    });
}

#[wasm_bindgen]
pub fn open_prepared_json5_file(
    name: String,
    view: String,
    highlights: Vec<u32>,
    folds: Vec<u32>,
    brackets: Vec<u32>,
    diagnostics: String,
) -> bool {
    let Ok(preparation) = prepared_document::take_ready(&view) else {
        return false;
    };
    let mut installed = false;
    APPLICATION.with(|application| {
        let application = application.borrow();
        let Some(handle) = application.as_ref() else {
            return;
        };
        handle.update(|cx| {
            let Some(window) = cx.windows().into_iter().next() else {
                return;
            };
            let _ = cx.update_window(window, |_, window, cx| {
                let Some((editor, syntax, filename, save_overlay)) =
                    cx.try_global::<Json5EditorGlobal>().map(|global| {
                        (
                            global.editor.clone(),
                            global.syntax.clone(),
                            global.filename.clone(),
                            global.save_overlay.clone(),
                        )
                    })
                else {
                    return;
                };
                syntax.borrow_mut().prepare_open();
                editor.update(cx, |state, cx| {
                    // The worker's final projection often matches the streamed
                    // preview exactly. Reuse its existing wrap/layout map rather
                    // than replacing and measuring the entire document again.
                    // Compare bytes before reusing: JSON5 comments or incomplete
                    // preview tokens can legitimately produce a different view.
                    if let Some(preparation) = preparation {
                        if !state.set_prepared_value(preparation, window, cx) {
                            return;
                        }
                    } else if streaming::is_receiving() && state.text() == view.as_str() {
                        log::info!("[JSON5 load] reusing the streamed preview buffer");
                    } else {
                        state.set_value(view.as_str(), window, cx);
                    }
                    if state.text() != view.as_str() {
                        log::error!("Document installation changed the worker's formatted text");
                        return;
                    }
                    if let Err(error) = syntax.borrow_mut().install(
                        &highlights,
                        &folds,
                        &brackets,
                        &diagnostics,
                        state.text(),
                    ) {
                        log::error!("Failed to install document analysis: {error}");
                        return;
                    }
                    syntax.borrow_mut().installing = false;
                    let snapshot = syntax.borrow();
                    let text = state.text().clone();
                    if let Some(current) = state.diagnostics_mut() {
                        current.reset(&text);
                        current.extend(snapshot.diagnostics.clone());
                    }
                    state.apply_highlighter_fold_candidates(snapshot.folds.clone(), cx);
                    state.set_readonly(false, cx);
                    installed = true;
                });
                if installed {
                    streaming::finish();
                    save_overlay.update(cx, |save, cx| {
                        save.dismiss(window, cx);
                    });
                    filename.update(cx, |filename, cx| {
                        *filename = name.clone().into();
                        cx.notify();
                    });
                }
            });
        });
    });
    installed
}

#[wasm_bindgen]
pub fn apply_syntax_result(
    revision: u32,
    highlights: Vec<u32>,
    folds: Vec<u32>,
    brackets: Vec<u32>,
    diagnostics: String,
) {
    APPLICATION.with(|application| {
        let application = application.borrow();
        let Some(handle) = application.as_ref() else {
            return;
        };
        handle.update(|cx| {
            let Some((editor, syntax)) = cx
                .try_global::<Json5EditorGlobal>()
                .map(|global| (global.editor.clone(), global.syntax.clone()))
            else {
                return;
            };
            if syntax.borrow().revision != revision {
                return;
            }
            editor.update(cx, |state, cx| {
                if let Err(error) = syntax.borrow_mut().install(
                    &highlights,
                    &folds,
                    &brackets,
                    &diagnostics,
                    state.text(),
                ) {
                    log::error!("Failed to install syntax analysis: {error}");
                    return;
                }
                let snapshot = syntax.borrow();
                let text = state.text().clone();
                if let Some(current) = state.diagnostics_mut() {
                    current.reset(&text);
                    current.extend(snapshot.diagnostics.clone());
                }
                state.apply_highlighter_fold_candidates(snapshot.folds.clone(), cx);
            });
        });
    });
}

#[wasm_bindgen(module = "/src/json5_editor_bridge.js")]
extern "C" {
    fn is_mac_platform() -> bool;
    fn update_document_name(name: &str);
    fn set_layout_busy(busy: bool);
    fn request_toggle_color_scheme();
    fn open_file_picker();
    fn request_open_document(name: &str, contents: &str);
    fn request_save_file(name: &str, formatting: &str, convert_to_json: bool);
    fn request_convert_to_json();
    fn request_revert_document();
    fn update_overlay_colors(
        window_background: &str,
        panel_background: &str,
        heading: &str,
        foreground: &str,
        border: &str,
        hover: &str,
        active: &str,
    );
    fn load_save_option(name: &str) -> bool;
    fn store_save_option(name: &str, enabled: bool);
    fn load_save_formatting() -> String;
    fn store_save_formatting(formatting: &str);
    fn update_editor_bounds(left: f32, top: f32, width: f32, height: f32);
    fn queue_syntax_edit(
        revision: u32,
        start: usize,
        old_end: usize,
        replacement: &str,
        start_row: usize,
        start_column: usize,
        old_end_row: usize,
        old_end_column: usize,
        new_end_row: usize,
        new_end_column: usize,
    );
}
