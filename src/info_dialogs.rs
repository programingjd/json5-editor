use gpui_kit::component::{
    ActiveTheme as _, FocusTrapElement as _, Icon, Sizable as _, Size,
    button::{Button, ButtonRounded, ButtonVariants as _},
    input::EditorState,
    link::Link,
    menu::{DropdownMenu as _, PopupMenuItem},
};
use gpui_kit::{
    App, AppContext as _, Context, ElementId, Empty, Entity, FocusHandle, FontWeight,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString, Styled as _,
    Window, div, prelude::FluentBuilder as _, rems,
};

const REPOSITORY: &str = "https://github.com/programingjd/json5-editor";

#[derive(Clone, Copy)]
pub(super) enum InfoPage {
    Shortcuts,
    About,
    Revert,
}

struct DependencyNotice {
    name: SharedString,
    attribution: SharedString,
    website: SharedString,
    license: SharedString,
    repository: SharedString,
    license_url: SharedString,
    source_focus: FocusHandle,
    website_focus: FocusHandle,
    license_focus: FocusHandle,
}

#[derive(serde::Deserialize)]
struct RawNotice {
    name: String,
    #[serde(default)]
    attribution: String,
    website: String,
    license: String,
    repository: String,
    license_url: String,
}

#[derive(serde::Deserialize)]
struct NoticeSections {
    color_schemes: Vec<RawNotice>,
    icons: Vec<RawNotice>,
    fonts: Vec<RawNotice>,
    libraries: Vec<RawNotice>,
}

pub(super) struct InfoDialogs {
    editor: Entity<EditorState>,
    page: Option<InfoPage>,
    focus: FocusHandle,
    repository_focus: FocusHandle,
    license_focus: FocusHandle,
    jetbrains_focus: FocusHandle,
    zed_focus: FocusHandle,
    cancel_focus: FocusHandle,
    jetbrains_logo: std::sync::Arc<gpui_kit::Image>,
    bom_focus: FocusHandle,
    sbom_url: SharedString,
    notice_focus: FocusHandle,
    notice_url: SharedString,
    return_focus: Option<FocusHandle>,
    color_schemes: Vec<DependencyNotice>,
    icons: Vec<DependencyNotice>,
    fonts: Vec<DependencyNotice>,
    libraries: Vec<DependencyNotice>,
}

impl InfoDialogs {
    pub(super) fn new(
        editor: &Entity<EditorState>,
        sbom_url: SharedString,
        notice_url: SharedString,
        cx: &mut App,
    ) -> Entity<Self> {
        let notices: NoticeSections =
            serde_json::from_str(include_str!("../assets/dependency-notices.json"))
                .expect("Generated dependency notices must be valid JSON");
        cx.new(|cx| {
            let mut convert = |notice: RawNotice| DependencyNotice {
                name: notice.name.into(),
                attribution: notice.attribution.into(),
                website: notice.website.into(),
                license: if notice.license.is_empty() {
                    SharedString::default()
                } else {
                    format!("{} License", notice.license).into()
                },
                repository: notice.repository.into(),
                license_url: notice.license_url.into(),
                source_focus: cx.focus_handle().tab_stop(true),
                website_focus: cx.focus_handle().tab_stop(true),
                license_focus: cx.focus_handle().tab_stop(true),
            };
            let color_schemes = notices
                .color_schemes
                .into_iter()
                .map(&mut convert)
                .collect();
            let icons = notices.icons.into_iter().map(&mut convert).collect();
            let fonts = notices.fonts.into_iter().map(&mut convert).collect();
            let libraries = notices.libraries.into_iter().map(&mut convert).collect();
            Self {
                editor: editor.clone(),
                page: None,
                focus: cx.focus_handle(),
                repository_focus: cx.focus_handle().tab_stop(true),
                license_focus: cx.focus_handle().tab_stop(true),
                jetbrains_focus: cx.focus_handle().tab_stop(true),
                zed_focus: cx.focus_handle().tab_stop(true),
                cancel_focus: cx.focus_handle().tab_stop(true),
                jetbrains_logo: std::sync::Arc::new(gpui_kit::Image::from_bytes(
                    gpui_kit::ImageFormat::Svg,
                    include_bytes!("../assets/icons/jetbrains.svg").to_vec(),
                )),
                bom_focus: cx.focus_handle().tab_stop(true),
                sbom_url,
                notice_focus: cx.focus_handle().tab_stop(true),
                notice_url,
                return_focus: None,
                color_schemes,
                icons,
                fonts,
                libraries,
            }
        })
    }

    pub(super) fn is_open(&self) -> bool {
        self.page.is_some()
    }

    pub(super) fn open(&mut self, page: InfoPage, window: &mut Window, cx: &mut Context<Self>) {
        if cx
            .global::<crate::Json5EditorGlobal>()
            .save_overlay
            .read(cx)
            .is_open()
        {
            return;
        }
        if !self.is_open() {
            self.return_focus = window.focused(cx);
        }
        self.page = Some(page);
        if matches!(page, InfoPage::Revert) {
            self.cancel_focus.focus(window, cx);
        } else {
            self.focus.focus(window, cx);
        }
        cx.notify();
    }

    pub(super) fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.page.take().is_none() {
            return false;
        }
        if let Some(focus) = self.return_focus.take() {
            focus.focus(window, cx);
        } else {
            self.editor
                .update(cx, |editor, cx| editor.focus(window, cx));
        }
        cx.notify();
        true
    }

    pub(super) fn block_background_key(&self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.is_open() || self.focus.contains_focused(window, cx) {
            return false;
        }
        self.focus.focus(window, cx);
        true
    }

    pub(super) fn menu(owner: &Entity<Self>, cx: &App) -> impl IntoElement {
        let owner = owner.downgrade();
        Button::new("editor-overflow")
            .cursor_pointer()
            .with_size(Size::Small)
            .custom(crate::toolbar_button_style(cx))
            .rounded(ButtonRounded::Small)
            .icon(Icon::default().data(include_bytes!("../assets/icons/more-vertical.svg")))
            .accessibility_label("More actions")
            .tooltip("More actions")
            .dropdown_menu(move |menu, window, cx| {
                [InfoPage::Shortcuts, InfoPage::About].into_iter().fold(
                    menu.with_size(Size::Small)
                        .min_w(window.rem_size() * 9.)
                        .item_colors(cx.theme().popover_foreground, cx.theme().selection),
                    |menu, page| {
                        let owner = owner.clone();
                        let label = match page {
                            InfoPage::Shortcuts => "Keyboard Shortcuts",
                            InfoPage::About => "About",
                            InfoPage::Revert => unreachable!(),
                        };
                        menu.item(
                            PopupMenuItem::element(move |_, _| {
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .text_xs()
                                    .child(
                                        Icon::default()
                                            .data(match page {
                                                InfoPage::Shortcuts => include_bytes!(
                                                    "../assets/icons/keyboard.svg"
                                                )
                                                .as_slice(),
                                                InfoPage::About => {
                                                    include_bytes!("../assets/icons/help.svg")
                                                        .as_slice()
                                                }
                                                InfoPage::Revert => unreachable!(),
                                            })
                                            .size_4(),
                                    )
                                    .child(label)
                            })
                            .on_click(move |_, window, cx| {
                                let _ = owner.update(cx, |_, cx| {
                                    // Open after menu dismissal restores focus to its trigger.
                                    cx.on_next_frame(window, move |this, window, cx| {
                                        this.open(page, window, cx)
                                    });
                                });
                            }),
                        )
                    },
                )
            })
    }

    fn credits_section(
        &self,
        title: &'static str,
        icon: &'static [u8],
        notices: &[DependencyNotice],
        window: &Window,
        cx: &App,
    ) -> gpui_kit::Div {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(Icon::default().data(icon).size_4())
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(crate::editor_heading_color(cx))
                    .child(title),
            )
            .when(!notices.is_empty(), |section| {
                section.child(self.notice_rows(notices, window, cx))
            })
    }

    fn notice_rows(
        &self,
        notices: &[DependencyNotice],
        window: &Window,
        cx: &App,
    ) -> gpui_kit::Div {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .children(notices.iter().map(|notice| {
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .min_h_6()
                    .text_xs()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(if notice.website.is_empty() {
                                div().child(notice.name.clone()).into_any_element()
                            } else {
                                about_link(
                                    (ElementId::from(notice.name.clone()), "website"),
                                    notice.name.clone(),
                                    notice.website.clone(),
                                    &notice.website_focus,
                                    window,
                                    cx,
                                )
                                .into_any_element()
                            })
                            .when(!notice.attribution.is_empty(), |label| {
                                if notice.name.as_ref() == "GPUI" {
                                    label.child("by").child(self.zed_credit(window, cx))
                                } else {
                                    label.child(notice.attribution.clone())
                                }
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .when(!notice.repository.is_empty(), |links| {
                                links.child(about_link(
                                    (ElementId::from(notice.name.clone()), "source"),
                                    "Repository",
                                    notice.repository.clone(),
                                    &notice.source_focus,
                                    window,
                                    cx,
                                ))
                            })
                            .when(!notice.license_url.is_empty(), |links| {
                                links.child(about_link(
                                    (ElementId::from(notice.name.clone()), "license"),
                                    notice.license.clone(),
                                    notice.license_url.clone(),
                                    &notice.license_focus,
                                    window,
                                    cx,
                                ))
                            }),
                    )
            }))
    }

    fn zed_credit(&self, window: &Window, cx: &App) -> impl IntoElement {
        div()
            .id("zed-credit-focus")
            .cursor_pointer()
            .track_focus(&self.zed_focus)
            .when(self.zed_focus.is_focused(window), |link| {
                link.bg(cx.theme().selection.opacity(0.35))
            })
            .on_key_down(|event, window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    window.prevent_default();
                    cx.stop_propagation();
                    cx.open_url("https://zed.dev");
                }
            })
            .child(
                Link::new("zed-credit")
                    .href("https://zed.dev")
                    .cursor_pointer()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .cursor_pointer()
                            .child(
                                Icon::default()
                                    .data(include_bytes!("../assets/icons/zed-logo.svg"))
                                    .size_4()
                                    .cursor_pointer(),
                            )
                            .child("Zed Industries"),
                    ),
            )
    }

    fn about(&self, window: &Window, cx: &App) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .text_2xl()
                    .text_center()
                    .text_color(crate::editor_heading_color(cx))
                    .child("JSON5 Editor"),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_xs()
                    .child(
                        Icon::default()
                            .data(include_bytes!("../assets/icons/vcs.svg"))
                            .size_4(),
                    )
                    .child(div().flex_1().min_w_0().child("Version"))
                    .child(env!("CARGO_PKG_VERSION")),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_xs()
                    .child(
                        Icon::default()
                            .data(include_bytes!("../assets/icons/folder-github.svg"))
                            .size_4(),
                    )
                    .child(div().flex_1().min_w_0().child("Source code"))
                    .child(about_link(
                        "about-repository",
                        "Repository",
                        REPOSITORY,
                        &self.repository_focus,
                        window,
                        cx,
                    ))
                    .child(about_link(
                        "about-license",
                        "Apache-2.0 License",
                        "https://github.com/programingjd/json5-editor/blob/main/LICENSE-APACHE",
                        &self.license_focus,
                        window,
                        cx,
                    )),
            )
            .child(credits_separator(cx))
            .child(
                self.credits_section(
                    "Theme",
                    include_bytes!("../assets/icons/palette.svg"),
                    &[],
                    window,
                    cx,
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .text_xs()
                        .italic()
                        .pb_2()
                        .child("Style, colors and icons by")
                        .child(
                            div()
                                .id("jetbrains-link-focus")
                                .cursor_pointer()
                                .track_focus(&self.jetbrains_focus)
                                .on_key_down(|event, window, cx| {
                                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                        window.prevent_default();
                                        cx.stop_propagation();
                                        cx.open_url("https://www.jetbrains.com");
                                    }
                                })
                                .child(
                                    Link::new("jetbrains-link")
                                        .href("https://www.jetbrains.com")
                                        .child(
                                            gpui_kit::img(self.jetbrains_logo.clone())
                                                .size_4()
                                                .cursor_pointer(),
                                        ),
                                ),
                        )
                        .child("JetBrains"),
                )
                .child(self.notice_rows(&self.color_schemes, window, cx))
                .child(self.notice_rows(&self.icons, window, cx)),
            )
            .child(credits_separator(cx))
            .child(self.credits_section(
                "Fonts",
                include_bytes!("../assets/icons/font.svg"),
                &self.fonts,
                window,
                cx,
            ))
            .child(credits_separator(cx))
            .child(
                self.credits_section(
                    "Libraries",
                    include_bytes!("../assets/icons/library.svg"),
                    &self.libraries,
                    window,
                    cx,
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .text_xs()
                        .italic()
                        .mt_2()
                        .child("Copyright and license notices:")
                        .child(about_link(
                            "about-notice",
                            "NOTICE",
                            self.notice_url.clone(),
                            &self.notice_focus,
                            window,
                            cx,
                        )),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .text_xs()
                        .italic()
                        .mt_2()
                        .child("Full bill of material:")
                        .child(about_link(
                            "about-bom",
                            "SBOM",
                            self.sbom_url.clone(),
                            &self.bom_focus,
                            window,
                            cx,
                        )),
                ),
            )
    }

    fn revert_confirmation(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div().flex().flex_col().gap_4()
            .child(div().text_lg().text_color(crate::editor_heading_color(cx)).child("Revert file?"))
            .child(div().text_sm().child("Restore the originally opened content? All edits and undo history will be discarded."))
            .child(div().flex().justify_end().gap_2()
                .child(div().id("cancel-revert-focus").track_focus(&self.cancel_focus)
                    .on_key_down(cx.listener(|this, event: &gpui_kit::KeyDownEvent, window, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            window.prevent_default(); cx.stop_propagation(); this.dismiss(window, cx);
                        }
                    }))
                    .child(crate::bordered_action_button("cancel-revert", "Cancel", cx)
                        .on_click(cx.listener(|this, _, window, cx| { this.dismiss(window, cx); }))))
                .child(crate::bordered_action_button("confirm-revert", "Revert", cx)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.dismiss(window, cx);
                        crate::request_revert_document();
                    }))))
    }
}

pub(super) fn credits_separator(cx: &App) -> gpui_kit::Div {
    let color = cx
        .theme()
        .highlight_theme
        .style
        .editor_invisible
        .unwrap_or_else(|| cx.theme().border.opacity(0.85))
        .opacity(0.5);
    div().w_full().h(gpui_kit::px(1.)).flex_shrink_0().bg(color)
}

fn about_link(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    url: impl Into<SharedString>,
    focus: &FocusHandle,
    window: &Window,
    cx: &App,
) -> impl IntoElement {
    let id = id.into();
    let url = url.into();
    let keyboard_url = url.clone();
    div()
        .id((id.clone(), "keyboard"))
        .cursor_pointer()
        .track_focus(focus)
        .text_xs()
        .when(focus.is_focused(window), |link| {
            link.bg(cx.theme().selection.opacity(0.35))
        })
        .on_key_down(move |event, window, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                window.prevent_default();
                cx.stop_propagation();
                cx.open_url(&keyboard_url);
            }
        })
        .child(Link::new(id).href(url).cursor_pointer().child(label.into()))
}

impl Render for InfoDialogs {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(page) = self.page else {
            return Empty.into_any_element();
        };
        let content = match page {
            InfoPage::Shortcuts => crate::shortcuts::render(cx).into_any_element(),
            InfoPage::About => self.about(window, cx).into_any_element(),
            InfoPage::Revert => self.revert_confirmation(cx).into_any_element(),
        };
        crate::overlay_frame::root("information-overlay", cx)
            .on_key_down(
                cx.listener(|this, event: &gpui_kit::KeyDownEvent, window, cx| {
                    if event.keystroke.key == "escape" {
                        window.prevent_default();
                        cx.stop_propagation();
                        this.dismiss(window, cx);
                    }
                }),
            )
            .child(
                crate::overlay_frame::page("information-page").child(
                    crate::overlay_frame::panel(cx)
                        .w(rems(
                            if matches!(page, InfoPage::About | InfoPage::Shortcuts) {
                                36.
                            } else {
                                42.
                            },
                        ))
                        .gap_4()
                        .track_focus(&self.focus)
                        .when(matches!(page, InfoPage::Shortcuts), |panel| {
                            panel.child(
                                div()
                                    .w_full()
                                    .text_2xl()
                                    .text_center()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(crate::editor_heading_color(cx))
                                    .child("Keyboard Shortcuts"),
                            )
                        })
                        .child(content)
                        .child(
                            Button::new("close-information")
                                .absolute()
                                .top_2()
                                .right_2()
                                .cursor_pointer()
                                .with_size(Size::Small)
                                .custom(crate::toolbar_button_style(cx))
                                .size(rems(2.25))
                                .p(rems(0.46875))
                                .child(
                                    Icon::default()
                                        .data(include_bytes!("../assets/icons/close.svg"))
                                        .size(rems(1.3125))
                                        .cursor_pointer(),
                                )
                                .accessibility_label("Close dialog")
                                .tooltip_with(|window, cx| {
                                    crate::shortcuts::tooltip("Close dialog", "Escape", window, cx)
                                })
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.dismiss(window, cx);
                                })),
                        )
                        .focus_trap("information-focus", &self.focus),
                ),
            )
            .into_any_element()
    }
}
