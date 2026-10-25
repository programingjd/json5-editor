use crate::input::InputModeKind;
use crate::input::{
    Indent, IndentInline, InputBaseState, Outdent, OutdentInline, RopeExt, cursor::CursorSelection,
    element::TextElement, layout::LastLayout, mode::LayoutMode,
};
use gpui::{
    Bounds, Context, Hsla, Path, PathBuilder, Pixels, SharedString, TextRun, TextStyle, Window,
    point, px,
};
use ropey::RopeSlice;

#[derive(Debug, Copy, Clone)]
pub struct TabSize {
    pub tab_size: usize,

    pub hard_tabs: bool,
}

impl Default for TabSize {
    fn default() -> Self {
        Self {
            tab_size: 2,
            hard_tabs: false,
        }
    }
}

impl TabSize {
    pub(super) fn to_string(&self) -> SharedString {
        if self.hard_tabs {
            "\t".into()
        } else {
            " ".repeat(self.tab_size).into()
        }
    }

    pub fn indent_count(&self, line: &RopeSlice) -> usize {
        let mut count = 0;
        for ch in line.chars() {
            match ch {
                '\t' => count += self.tab_size,
                ' ' => count += 1,
                _ => break,
            }
        }
        count
    }
}

impl LayoutMode {
    #[inline]
    pub(super) fn is_indentable(&self) -> bool {
        matches!(
            self,
            LayoutMode::PlainText { .. } | LayoutMode::CodeEditor { .. }
        )
    }

    #[inline]
    pub(super) fn has_indent_guides(&self) -> bool {
        match self {
            LayoutMode::CodeEditor { indent_guides, .. } => *indent_guides,
            _ => false,
        }
    }

    #[inline]
    pub(super) fn tab_size(&self) -> TabSize {
        match self {
            LayoutMode::PlainText { tab, .. } => *tab,
            LayoutMode::CodeEditor { tab, .. } => *tab,
            _ => TabSize::default(),
        }
    }
}

impl<M: InputModeKind> TextElement<M> {
    fn measure_indent_width(&self, style: &TextStyle, column: usize, window: &Window) -> Pixels {
        let font_size = style.font_size.to_pixels(window.rem_size());
        let layout = window.text_system().shape_line(
            SharedString::from(" ".repeat(column)),
            font_size,
            &[TextRun {
                len: column,
                font: style.font(),
                color: Hsla::default(),
                background_color: None,
                strikethrough: None,
                underline: None,
            }],
            None,
        );

        layout.width
    }

    pub(super) fn layout_indent_guides(
        &self,
        state: &InputBaseState<M>,
        bounds: &Bounds<Pixels>,
        last_layout: &LastLayout,
        text_style: &TextStyle,
        window: &mut Window,
    ) -> Option<Path<Pixels>> {
        if !state.is_multi_line() || !state.mode.has_indent_guides() {
            return None;
        }

        let indent_width =
            self.measure_indent_width(text_style, state.mode.tab_size().tab_size, window);

        let tab_size = state.mode.tab_size();
        let line_height = last_layout.line_height;
        let mut builder = PathBuilder::stroke(px(1.));
        let mut offset_y = last_layout.visible_top;
        let mut last_indents = vec![];

        for (&buffer_line, line_layout) in last_layout
            .visible_buffer_lines
            .iter()
            .zip(last_layout.lines.iter())
        {
            let line = state.text.slice_line(buffer_line);
            // A logical line can occupy several visual rows. Carry its scope
            // guides through every continuation, using the same height as text.
            let height = line_layout.size(line_height).height;
            let mut current_indents = vec![];
            if line.len() > 0 {
                let indent_count = tab_size.indent_count(&line);
                for offset in (0..indent_count).step_by(tab_size.tab_size) {
                    let x = if indent_count > 0 {
                        indent_width * offset as f32 / tab_size.tab_size as f32
                    } else {
                        px(0.)
                    };

                    let pos = point(x + last_layout.line_number_width, offset_y);

                    builder.move_to(pos);
                    builder.line_to(point(pos.x, pos.y + height));
                    current_indents.push(pos.x);
                }
            } else if last_indents.len() > 0 {
                for x in &last_indents {
                    let pos = point(*x, offset_y);
                    builder.move_to(pos);
                    builder.line_to(point(pos.x, pos.y + height));
                }
                current_indents = last_indents.clone();
            }

            offset_y += height;
            last_indents = current_indents;
        }

        builder.translate(bounds.origin + point(state.indent_guide_offset, px(0.)));
        let path = builder.build().unwrap();
        Some(path)
    }
}

impl InputBaseState<crate::input::EditorMode> {
    pub fn set_indent_guide_offset(&mut self, offset: Pixels, cx: &mut Context<Self>) {
        if self.indent_guide_offset != offset {
            self.indent_guide_offset = offset;
            cx.notify();
        }
    }

    #[doc(hidden)]
    pub fn indent_guides(mut self, indent_guides: bool) -> Self {
        if let LayoutMode::CodeEditor {
            indent_guides: l, ..
        } = &mut self.mode
        {
            *l = indent_guides;
        }
        self
    }

    pub fn set_indent_guides(
        &mut self,
        indent_guides: bool,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let LayoutMode::CodeEditor {
            indent_guides: l, ..
        } = &mut self.mode
        {
            *l = indent_guides;
        }
        cx.notify();
    }
}

impl<M: InputModeKind> InputBaseState<M> {
    pub(super) fn indent_inline(
        &mut self,
        _: &IndentInline,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // First, try to accept inline completion if present
        if M::accept_inline_completion(self, window, cx) {
            return;
        }
        self.indent(false, window, cx);
    }

    pub(super) fn indent_block(&mut self, _: &Indent, window: &mut Window, cx: &mut Context<Self>) {
        self.indent(true, window, cx);
    }

    pub(super) fn outdent_inline(
        &mut self,
        _: &OutdentInline,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.outdent(false, window, cx);
    }

    pub(super) fn outdent_block(
        &mut self,
        _: &Outdent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.outdent(true, window, cx);
    }

    pub(super) fn indent(&mut self, block: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_indent(IndentDirection::Indent, block, window, cx);
    }

    pub(super) fn outdent(&mut self, block: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_indent(IndentDirection::Outdent, block, window, cx);
    }

    fn apply_indent(
        &mut self,
        direction: IndentDirection,
        block: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.is_editable() || !self.is_multi_line() || !self.mode.is_indentable() {
            cx.propagate();
            return;
        }

        let tab_indent = self.mode.tab_size().to_string();
        let tab_len = tab_indent.len();

        // Non-collapsed selections and explicit block operations indent whole lines.
        let has_non_collapsed = self.selections.iter().any(|sel| !sel.is_collapsed());
        let use_block = has_non_collapsed || block;

        let before: Vec<CursorSelection> = self.selections.iter().copied().collect();

        let (edits, new_selections) = if use_block {
            self.compute_block_indent(direction, &tab_indent, tab_len)
        } else {
            self.compute_inline_indent(direction, tab_len)
        };

        if edits.is_empty() {
            return;
        }

        self.undo_manager.begin_transaction();
        self.replace_text_in_ranges(&edits, window, cx);
        self.selections.replace_all(new_selections);
        let after: Vec<CursorSelection> = self.selections.iter().copied().collect();
        self.undo_manager.record_selections(before, after);
        self.undo_manager.commit_transaction();

        self.scroll_to(self.cursor(), None, cx);
        cx.notify();
    }

    fn compute_block_indent(
        &self,
        direction: IndentDirection,
        tab_indent: &str,
        tab_len: usize,
    ) -> (Vec<(std::ops::Range<usize>, String)>, Vec<CursorSelection>) {
        let mut rows: std::collections::HashSet<usize> = std::collections::HashSet::new();
        for sel in self.selections.iter() {
            let start_row = self.text.offset_to_point(sel.start).row;
            let end_row = self.text.offset_to_point(sel.end).row;
            for row in start_row..=end_row {
                rows.insert(row);
            }
        }

        let mut rows: Vec<usize> = rows.into_iter().collect();
        rows.sort_unstable();

        let mut edits: Vec<(std::ops::Range<usize>, String)> = Vec::new();
        for row in rows {
            let line_start = self.text.line_start_offset(row);
            match direction {
                IndentDirection::Indent => {
                    edits.push((line_start..line_start, tab_indent.to_string()));
                }
                IndentDirection::Outdent => {
                    if self
                        .text
                        .slice(line_start..)
                        .chars()
                        .take(tab_indent.chars().count())
                        .eq(tab_indent.chars())
                    {
                        edits.push((line_start..line_start + tab_len, String::new()));
                    }
                }
            }
        }

        // Map both endpoints through every earlier edit, including edits belonging
        // to other cursors. A point inside removed indentation stays on its line.
        let map_offset = |offset: usize| match direction {
            IndentDirection::Indent => {
                offset + edits.partition_point(|(range, _)| range.start <= offset) * tab_len
            }
            IndentDirection::Outdent => {
                let preceding = edits.partition_point(|(range, _)| range.end <= offset);
                let partial = edits
                    .get(preceding)
                    .map_or(0, |(range, _)| offset.saturating_sub(range.start));
                offset - preceding * tab_len - partial
            }
        };
        let new_selections = self
            .selections
            .iter()
            .map(|sel| {
                let mut selection = *sel;
                selection.start = map_offset(sel.start);
                selection.end = map_offset(sel.end);
                selection.column_anchor = None;
                selection
            })
            .collect();

        (edits, new_selections)
    }

    fn compute_inline_indent(
        &self,
        direction: IndentDirection,
        tab_len: usize,
    ) -> (Vec<(std::ops::Range<usize>, String)>, Vec<CursorSelection>) {
        let tab_indent = self.mode.tab_size().to_string();

        // The edit range for each cursor: an insertion point for indent, the
        // removed range for a removable outdent.
        let mut ranges: Vec<std::ops::Range<usize>> = Vec::with_capacity(self.selections.len());
        for sel in self.selections.iter() {
            let cursor = sel.cursor_offset();
            match direction {
                IndentDirection::Indent => ranges.push(cursor..cursor),
                IndentDirection::Outdent => {
                    let row = self.text.offset_to_point(cursor).row;
                    let start = self.text.line_start_offset(row);
                    if self
                        .text
                        .slice(start..)
                        .chars()
                        .take(tab_indent.chars().count())
                        .eq(tab_indent.chars())
                    {
                        ranges.push(start..start + tab_len);
                    }
                }
            }
        }

        // Build disjoint edits, dropping any that would overlap a previous one.
        ranges.sort_by_key(|range| range.start);
        let mut edits: Vec<(std::ops::Range<usize>, String)> = Vec::new();
        let mut last_end: Option<usize> = None;
        for range in ranges {
            if let Some(last_end) = last_end {
                if range.start < last_end {
                    continue;
                }
            }
            last_end = Some(range.end);
            let text = match direction {
                IndentDirection::Indent => tab_indent.to_string(),
                IndentDirection::Outdent => String::new(),
            };
            edits.push((range, text));
        }

        // Shift every cursor by the surviving edits before (or at) it. Cursors
        // whose own edit was dropped or not applicable keep their position.
        let mut new_selections: Vec<CursorSelection> = Vec::with_capacity(self.selections.len());
        for sel in self.selections.iter() {
            let cursor = sel.cursor_offset();
            let new_offset = match direction {
                IndentDirection::Indent => {
                    let inserted_before = edits
                        .iter()
                        .filter(|(range, _)| range.start <= cursor)
                        .count();
                    cursor + inserted_before * tab_len
                }
                IndentDirection::Outdent => {
                    let removed_before: usize = edits
                        .iter()
                        .map(|(range, _)| range.end.min(cursor) - range.start.min(cursor))
                        .sum();
                    cursor - removed_before
                }
            };
            let mut selection = CursorSelection::new(sel.id, new_offset, new_offset);
            selection.column_anchor = None;
            new_selections.push(selection);
        }

        (edits, new_selections)
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
enum IndentDirection {
    Indent,
    Outdent,
}

impl<M: crate::input::MultiLineMode> InputBaseState<M> {
    #[doc(hidden)]
    pub fn tab_size(mut self, tab: TabSize) -> Self {
        match &mut self.mode {
            LayoutMode::PlainText { tab: t, .. } => *t = tab,
            LayoutMode::CodeEditor { tab: t, .. } => *t = tab,
            _ => {}
        }
        self
    }

    pub fn set_tab_size(&mut self, tab: TabSize, cx: &mut Context<Self>) {
        match &mut self.mode {
            LayoutMode::PlainText { tab: value, .. }
            | LayoutMode::CodeEditor { tab: value, .. } => *value = tab,
            _ => {}
        }
        cx.notify();
    }
}
