use crate::input::InputModeKind;
use std::ops::Range;

use gpui::{Context, Window};
use ropey::Rope;
use sum_tree::Bias;

use super::{InputBaseState, RopeExt as _, cursor::CursorSelection};
use crate::text_boundary::word_range_from_chars;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub(super) struct CursorId(usize);

impl CursorId {
    pub(super) fn new(id: usize) -> Self {
        Self(id)
    }
}

impl<M: InputModeKind> InputBaseState<M> {
    pub(super) fn select_word(&mut self, offset: usize, _: &mut Window, cx: &mut Context<Self>) {
        // A masked value renders as one unbroken run of mask characters, so it
        // has no word boundaries to select by. Take all of it instead, rather
        // than let the selection highlight reveal where the words are.
        let range = if self.masked {
            0..self.text.len()
        } else {
            let Some(range) = TextSelector::word_range(&self.text, offset) else {
                return;
            };
            range
        };

        self.undo_manager.break_transaction_coalescing();
        self.selections.remove_all_but_active();
        self.set_selection(range.start, range.end);
        self.active_selection_mut().reversed = false;
        self.selected_word_range = Some(*self.active_selection());
        cx.notify()
    }

    pub(super) fn select_word_to(&mut self, offset: usize, anchor: CursorSelection) {
        let (range, reversed) =
            TextSelector::word_drag_range(&self.text, anchor.start..anchor.end, offset);
        let selection = self.active_selection_mut();
        selection.start = range.start;
        selection.end = range.end;
        selection.reversed = reversed;
        selection.column_anchor = None;
    }

    pub(super) fn select_line(&mut self, offset: usize, _: &mut Window, cx: &mut Context<Self>) {
        let range = TextSelector::line_range(&self.text, offset);
        self.undo_manager.break_transaction_coalescing();
        self.selections.remove_all_but_active();
        self.set_selection(range.start, range.end);
        self.selected_word_range = None;
        cx.notify()
    }
}

struct TextSelector;
impl TextSelector {
    fn word_drag_range(text: &Rope, anchor: Range<usize>, offset: usize) -> (Range<usize>, bool) {
        let offset = text.clip_offset(offset, Bias::Left);
        // Always rebuild from the originally clicked word. Extending the
        // previous selection loses its anchor when the drag changes sides.
        if offset < anchor.start {
            let start = Self::word_range(text, offset).map_or(offset, |range| range.start);
            (start..anchor.end, true)
        } else if offset > anchor.end {
            let end = Self::word_range(text, offset).map_or(offset, |range| range.end);
            (anchor.start..end, false)
        } else {
            (anchor, false)
        }
    }

    pub(crate) fn line_range(text: &Rope, offset: usize) -> Range<usize> {
        let offset = text.clip_offset(offset, Bias::Left);
        let row = text.offset_to_point(offset).row;
        let start = text.line_start_offset(row);
        let end = text.line_end_offset(row);

        start..end
    }

    pub(crate) fn word_range(text: &Rope, offset: usize) -> Option<Range<usize>> {
        let offset = text.clip_offset(offset, Bias::Left);
        let Some(char) = text.char_at(offset) else {
            return None;
        };

        let end = offset + char.len_utf8();
        let prev_chars = text.chars_at(offset).reversed().take(128);
        let next_chars = text.chars_at(end).take(128);
        Some(word_range_from_chars(offset, char, prev_chars, next_chars))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_drag_keeps_original_word_when_reversing_and_shrinking() {
        let text = Rope::from("aaaa-bbb-ccc-dddd");
        let anchor = 5..8;
        // One gesture can cross the anchor repeatedly and return to it.
        for (offset, expected, reversed) in [
            (10, 5..12, false),
            (14, 5..17, false),
            (10, 5..12, false),
            (1, 0..8, true),
            (7, 5..8, false),
            (14, 5..17, false),
            (5, 5..8, false),
            (8, 5..8, false),
        ] {
            assert_eq!(
                TextSelector::word_drag_range(&text, anchor.clone(), offset),
                (expected, reversed)
            );
        }
    }

    #[test]
    fn word_drag_handles_document_edges_and_empty_text() {
        let text = Rope::from("aaaa-bbb-ccc-dddd");
        assert_eq!(TextSelector::word_drag_range(&text, 5..8, 0), (0..8, true));
        assert_eq!(
            TextSelector::word_drag_range(&text, 5..8, 100),
            (5..17, false)
        );
        assert_eq!(
            TextSelector::word_drag_range(&Rope::new(), 0..0, 0),
            (0..0, false)
        );
    }

    #[test]
    fn word_drag_uses_utf8_boundaries_and_crosses_lines() {
        let text = Rope::from("été bbb\r\n  café");
        assert_eq!(TextSelector::word_drag_range(&text, 6..9, 3), (0..9, true));
        assert_eq!(
            TextSelector::word_drag_range(&text, 6..9, 16),
            (6..18, false)
        );
        assert_eq!(
            TextSelector::word_drag_range(&text, 6..9, 11),
            (6..13, false)
        );
    }
}
