use gpui_kit::base::input::RopeExt as _;
use gpui_kit::component::{
    ActiveTheme as _, Sizable as _, Size,
    button::{Button, ButtonVariants as _},
    input::{EditorState, Input, InputEvent, InputState, Position},
    status_bar::StatusBar,
};
use gpui_kit::{
    App, AppContext as _, Context, Entity, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, Styled as _, Subscription, Window, div,
};
use ropey::{LineType, Rope};

pub(super) struct EditorStatusBar {
    editor: Entity<EditorState>,
    status: CaretStatus,
    entry: Option<Entity<InputState>>,
    entry_initial_value: Option<String>,
    _entry_subscription: Option<Subscription>,
    _editor_observer: Subscription,
}

impl EditorStatusBar {
    pub(super) fn new(editor: &Entity<EditorState>, cx: &mut App) -> Entity<Self> {
        cx.new(|cx: &mut Context<Self>| Self {
            editor: editor.clone(),
            status: CaretStatus::from_editor(editor.read(cx)),
            entry: None,
            entry_initial_value: None,
            _entry_subscription: None,
            _editor_observer: cx.observe(editor, |this, editor, cx| {
                let status = CaretStatus::from_editor(editor.read(cx));
                if this.status != status {
                    this.status = status;
                    cx.notify();
                }
            }),
        })
    }

    fn begin_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let destination = CaretDestination::at_cursor(self.editor.read(cx));
        let entry = cx.new(|cx| InputState::new(window, cx).placeholder("Line:column"));
        self._entry_subscription = Some(cx.subscribe_in(&entry, window, Self::on_entry_event));
        self.entry = Some(entry);
        self.entry_initial_value = Some(format!(
            "{}:{}",
            destination.row + 1,
            destination.column + 1
        ));
        cx.notify();
    }

    fn initialize_entry(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(value) = self.entry_initial_value.take() else {
            return;
        };
        let Some(entry) = self.entry.clone() else {
            return;
        };
        // The first styled layout installs Inter in the input's display map.
        // set_value measures text immediately, so it must run after that layout,
        // rather than using the unregistered .SystemUIFont from the click event.
        cx.on_next_frame(window, move |this, window, cx| {
            if this.entry.as_ref() != Some(&entry) {
                return;
            }
            entry.update(cx, |state, cx| {
                state.set_value(value, window, cx);
                state.select_all(window, cx);
                state.focus(window, cx);
            });
        });
    }

    fn on_entry_event(
        &mut self,
        entry: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.entry.as_ref() != Some(entry) {
            return;
        }
        if let InputEvent::Blur = event {
            self.finish_edit(false, window, cx);
        }
    }

    fn finish_edit(&mut self, restore_focus: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.entry.take().is_none() {
            return;
        }
        self.entry_initial_value = None;
        self._entry_subscription = None;
        if restore_focus {
            self.editor.update(cx, |state, cx| state.focus(window, cx));
        }
        cx.notify();
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = &self.entry else {
            return;
        };
        let destination = CaretDestination::parse(entry.read(cx).value().as_ref());
        // Close before changing focus, so the resulting Blur cannot cancel a
        // newer edit or steal focus from the destination.
        self.finish_edit(false, window, cx);
        self.editor.update(cx, |state, cx| {
            if let Some(destination) = destination {
                let position = destination.position_in(state.text());
                state.unfold_at(position, cx);
                state.set_cursor_position(position, window, cx);
                state.center_cursor(cx);
            } else {
                state.focus(window, cx);
            }
        });
    }
}

impl Render for EditorStatusBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = if let Some(entry) = &self.entry {
            let owner = cx.weak_entity();
            div()
                .w_40()
                .on_children_prepainted(move |_, window, cx| {
                    let _ = owner.update(cx, |this, cx| this.initialize_entry(window, cx));
                })
                .capture_action(cx.listener(
                    |this, _: &gpui_kit::base::input::Enter, window, cx| {
                        // Single-line Input normally propagates Enter to a dialog.
                        // Consume it here before restoring editor focus.
                        cx.stop_propagation();
                        this.submit(window, cx);
                    },
                ))
                .capture_action(cx.listener(
                    |this, _: &gpui_kit::base::input::Escape, window, cx| {
                        cx.stop_propagation();
                        this.finish_edit(true, window, cx);
                    },
                ))
                .child(
                    Input::new(entry)
                        .with_size(Size::XSmall)
                        .appearance(false)
                        .font_family(cx.theme().font_family.clone())
                        .w_full()
                        .aria_label("Go to line and column"),
                )
                .into_any_element()
        } else {
            Button::new("caret-position")
                .cursor_pointer()
                .ghost()
                .with_size(Size::XSmall)
                .text_xs()
                .font_weight(FontWeight::NORMAL)
                .text_color(cx.theme().muted_foreground)
                .label(self.status.label())
                .accessibility_label("Go to line and column")
                .tooltip("Go to line or line:column")
                .on_click(cx.listener(|this, _, window, cx| this.begin_edit(window, cx)))
                .into_any_element()
        };
        StatusBar::new()
            .w_full()
            .flex_none()
            .bg(cx.theme().muted)
            .font_family(cx.theme().font_family.clone())
            .left(content)
    }
}

struct CaretDestination {
    row: usize,
    column: usize,
}

impl CaretDestination {
    fn at_cursor(editor: &EditorState) -> Self {
        let text = editor.text();
        let cursor = editor.cursor();
        let point = text.offset_to_point(cursor);
        Self {
            row: point.row,
            column: text
                .slice(text.line_start_offset(point.row)..cursor)
                .len_chars(),
        }
    }

    fn parse(value: &str) -> Option<Self> {
        let (row, column) = match value.split_once(':') {
            Some((row, column)) => (row, Some(column)),
            None => (value, None),
        };
        Some(Self {
            row: Self::parse_coordinate(row)?,
            column: match column {
                Some(column) => Self::parse_coordinate(column)?,
                None => 0,
            },
        })
    }

    fn parse_coordinate(value: &str) -> Option<usize> {
        let value = value.trim();
        let (negative, digits) = match value.as_bytes().first() {
            Some(b'-') => (true, &value[1..]),
            Some(b'+') => (false, &value[1..]),
            _ => (false, value),
        };
        if digits.is_empty() {
            return None;
        }
        let mut number = 0usize;
        for digit in digits.bytes() {
            if !digit.is_ascii_digit() {
                return None;
            }
            // An arbitrarily large integer is still a valid destination: it
            // clamps to the last line/column rather than overflowing.
            number = number
                .saturating_mul(10)
                .saturating_add((digit - b'0') as usize);
        }
        Some(if negative {
            0
        } else {
            number.saturating_sub(1)
        })
    }

    fn position_in(&self, text: &Rope) -> Position {
        let row = self.row.min(text.lines_len().saturating_sub(1));
        let line = text.slice_line(row);
        // slice_line removes LF; exclude the CR of CRLF as well so a jump
        // cannot place the caret inside a line terminator.
        let end = line.len();
        let content = if end > 0 && line.is_char_boundary(end - 1) && line.char(end - 1) == '\r' {
            line.slice(..end - 1)
        } else {
            line
        };
        let column = self.column.min(content.len_chars());
        Position::new(row as u32, column as u32)
    }
}

#[derive(PartialEq, Eq)]
enum CaretStatus {
    Single {
        row: usize,
        column: usize,
        chars: usize,
        line_breaks: usize,
    },
    Multiple(usize),
}

impl CaretStatus {
    fn from_editor(editor: &EditorState) -> Self {
        let carets = editor.caret_count();
        if carets > 1 {
            return Self::Multiple(carets);
        }

        let text = editor.text();
        let destination = CaretDestination::at_cursor(editor);
        let selected = text.slice(editor.selected_range());
        // Rope metrics count even a large folded selection without copying or
        // scanning it. Columns count Unicode characters rather than UTF-8 bytes.
        Self::Single {
            row: destination.row + 1,
            column: destination.column + 1,
            chars: selected.len_chars(),
            line_breaks: selected.len_lines(LineType::LF).saturating_sub(1),
        }
    }

    fn label(&self) -> String {
        match self {
            Self::Multiple(carets) => format!("{carets} carets"),
            Self::Single {
                row,
                column,
                chars: 0,
                ..
            } => format!("{row}:{column}"),
            Self::Single {
                row,
                column,
                chars,
                line_breaks: 0,
            } => {
                format!("{row}:{column} ({chars} chars)")
            }
            Self::Single {
                row,
                column,
                chars,
                line_breaks,
            } => {
                format!("{row}:{column} ({chars} chars, {line_breaks} line breaks)")
            }
        }
    }
}
