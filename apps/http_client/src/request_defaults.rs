use std::sync::atomic::{AtomicU64, Ordering};

use gpui_kit::base::{FocusTrapElement as _, StyledExt as _, input::InputEvent};
use gpui_kit::component::{
    ActiveTheme as _, Sizable as _, Size,
    button::{Button, ButtonCustomVariant, ButtonVariants as _},
    input::InputState,
    menu::DropdownMenu as _,
};
use gpui_kit::{
    Anchor, AnyElement, App, AppContext as _, ClickEvent, Context, Div, Entity, FocusHandle,
    Focusable, InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Render,
    StatefulInteractiveElement as _, Styled as _, Subscription, WeakEntity, Window,
    accesskit::Role, div, prelude::FluentBuilder as _, px, rems,
};
use serde::{Deserialize, Serialize};

use crate::{
    buttons::{
        action_button, checked_menu_item, field_button, field_button_icon, field_input,
        inheritance_button, modal_overlay, small_icon,
    },
    client::Workspace,
    palette,
    request_fields::{RequestFields, param_field, param_placeholder_color},
};

const DEFAULT_TIMEOUT_SECONDS: u32 = 30;
const CLOSE_ICON: &[u8] = include_bytes!("../../../assets/icons/close.svg");
const CHEVRON_ICON: &[u8] = include_bytes!("../../../assets/icons/chevron-down.svg");

static NEXT_DEFAULT_HEADER_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct DefaultHeaderId(u64);

impl DefaultHeaderId {
    pub(super) fn next() -> Self {
        Self(NEXT_DEFAULT_HEADER_ID.fetch_add(1, Ordering::Relaxed))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct DefaultHeader {
    pub(super) id: DefaultHeaderId,
    pub(super) name: String,
    pub(super) value: String,
}

pub(super) fn parse_header_line(line: &str) -> Option<(&str, &str)> {
    let (name, value) = line.split_once(':').unwrap_or((line, ""));
    let name = name.trim();
    (!name.is_empty()).then_some((name, value.trim()))
}

#[derive(Clone, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub(super) struct RequestDefaults {
    pub(super) timeout_seconds: Option<u32>,
    pub(super) follow_redirects: bool,
    headers: String,
    #[serde(skip)]
    default_headers: Vec<DefaultHeader>,
    #[serde(skip)]
    live_header_ids: Option<Vec<DefaultHeaderId>>,
}

impl Default for RequestDefaults {
    fn default() -> Self {
        let mut defaults = Self {
            timeout_seconds: None,
            follow_redirects: true,
            headers: concat!(
                "User-Agent: JSON5-HTTP-Client/",
                env!("CARGO_PKG_VERSION"),
                "\nAccept: */*"
            )
            .into(),
            default_headers: Vec::new(),
            live_header_ids: None,
        };
        defaults.rebuild_default_headers();
        defaults
    }
}

impl RequestDefaults {
    pub(super) fn from_storage(value: &str) -> Self {
        let mut defaults: Self = serde_json::from_str(value).unwrap_or_default();
        defaults.timeout_seconds = defaults.timeout_seconds.filter(|s| (1..=999).contains(s));
        defaults.rebuild_default_headers();
        defaults
    }

    pub(super) fn from_parts(
        timeout_seconds: Option<u32>,
        follow_redirects: bool,
        headers: String,
        default_headers: Vec<DefaultHeader>,
        live_header_ids: Option<Vec<DefaultHeaderId>>,
    ) -> Self {
        Self {
            timeout_seconds,
            follow_redirects,
            headers,
            default_headers,
            live_header_ids,
        }
    }

    pub(super) fn default_headers(&self) -> &[DefaultHeader] {
        &self.default_headers
    }

    pub(super) fn is_header_id_live(&self, id: DefaultHeaderId) -> bool {
        self.default_headers.iter().any(|entry| entry.id == id)
            || self
                .live_header_ids
                .as_ref()
                .is_some_and(|ids| ids.contains(&id))
    }

    pub(super) fn same_storage(&self, other: &Self) -> bool {
        self.timeout_seconds == other.timeout_seconds
            && self.follow_redirects == other.follow_redirects
            && self.headers == other.headers
    }

    pub(super) fn clear_live_header_ids(&mut self) -> bool {
        let Some(live_ids) = self.live_header_ids.take() else {
            return false;
        };
        live_ids
            .into_iter()
            .any(|id| !self.default_headers.iter().any(|entry| entry.id == id))
    }

    fn rebuild_default_headers(&mut self) {
        self.default_headers = self
            .headers
            .lines()
            .filter_map(parse_header_line)
            .map(|(name, value)| DefaultHeader {
                id: DefaultHeaderId::next(),
                name: name.to_owned(),
                value: value.to_owned(),
            })
            .collect();
        self.live_header_ids = None;
    }
}

pub(super) type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

pub(super) struct SettingInheritance {
    pub(super) inherited: bool,
    pub(super) on_click: ClickHandler,
}

pub(super) fn timeout_input(
    seconds: Option<u32>,
    window: &mut Window,
    cx: &mut Context<InputState>,
) -> InputState {
    InputState::new(window, cx)
        .placeholder(DEFAULT_TIMEOUT_SECONDS.to_string())
        .default_value(seconds.unwrap_or(DEFAULT_TIMEOUT_SECONDS).to_string())
        .validate(|text, _| valid_timeout(text))
}

pub(super) fn set_timeout_input(
    timeout: &Entity<InputState>,
    seconds: Option<u32>,
    window: &mut Window,
    cx: &mut App,
) {
    let value = seconds.unwrap_or(DEFAULT_TIMEOUT_SECONDS).to_string();
    if timeout.read(cx).value().as_str() != value {
        timeout.update(cx, |state, cx| state.set_value(value, window, cx));
    }
}

pub(super) fn timeout_seconds(
    timeout: &Entity<InputState>,
    enabled: bool,
    cx: &App,
) -> Option<u32> {
    enabled.then(|| {
        timeout
            .read(cx)
            .value()
            .parse()
            .unwrap_or(DEFAULT_TIMEOUT_SECONDS)
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn request_settings_controls(
    timeout: &Entity<InputState>,
    timeout_enabled: bool,
    follow_redirects: bool,
    on_timeout_change: impl Fn(bool, &mut Window, &mut App) + Clone + 'static,
    on_redirect_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    timeout_inheritance: Option<SettingInheritance>,
    redirect_inheritance: Option<SettingInheritance>,
    cx: &App,
) -> Div {
    let timeout_action = timeout_inheritance.map(|inheritance| {
        inheritance_button(
            "timeout-inheritance",
            inheritance.inherited,
            "Override inherited timeout",
            "Reset timeout to default",
            inheritance.on_click,
            cx,
        )
        .into_any_element()
    });
    let redirect_action = redirect_inheritance.map(|inheritance| {
        inheritance_button(
            "follow-redirects-inheritance",
            inheritance.inherited,
            "Override inherited follow redirects",
            "Reset follow redirects to default",
            inheritance.on_click,
            cx,
        )
        .into_any_element()
    });
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap_3()
        .text_sm()
        .child(setting_row(
            "Timeout",
            timeout_field(timeout, timeout_enabled, on_timeout_change, cx),
            timeout_action,
        ))
        .child(divider(cx))
        .child(setting_row(
            "Follow redirect",
            redirect_switch(follow_redirects, cx).on_click(on_redirect_click),
            redirect_action,
        ))
}

fn setting_row(
    label: &'static str,
    control: impl IntoElement,
    trailing: Option<AnyElement>,
) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_3()
        .child(label)
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(control)
                .children(trailing),
        )
}

fn divider(cx: &App) -> Div {
    div()
        .w_full()
        .h(px(1.))
        .bg(palette::get(cx).border.opacity(0.6))
}

fn valid_timeout(text: &str) -> bool {
    text.is_empty()
        || (text.len() <= 3
            && text.bytes().all(|byte| byte.is_ascii_digit())
            && text.parse::<u32>().is_ok_and(|seconds| seconds > 0))
}

fn timeout_field(
    timeout: &Entity<InputState>,
    enabled: bool,
    on_change: impl Fn(bool, &mut Window, &mut App) + Clone + 'static,
    cx: &App,
) -> Div {
    let input = timeout.clone();
    let muted = cx.theme().muted_foreground;
    let placeholder = param_placeholder_color(cx);
    let chevron = field_button_icon(CHEVRON_ICON, rems(0.75), muted, true);
    let menu = field_button("timeout-options", cx)
        .with_size(Size::Small)
        .cursor_pointer()
        .accessibility_label(if enabled {
            "Timeout options"
        } else {
            "No timeout"
        })
        .map(|button| {
            if enabled {
                button.min_w_6().flex_none().child(chevron)
            } else {
                button.w_full().px_2().child(
                    div()
                        .w_full()
                        .flex()
                        .items_center()
                        .justify_between()
                        .text_color(placeholder)
                        .child("No timeout")
                        .child(chevron),
                )
            }
        })
        .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
            [(true, "Edit"), (false, "No timeout")].into_iter().fold(
                menu.with_size(Size::Small),
                |menu, (edit, label)| {
                    let on_change = on_change.clone();
                    let input = input.clone();
                    let item = checked_menu_item(label, enabled == edit);
                    menu.item(item.on_click(move |_, window, cx| {
                        on_change(edit, window, cx);
                        if edit {
                            let focus = input.read(cx).focus_handle(cx);
                            window.on_next_frame(move |window, cx| focus.focus(window, cx));
                        }
                    }))
                },
            )
        });
    param_field(!enabled, cx)
        .flex_none()
        .w(rems(if enabled { 5.25 } else { 6.5 }))
        .when(enabled, |field| {
            field
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(field_input(timeout, "Timeout in seconds")),
                )
                .child(div().text_color(muted).child("s"))
        })
        .when(!enabled, |field| {
            field.flex_col().items_stretch().justify_center()
        })
        .child(menu)
}

fn redirect_switch(enabled: bool, cx: &App) -> Button {
    let palette = palette::get(cx);
    let track = if enabled {
        palette.selection
    } else {
        palette.hover
    };
    Button::new("follow-redirects")
        .custom(
            ButtonCustomVariant::new(cx)
                .color(track)
                .hover(track.opacity(0.8))
                .active(track.opacity(0.7)),
        )
        .role(Role::Switch)
        .accessibility_label("Follow redirects")
        .toggled(enabled)
        .cursor_pointer()
        .w_8()
        .h_4()
        .p_0p5()
        .rounded_full()
        .child(
            div()
                .w_full()
                .flex()
                .when(enabled, |row| row.justify_end())
                .child(div().size_3().rounded_full().bg(palette.icon)),
        )
}

pub(super) struct RequestDefaultsDialog {
    focus: FocusHandle,
    timeout: Entity<InputState>,
    timeout_enabled: bool,
    follow_redirects: bool,
    headers: Entity<RequestFields>,
    workspace: WeakEntity<Workspace>,
    last_saved: RequestDefaults,
    save_failed: bool,
    _timeout_subscription: Subscription,
    _headers_observation: Subscription,
}

impl RequestDefaultsDialog {
    pub(super) fn new(
        defaults: &RequestDefaults,
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let timeout = cx.new(|cx| timeout_input(defaults.timeout_seconds, window, cx));
        let headers =
            cx.new(|cx| RequestFields::default_headers(defaults.default_headers(), window, cx));
        let timeout_subscription =
            cx.subscribe_in(&timeout, window, |this, _, event, window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.persist(window, cx);
                }
            });
        let headers_observation = cx.observe_in(&headers, window, |this, _, window, cx| {
            this.persist(window, cx)
        });
        Self {
            focus: cx.focus_handle(),
            timeout,
            timeout_enabled: defaults.timeout_seconds.is_some(),
            follow_redirects: defaults.follow_redirects,
            headers,
            workspace,
            last_saved: defaults.clone(),
            save_failed: false,
            _timeout_subscription: timeout_subscription,
            _headers_observation: headers_observation,
        }
    }

    fn persist(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (headers, default_headers, live_ids) =
            self.headers.read(cx).default_headers_snapshot(cx);
        let defaults = RequestDefaults::from_parts(
            timeout_seconds(&self.timeout, self.timeout_enabled, cx),
            self.follow_redirects,
            headers,
            default_headers,
            Some(live_ids),
        );
        if !self.save_failed && defaults == self.last_saved {
            return;
        }
        let persist_to_storage = self.save_failed || !defaults.same_storage(&self.last_saved);
        self.save_failed = !self
            .workspace
            .update(cx, |workspace, cx| {
                workspace.save_defaults(defaults.clone(), persist_to_storage, window, cx)
            })
            .is_ok_and(|saved| saved);
        if !self.save_failed {
            self.last_saved = defaults;
        }
        cx.notify();
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let _ = self
            .workspace
            .update(cx, |workspace, cx| workspace.close_settings(window, cx));
    }
}

impl Focusable for RequestDefaultsDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for RequestDefaultsDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let viewport = window.viewport_size();
        let palette = palette::get(cx);
        let owner = cx.weak_entity();
        let controls = request_settings_controls(
            &self.timeout,
            self.timeout_enabled,
            self.follow_redirects,
            move |enabled, window, cx| {
                let _ = owner.update(cx, |this, cx| {
                    this.timeout_enabled = enabled;
                    this.persist(window, cx);
                    cx.notify();
                });
            },
            cx.listener(|this, _, window, cx| {
                this.follow_redirects = !this.follow_redirects;
                this.persist(window, cx);
                cx.notify();
            }),
            None,
            None,
            cx,
        );
        let dialog = div()
            .id("request-defaults-dialog")
            .role(Role::Dialog)
            .aria_label("Request defaults")
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    window.prevent_default();
                    cx.stop_propagation();
                    this.close(window, cx);
                }
            }))
            .w(rems(32.))
            .max_w(viewport.width * 0.9)
            .max_h(viewport.height * 0.9)
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .bg(palette.background)
            .text_color(palette.foreground)
            .border_1()
            .border_color(cx.theme().border)
            .rounded(cx.theme().radius)
            .text_sm()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(div().font_semibold().child("Request defaults"))
                    .child(
                        action_button("close-request-defaults", cx)
                            .with_size(Size::Small)
                            .accessibility_label("Close request defaults")
                            .icon(small_icon(CLOSE_ICON, palette.icon))
                            .on_click(cx.listener(|this, _, window, cx| this.close(window, cx))),
                    ),
            )
            .child(controls)
            .child(divider(cx))
            .child(
                div().flex_none().child("Default headers").child(
                    div()
                        .h(rems(8.)
                            .to_pixels(window.rem_size())
                            .min(viewport.height * 0.35))
                        .child(self.headers.clone()),
                ),
            )
            .when(self.save_failed, |panel| {
                panel.child(
                    div()
                        .text_color(cx.theme().danger)
                        .child("Could not save request defaults."),
                )
            })
            .focus_trap("request-defaults-focus-trap", &self.focus);
        modal_overlay("request-defaults-backdrop", dialog, window, cx)
    }
}

#[cfg(test)]
mod tests {
    use super::{DefaultHeaderId, RequestDefaults};

    #[test]
    fn saved_settings_replace_the_built_in_defaults() {
        let defaults = RequestDefaults::from_storage(
            r#"{"timeout_seconds":45,"follow_redirects":false,"headers":""}"#,
        );
        assert_eq!(defaults.timeout_seconds, Some(45));
        assert!(!defaults.follow_redirects);
        assert!(defaults.default_headers().is_empty());

        let built_in = RequestDefaults::from_storage("");
        assert_eq!(built_in.timeout_seconds, None);
        assert!(built_in.follow_redirects);
        assert!(
            built_in
                .default_headers()
                .iter()
                .any(|header| header.name == "Accept" && header.value == "*/*")
        );
    }

    #[test]
    fn duplicate_default_headers_receive_distinct_ids() {
        let defaults = RequestDefaults::from_storage(
            r#"{"headers":"X-Tag: one\nx-tag: two\n  \nUser-Agent: custom"}"#,
        );
        let headers = defaults.default_headers();
        assert_eq!(headers.len(), 3);
        assert_ne!(headers[0].id, headers[1].id);
        assert_ne!(headers[1].id, headers[2].id);
        assert_ne!(headers[0].id, headers[2].id);
    }

    #[test]
    fn clear_live_header_ids_reports_dropped_empty_rows() {
        let mut defaults = RequestDefaults::from_storage(r#"{"headers":"Accept: */*"}"#);
        let existing_id = defaults.default_headers()[0].id;
        let transient_id = DefaultHeaderId::next();
        defaults = RequestDefaults::from_parts(
            defaults.timeout_seconds,
            defaults.follow_redirects,
            "Accept: */*".into(),
            defaults.default_headers().to_vec(),
            Some(vec![existing_id, transient_id]),
        );
        assert!(defaults.is_header_id_live(transient_id));
        assert!(defaults.clear_live_header_ids());
        assert!(!defaults.is_header_id_live(transient_id));
        assert!(defaults.is_header_id_live(existing_id));
    }
}
