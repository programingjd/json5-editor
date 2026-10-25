use std::{cell::RefCell, ops::Range, rc::Rc};

use gpui_kit::base::input::{Diagnostic, DiagnosticSeverity, RopeExt};
use gpui_kit::component::input::{
    EditorState, FoldRange, HighlightStyleResolver, InputEdit, InputHighlighter,
};
use gpui_kit::{Context, HighlightStyle, SharedString, Window};
use ropey::Rope;

use crate::syntax_data::{DiagnosticData, SEMANTIC_NAMES};

#[derive(Default)]
pub(super) struct ParseSnapshot {
    pub revision: u32,
    pub version: u64,
    pub installing: bool,
    pub diagnostics: Vec<Diagnostic>,
    pub folds: Vec<FoldRange>,
    highlights: Vec<(Range<usize>, u32)>,
    // Both endpoints are indexed, so caret movement never scans every pair.
    brackets: Vec<(usize, usize)>,
}

impl ParseSnapshot {
    pub fn install(
        &mut self,
        highlights: &[u32],
        folds: &[u32],
        brackets: &[u32],
        diagnostics: &str,
        text: &Rope,
    ) -> Result<(), String> {
        let diagnostics: Vec<DiagnosticData> =
            serde_json::from_str(diagnostics).map_err(|error| error.to_string())?;
        self.highlights = highlights
            .chunks_exact(3)
            .map(|run| (run[0] as usize..run[1] as usize, run[2]))
            .collect();
        self.folds = folds
            .chunks_exact(4)
            .map(|fold| {
                FoldRange::with_columns(
                    fold[0] as usize,
                    fold[1] as usize,
                    fold[2] as usize,
                    fold[3] as usize,
                )
            })
            .collect();
        self.brackets = brackets
            .chunks_exact(2)
            .map(|pair| (pair[0] as usize, pair[1] as usize))
            .collect();
        self.diagnostics = diagnostics
            .into_iter()
            .map(|diagnostic| {
                Diagnostic::new(
                    text.offset_to_position(diagnostic.start.min(text.len()))
                        ..text.offset_to_position(diagnostic.end.min(text.len())),
                    diagnostic.message,
                )
                .with_severity(DiagnosticSeverity::Error)
                .with_source("JSON5")
                .with_code(diagnostic.code)
            })
            .collect();
        self.version += 1;
        Ok(())
    }

    pub fn prepare_open(&mut self) {
        self.installing = true;
        self.revision = 0;
        self.highlights.clear();
        self.folds.clear();
        self.brackets.clear();
        self.diagnostics.clear();
    }

    pub fn matching_brackets(&self, cursor: usize) -> Option<[Range<usize>; 2]> {
        [cursor.saturating_sub(1), cursor]
            .into_iter()
            .find_map(|offset| {
                self.brackets
                    .binary_search_by_key(&offset, |pair| pair.0)
                    .ok()
                    .map(|index| {
                        let (at, matching) = self.brackets[index];
                        [at..at + 1, matching..matching + 1]
                    })
            })
    }

    fn adjust(&mut self, edit: InputEdit) {
        let delta = edit.new_end_byte as isize - edit.old_end_byte as isize;
        self.highlights.retain_mut(|(range, _)| {
            if range.end <= edit.start_byte {
                return true;
            }
            if range.start >= edit.old_end_byte
                && let Some(start) = range.start.checked_add_signed(delta)
                && let Some(end) = range.end.checked_add_signed(delta)
            {
                *range = start..end;
                return true;
            }
            false
        });
        self.brackets.clear();
        self.diagnostics.clear();
        self.version += 1;
    }
}

pub(super) struct TreeSitterHighlighter {
    language: SharedString,
    snapshot: Rc<RefCell<ParseSnapshot>>,
}

impl TreeSitterHighlighter {
    pub(super) fn new(language: &str, snapshot: Rc<RefCell<ParseSnapshot>>) -> Option<Self> {
        (language == "json5").then(|| Self {
            language: language.into(),
            snapshot,
        })
    }
}

impl InputHighlighter for TreeSitterHighlighter {
    fn language(&self) -> SharedString {
        self.language.clone()
    }

    fn diagnostics(&self, _: &Rope) -> Option<Vec<Diagnostic>> {
        Some(self.snapshot.borrow().diagnostics.clone())
    }

    fn update(
        &mut self,
        edit: Option<InputEdit>,
        text: &Rope,
        _folding: bool,
        _window: &mut Window,
        _cx: &mut Context<EditorState>,
    ) {
        let Some(edit) = edit else {
            return;
        };
        let revision = {
            let mut snapshot = self.snapshot.borrow_mut();
            if snapshot.installing {
                return;
            }
            snapshot.adjust(edit);
            snapshot.revision += 1;
            snapshot.revision
        };
        let replacement = text.slice(edit.start_byte..edit.new_end_byte).to_string();
        crate::queue_syntax_edit(
            revision,
            edit.start_byte,
            edit.old_end_byte,
            &replacement,
            edit.start_position.row,
            edit.start_position.column,
            edit.old_end_position.row,
            edit.old_end_position.column,
            edit.new_end_position.row,
            edit.new_end_position.column,
        );
    }

    fn update_batch(
        &mut self,
        edits: &[(InputEdit, Rope)],
        folding: bool,
        window: &mut Window,
        cx: &mut Context<EditorState>,
    ) {
        for (edit, text) in edits {
            self.update(Some(*edit), text, folding, window, cx);
        }
    }

    fn styles(
        &self,
        range: &Range<usize>,
        resolver: &dyn HighlightStyleResolver,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        let snapshot = self.snapshot.borrow();
        let first = snapshot
            .highlights
            .partition_point(|(span, _)| span.end <= range.start);
        let mut position = range.start;
        let mut runs = Vec::new();
        for (span, semantic) in snapshot.highlights[first..]
            .iter()
            .take_while(|(span, _)| span.start < range.end)
        {
            let start = span.start.max(range.start);
            let end = span.end.min(range.end);
            if position < start {
                runs.push((position..start, HighlightStyle::default()));
            }
            let style = SEMANTIC_NAMES
                .get(*semantic as usize)
                .and_then(|name| resolver.style(name))
                .unwrap_or_default();
            runs.push((start..end, style));
            position = end;
        }
        if position < range.end {
            runs.push((position..range.end, HighlightStyle::default()));
        }
        runs
    }

    fn fold_ranges(&self, _: &Rope) -> Vec<FoldRange> {
        self.snapshot.borrow().folds.clone()
    }

    fn fold_ranges_for_edit(&self, _: Range<usize>, _: &Rope) -> Vec<FoldRange> {
        // Unaffected candidates are shifted by the editor. The worker installs
        // the complete, current candidates when this revision finishes.
        Vec::new()
    }
}
