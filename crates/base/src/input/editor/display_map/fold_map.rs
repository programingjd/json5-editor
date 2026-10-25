use std::ops::Range;

use super::folding::FoldRange;
use super::wrap_map::WrapMap;

#[derive(Debug, Clone, PartialEq, Eq)]
struct HiddenRows {
    wrap_rows: Range<usize>,

    hidden_before: usize,
}

impl HiddenRows {
    fn display_start(&self) -> usize {
        self.wrap_rows.start - self.hidden_before
    }
}

pub(super) struct FoldMap {
    hidden: Vec<HiddenRows>,

    total_hidden: usize,

    projected_wrap_row_count: usize,

    candidates: Vec<FoldRange>,

    folded: Vec<FoldRange>,

    folded_prefix_ends: Vec<usize>,

    needs_rebuild: bool,

    cached_wrap_row_count: usize,
}

impl FoldMap {
    pub(super) fn new() -> Self {
        Self {
            hidden: Vec::new(),
            total_hidden: 0,
            projected_wrap_row_count: 0,
            candidates: Vec::new(),
            folded: Vec::new(),
            folded_prefix_ends: Vec::new(),
            needs_rebuild: true,
            cached_wrap_row_count: 0,
        }
    }

    pub(super) fn mark_dirty_with_wrap_count(&mut self, wrap_row_count: usize) {
        self.needs_rebuild = true;
        self.cached_wrap_row_count = wrap_row_count;
    }

    pub(super) fn display_row_count(&self) -> usize {
        if self.folded.is_empty() {
            return self.cached_wrap_row_count;
        }
        self.projected_wrap_row_count - self.total_hidden
    }

    fn hidden_before(&self, index: usize) -> usize {
        self.hidden
            .get(index)
            .map_or(self.total_hidden, |run| run.hidden_before)
    }

    pub(super) fn wrap_row_to_display_row(&self, wrap_row: usize) -> Option<usize> {
        if self.folded.is_empty() {
            return if wrap_row < self.cached_wrap_row_count {
                Some(wrap_row)
            } else {
                None
            };
        }
        if wrap_row >= self.projected_wrap_row_count {
            return None;
        }
        let index = self
            .hidden
            .partition_point(|run| run.wrap_rows.end <= wrap_row);
        if self
            .hidden
            .get(index)
            .is_some_and(|run| run.wrap_rows.start <= wrap_row)
        {
            return None;
        }
        Some(wrap_row - self.hidden_before(index))
    }

    pub(super) fn display_row_to_wrap_row(&self, display_row: usize) -> Option<usize> {
        if self.folded.is_empty() {
            return if display_row < self.cached_wrap_row_count {
                Some(display_row)
            } else {
                None
            };
        }
        if display_row >= self.display_row_count() {
            return None;
        }
        let index = self
            .hidden
            .partition_point(|run| run.display_start() <= display_row);
        Some(display_row + self.hidden_before(index))
    }

    pub(super) fn nearest_visible_display_row(&self, wrap_row: usize) -> usize {
        if self.folded.is_empty() {
            return wrap_row.min(self.cached_wrap_row_count.saturating_sub(1));
        }

        if let Some(dr) = self.wrap_row_to_display_row(wrap_row) {
            return dr;
        }

        // A hidden row maps to the last visible row before it.
        let visible_before = if wrap_row >= self.projected_wrap_row_count {
            self.display_row_count()
        } else {
            let index = self
                .hidden
                .partition_point(|run| run.wrap_rows.end <= wrap_row);
            self.hidden
                .get(index)
                .map_or(self.display_row_count(), HiddenRows::display_start)
        };
        visible_before.saturating_sub(1)
    }

    pub(super) fn set_candidates(&mut self, mut candidates: Vec<FoldRange>) {
        // Sort and deduplicate by start_line
        candidates.sort_by_key(|r| r.start_line);
        candidates.dedup_by_key(|r| r.start_line);
        self.candidates = candidates;

        // Remove any folded ranges that are no longer in candidates
        self.folded.retain_mut(|fold| {
            let Ok(index) = self
                .candidates
                .binary_search_by_key(&fold.start_line, |candidate| candidate.start_line)
            else {
                return false;
            };
            *fold = self.candidates[index];
            true
        });
        self.needs_rebuild = true;
    }

    pub(super) fn merge_candidates_for_edit(
        &mut self,
        edit_start_line: usize,
        edit_end_line: usize,
        new_candidates: Vec<FoldRange>,
    ) {
        // Replace candidates whose opener is in the edit range, and also
        // replace enclosing candidates returned when an edit touches their
        // closing line (for example, whitespace before a trailing comma).
        self.candidates.retain(|candidate| {
            (candidate.start_line < edit_start_line || candidate.start_line > edit_end_line)
                && !new_candidates
                    .iter()
                    .any(|updated| updated.start_line == candidate.start_line)
        });

        // Keep an affected fold only while the parser still reports its
        // opening line as a fold candidate. Refresh its end line so harmless
        // edits on the closing line don't unfold it or leave stale bounds.
        self.folded.retain(|fold| {
            let updated = new_candidates
                .iter()
                .find(|candidate| candidate.start_line == fold.start_line);
            let was_affected = fold.start_line <= edit_end_line && fold.end_line >= edit_start_line;
            updated.is_some() || !was_affected
        });
        for fold in &mut self.folded {
            if let Some(updated) = new_candidates
                .iter()
                .find(|candidate| candidate.start_line == fold.start_line)
            {
                *fold = *updated;
            }
        }

        // Add new candidates
        self.candidates.extend(new_candidates);
        self.candidates.sort_by_key(|r| r.start_line);
        self.candidates.dedup_by_key(|r| r.start_line);
        self.needs_rebuild = true;
    }

    pub(super) fn set_folded(&mut self, start_line: usize, folded: bool) {
        let index = self
            .folded
            .binary_search_by_key(&start_line, |fold| fold.start_line);
        if folded {
            if let Err(index) = index
                && let Ok(candidate) = self
                    .candidates
                    .binary_search_by_key(&start_line, |candidate| candidate.start_line)
            {
                self.folded.insert(index, self.candidates[candidate]);
                self.needs_rebuild = true;
            }
        } else if let Ok(index) = index {
            self.folded.remove(index);
            self.needs_rebuild = true;
        }
    }

    pub(super) fn toggle_fold(&mut self, start_line: usize) {
        let is_folded = self.is_folded_at(start_line);
        self.set_folded(start_line, !is_folded);
    }

    pub(super) fn is_folded_at(&self, start_line: usize) -> bool {
        self.folded_range_at(start_line).is_some()
    }

    pub(super) fn is_fold_candidate(&self, start_line: usize) -> bool {
        self.candidates
            .binary_search_by_key(&start_line, |candidate| candidate.start_line)
            .is_ok()
    }

    pub(super) fn folded_range_at(&self, start_line: usize) -> Option<FoldRange> {
        self.folded
            .binary_search_by_key(&start_line, |fold| fold.start_line)
            .ok()
            .map(|index| self.folded[index])
    }

    pub(super) fn fold_covering_line(&self, line: usize) -> Option<FoldRange> {
        let end = self.folded.partition_point(|fold| fold.start_line <= line);
        let index = self.folded_prefix_ends[..end].partition_point(|&last| last < line);
        (index < end).then(|| self.folded[index])
    }

    #[inline]
    pub(super) fn fold_candidates(&self) -> &[FoldRange] {
        &self.candidates
    }

    #[inline]
    pub(super) fn folded_ranges(&self) -> &[FoldRange] {
        &self.folded
    }

    #[inline]
    pub(super) fn clear_folds(&mut self) {
        self.folded.clear();
        self.folded_prefix_ends.clear();
        self.needs_rebuild = true;
    }

    pub(super) fn fold_all(&mut self) {
        self.folded.clone_from(&self.candidates);
        self.needs_rebuild = true;
    }

    pub(super) fn set_scope_folded(
        &mut self,
        scope: FoldRange,
        include_scope: bool,
        folded: bool,
    ) -> bool {
        let first_line = scope.start_line + usize::from(!include_scope);
        let is_target = |fold: &FoldRange| {
            fold.start_line >= first_line
                && (fold.end_line < scope.end_line
                    || (fold.end_line == scope.end_line
                        && fold.end_column.unwrap_or(0) <= scope.end_column.unwrap_or(usize::MAX)))
        };
        let changed = if folded {
            let first = self
                .candidates
                .partition_point(|fold| fold.start_line < first_line);
            let targets = self.candidates[first..]
                .iter()
                .take_while(|fold| fold.start_line <= scope.end_line)
                .filter(|fold| is_target(fold));
            // Both inputs are sorted. Merge once instead of inserting each
            // child separately and rebuilding the projection after every one.
            let mut existing = self.folded.iter().copied().peekable();
            let mut next = Vec::with_capacity(self.folded.len());
            for &child in targets {
                while existing
                    .peek()
                    .is_some_and(|fold| fold.start_line < child.start_line)
                {
                    next.push(existing.next().unwrap());
                }
                if existing
                    .peek()
                    .is_some_and(|fold| fold.start_line == child.start_line)
                {
                    existing.next();
                }
                next.push(child);
            }
            next.extend(existing);
            if next == self.folded {
                false
            } else {
                self.folded = next;
                true
            }
        } else {
            let before = self.folded.len();
            self.folded.retain(|fold| !is_target(fold));
            self.folded.len() != before
        };
        self.needs_rebuild |= changed;
        changed
    }

    pub(super) fn adjust_folds_for_edit(
        &mut self,
        edit_start_line: usize,
        edit_end_line: usize,
        line_delta: isize,
    ) {
        // Preserve folds across edits that touch their source, then reconcile
        // them with the refreshed candidates in `merge_candidates_for_edit`.
        // This lets edits to a closing line update its bounds without
        // expanding the scope while the parser catches up.
        if line_delta != 0 {
            for fold in &mut self.folded {
                if fold.start_line > edit_end_line {
                    fold.start_line = (fold.start_line as isize + line_delta).max(0) as usize;
                    fold.end_line = (fold.end_line as isize + line_delta).max(0) as usize;
                } else if fold.end_line >= edit_start_line {
                    fold.end_line = (fold.end_line as isize + line_delta).max(0) as usize;
                }
            }
        }

        // Adjust candidates the same way
        if !self.candidates.is_empty() {
            self.candidates
                .retain(|c| !(c.start_line <= edit_end_line && c.end_line >= edit_start_line));

            if line_delta != 0 {
                for c in &mut self.candidates {
                    if c.start_line > edit_end_line {
                        c.start_line = (c.start_line as isize + line_delta).max(0) as usize;
                        c.end_line = (c.end_line as isize + line_delta).max(0) as usize;
                    }
                }
            }
        }

        self.needs_rebuild = true;
    }

    pub(super) fn rebuild(&mut self, wrap_map: &WrapMap) {
        let wrap_row_count = wrap_map.wrap_row_count();

        // Performance optimization: skip rebuild if nothing changed
        if !self.needs_rebuild && wrap_row_count == self.cached_wrap_row_count {
            return;
        }

        self.cached_wrap_row_count = wrap_row_count;

        self.folded_prefix_ends.clear();
        let mut last_end = 0;
        for fold in &self.folded {
            last_end = last_end.max(fold.end_line);
            self.folded_prefix_ends.push(last_end);
        }

        if self.folded.is_empty() {
            // Fast path: no folds, all wrap rows are visible
            self.set_hidden_rows(wrap_row_count, Vec::new());
            self.needs_rebuild = false;
            return;
        }

        // Build set of hidden wrap_row ranges from folded buffer lines
        let mut hidden_ranges = Vec::new();
        let mut hidden_through_line = None;
        for fold in &self.folded {
            // Keep the opening line visible and hide the body and closing line.
            let hide_start_line = fold.start_line + 1;
            let hide_end_line = fold.end_line;

            if hide_start_line > hide_end_line {
                continue; // A single-line scope has no closing row to fold.
            }
            // Collapse All retains nested fold state so expanding a parent
            // reveals its folded children. Its projection only needs the
            // outer hidden runs, without resolving every child's wrap rows.
            if hidden_through_line.is_some_and(|end| hide_end_line <= end) {
                continue;
            }
            hidden_through_line = Some(hide_end_line);

            // Get wrap_row ranges for the hidden buffer lines
            let start_wrap_row = wrap_map.buffer_line_to_first_wrap_row(hide_start_line);
            let end_wrap_row = if hide_end_line + 1 < wrap_map.buffer_line_count() {
                wrap_map.buffer_line_to_first_wrap_row(hide_end_line + 1)
            } else {
                wrap_row_count
            };

            if start_wrap_row < end_wrap_row {
                hidden_ranges.push(start_wrap_row..end_wrap_row);
            }
        }

        self.set_hidden_rows(wrap_row_count, hidden_ranges);
        self.needs_rebuild = false;
    }

    fn set_hidden_rows(&mut self, wrap_row_count: usize, mut hidden_ranges: Vec<Range<usize>>) {
        hidden_ranges.sort_by_key(|range| range.start);

        self.hidden.clear();
        self.total_hidden = 0;
        self.projected_wrap_row_count = wrap_row_count;
        for range in hidden_ranges {
            let range = range.start..range.end.min(wrap_row_count);
            if range.is_empty() {
                continue;
            }
            if let Some(last) = self.hidden.last_mut()
                && range.start <= last.wrap_rows.end
            {
                // Overlapping or adjacent, merge
                if range.end > last.wrap_rows.end {
                    self.total_hidden += range.end - last.wrap_rows.end;
                    last.wrap_rows.end = range.end;
                }
                continue;
            }
            self.total_hidden += range.len();
            self.hidden.push(HiddenRows {
                hidden_before: self.total_hidden - range.len(),
                wrap_rows: range,
            });
        }
    }
}
