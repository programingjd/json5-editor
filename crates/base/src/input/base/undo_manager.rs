use super::auto_close::AutoClosedPairs;
use crate::input::change::Change;

use super::cursor::CursorSelection;

const MAX_UNDO_TRANSACTIONS: usize = 1000;
const MAX_CHANGES_PER_TRANSACTION: usize = 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EditIntent {
    Typing,
    Backspace,
    DeleteForward,
    Atomic,
}

#[derive(Debug)]
struct UndoTransaction {
    intent: EditIntent,
    changes: Vec<Change>,

    last_batch_len: usize,

    recorded_changes: usize,

    selections_before: Option<Vec<CursorSelection>>,

    selections_after: Option<Vec<CursorSelection>>,
    auto_closed_pairs_before: Option<AutoClosedPairs>,
    auto_closed_pairs_after: Option<AutoClosedPairs>,
}

#[derive(Debug)]
struct PendingTransaction {
    intent: EditIntent,
    changes: Vec<Change>,
    selections_before: Option<Vec<CursorSelection>>,
    selections_after: Option<Vec<CursorSelection>>,
    auto_closed_pairs_before: Option<AutoClosedPairs>,
    auto_closed_pairs_after: Option<AutoClosedPairs>,
}

pub(crate) struct Replay {
    pub(super) changes: Vec<Change>,
    pub(super) selections: Option<Vec<CursorSelection>>,
    pub(super) auto_closed_pairs: Option<AutoClosedPairs>,
}

#[derive(Debug)]
pub(crate) struct UndoManager {
    undo_transactions: Vec<UndoTransaction>,
    redo_transactions: Vec<UndoTransaction>,
    ignoring: bool,
    transaction_depth: usize,
    pending: Option<PendingTransaction>,
    pending_intent: Option<EditIntent>,
    coalescing_boundary: bool,
}

impl UndoManager {
    pub(super) fn new() -> Self {
        Self {
            undo_transactions: Vec::new(),
            redo_transactions: Vec::new(),
            ignoring: false,
            transaction_depth: 0,
            pending: None,
            pending_intent: None,
            coalescing_boundary: false,
        }
    }

    pub(super) fn take_pending_intent(&mut self) -> Option<EditIntent> {
        self.pending_intent.take()
    }

    pub(super) fn set_pending_intent(&mut self, intent: EditIntent) {
        self.pending_intent = Some(intent);
    }

    pub(super) fn record_transaction(&mut self, change: Change, intent: EditIntent) -> bool {
        if self.ignoring {
            return false;
        }
        if change.token_delta.is_none()
            && change.old_range == change.new_range
            && change.old_text == change.new_text
        {
            self.break_transaction_coalescing();
            return false;
        }

        match self.pending.as_mut() {
            Some(pending) => pending.changes.push(change),
            None => self.push_batch(vec![change], intent),
        }
        true
    }

    pub(super) fn begin_transaction(&mut self) {
        self.begin_transaction_with(EditIntent::Atomic);
    }

    pub(super) fn begin_transaction_with(&mut self, intent: EditIntent) {
        self.transaction_depth += 1;
        if self.transaction_depth == 1 {
            self.pending = Some(PendingTransaction {
                intent,
                changes: Vec::new(),
                selections_before: None,
                selections_after: None,
                auto_closed_pairs_before: None,
                auto_closed_pairs_after: None,
            });
        }
    }

    pub(super) fn commit_transaction(&mut self) {
        if self.transaction_depth == 0 {
            return;
        }

        self.transaction_depth -= 1;
        if self.transaction_depth > 0 {
            return;
        }

        let Some(pending) = self.pending.take() else {
            return;
        };
        // A composition that ends where it started (typed then canceled) leaves
        // the document untouched and must not become an undo entry.
        if pending.changes.is_empty() || is_noop_batch(&pending.changes) {
            return;
        }
        self.push_batch(pending.changes, pending.intent);
        if let Some(before) = pending.selections_before {
            self.record_selections_before(before);
        }
        if let Some(after) = pending.selections_after {
            self.record_selections_after(after);
        }
        // These snapshots were recorded with the edits. Commit them even when
        // replay has just enabled ignoring to flush an open IME transaction.
        if let Some(transaction) = self.undo_transactions.last_mut() {
            if let Some(before) = pending.auto_closed_pairs_before {
                transaction.auto_closed_pairs_before.get_or_insert(before);
            }
            if let Some(after) = pending.auto_closed_pairs_after {
                transaction.auto_closed_pairs_after = Some(after);
            }
        }
    }

    fn push_batch(&mut self, changes: Vec<Change>, intent: EditIntent) {
        if changes.is_empty() {
            return;
        }

        self.redo_transactions.clear();
        let can_coalesce = !self.coalescing_boundary
            && intent != EditIntent::Atomic
            && self.undo_transactions.last().is_some_and(|previous| {
                previous.intent == intent
                    && previous.last_batch_len == changes.len()
                    && previous.recorded_changes + changes.len() <= MAX_CHANGES_PER_TRANSACTION
                    && is_adjacent_batch(intent, previous.trailing_batch(), &changes)
            });

        if can_coalesce {
            let previous = self
                .undo_transactions
                .last_mut()
                .expect("coalescing requires a previous transaction");
            previous.last_batch_len = changes.len();
            previous.recorded_changes += changes.len();
            // Adjacent single-cursor keystrokes form one contiguous insertion.
            // Keep it as one change so undo and redo replay it as a single
            // edit rather than once per keystroke.
            if intent == EditIntent::Typing
                && let [change] = changes.as_slice()
                && let Some(last) = previous.changes.last_mut()
            {
                last.new_text
                    .truncate(change.old_range.start - last.new_range.start);
                last.new_text.push_str(&change.new_text);
                last.new_range.end = change.new_range.end;
                return;
            }
            previous.changes.extend(changes);
            return;
        }

        if self.undo_transactions.len() >= MAX_UNDO_TRANSACTIONS {
            self.undo_transactions.remove(0);
        }
        self.undo_transactions.push(UndoTransaction {
            intent,
            last_batch_len: changes.len(),
            recorded_changes: changes.len(),
            changes,
            selections_before: None,
            selections_after: None,
            auto_closed_pairs_before: None,
            auto_closed_pairs_after: None,
        });
        self.coalescing_boundary = intent == EditIntent::Atomic;
    }

    pub(super) fn record_selections(
        &mut self,
        before: Vec<CursorSelection>,
        after: Vec<CursorSelection>,
    ) {
        if self.ignoring {
            return;
        }
        self.record_selections_before(before);
        self.record_selections_after(after);
    }

    pub(super) fn record_auto_closed_pairs(
        &mut self,
        before: AutoClosedPairs,
        after: AutoClosedPairs,
    ) {
        if self.ignoring {
            return;
        }
        if let Some(pending) = self.pending.as_mut() {
            pending.auto_closed_pairs_before.get_or_insert(before);
        } else if let Some(transaction) = self.undo_transactions.last_mut() {
            transaction.auto_closed_pairs_before.get_or_insert(before);
        }
        self.record_auto_closed_pairs_after(after);
    }

    pub(super) fn record_auto_closed_pairs_after(&mut self, after: AutoClosedPairs) {
        if self.ignoring {
            return;
        }
        if let Some(pending) = self.pending.as_mut() {
            pending.auto_closed_pairs_after = Some(after);
        } else if let Some(transaction) = self.undo_transactions.last_mut() {
            transaction.auto_closed_pairs_after = Some(after);
        }
    }

    fn record_selections_before(&mut self, before: Vec<CursorSelection>) {
        if let Some(pending) = self.pending.as_mut() {
            pending.selections_before.get_or_insert(before);
        } else if let Some(transaction) = self.undo_transactions.last_mut() {
            transaction.selections_before.get_or_insert(before);
        }
    }

    fn record_selections_after(&mut self, after: Vec<CursorSelection>) {
        if let Some(pending) = self.pending.as_mut() {
            pending.selections_after = Some(after);
        } else if let Some(transaction) = self.undo_transactions.last_mut() {
            transaction.selections_after = Some(after);
        }
    }

    pub(super) fn has_open_transaction(&self) -> bool {
        self.transaction_depth > 0
    }

    pub(super) fn break_transaction_coalescing(&mut self) {
        // While a batch is open the boundary applies to that batch, so never
        // close a bracket the caller still owns.
        if self.transaction_depth == 0 {
            self.commit_all_transactions();
        }
        self.coalescing_boundary = true;
    }

    fn commit_all_transactions(&mut self) {
        if self.transaction_depth == 0 {
            return;
        }
        self.transaction_depth = 1;
        self.commit_transaction();
    }

    pub(super) fn is_ignoring(&self) -> bool {
        self.ignoring
    }

    pub(super) fn set_ignoring(&mut self, ignoring: bool) {
        self.ignoring = ignoring;
        if ignoring {
            self.commit_all_transactions();
        }
    }

    pub(super) fn clear(&mut self) {
        self.undo_transactions.clear();
        self.redo_transactions.clear();
        self.transaction_depth = 0;
        self.pending = None;
        self.pending_intent = None;
        self.coalescing_boundary = false;
    }

    pub(super) fn undo(&mut self) -> Option<Replay> {
        self.commit_all_transactions();
        let transaction = self.undo_transactions.pop()?;
        let replay = Replay {
            changes: transaction.changes.iter().rev().cloned().collect(),
            selections: transaction.selections_before.clone(),
            auto_closed_pairs: transaction.auto_closed_pairs_before.clone(),
        };
        self.redo_transactions.push(transaction);
        self.coalescing_boundary = true;
        Some(replay)
    }

    pub(super) fn redo(&mut self) -> Option<Replay> {
        self.commit_all_transactions();
        let transaction = self.redo_transactions.pop()?;
        let replay = Replay {
            changes: transaction.changes.clone(),
            selections: transaction.selections_after.clone(),
            auto_closed_pairs: transaction.auto_closed_pairs_after.clone(),
        };
        self.undo_transactions.push(transaction);
        self.coalescing_boundary = true;
        Some(replay)
    }

    #[cfg(test)]
    pub(super) fn has_undos(&self) -> bool {
        !self.undo_transactions.is_empty()
    }

    #[cfg(test)]
    pub(super) fn undo_count(&self) -> usize {
        self.undo_transactions.len()
    }
}

impl UndoTransaction {
    fn trailing_batch(&self) -> &[Change] {
        &self.changes[self.changes.len() - self.last_batch_len..]
    }
}

fn is_noop_batch(changes: &[Change]) -> bool {
    if changes.iter().any(|c| c.token_delta.is_some()) {
        return false;
    }
    let Some(first) = changes.first() else {
        return true;
    };

    let mut last = first;
    for change in &changes[1..] {
        if last.new_range.start != change.old_range.start
            || last.new_range.end != change.old_range.end
        {
            return false;
        }
        last = change;
    }

    first.old_range.start == last.new_range.start
        && first.old_range.end == last.new_range.end
        && first.old_text == last.new_text
}

fn is_adjacent_batch(intent: EditIntent, previous: &[Change], current: &[Change]) -> bool {
    if previous.len() != current.len() {
        return false;
    }

    let mut shifts = vec![0isize; previous.len()];
    let mut shift = 0isize;
    for (index, change) in previous.iter().enumerate().rev() {
        shifts[index] = shift;
        shift += change.new_text.len() as isize - change.old_text.len() as isize;
    }

    previous
        .iter()
        .zip(current)
        .zip(shifts)
        .all(|((previous, current), shift)| is_adjacent(intent, &previous.shifted(shift), current))
}

fn is_adjacent(intent: EditIntent, previous: &Change, current: &Change) -> bool {
    if previous.token_delta.is_some() || current.token_delta.is_some() {
        return false;
    }
    match intent {
        EditIntent::Typing => {
            previous.old_range.is_empty()
                && !previous.new_text.contains(['\n', '\r'])
                && !current.new_text.contains(['\n', '\r'])
                && previous.new_range.start <= current.old_range.start
                && previous.new_range.end == current.old_range.end
        }
        EditIntent::Backspace => {
            previous.new_text.is_empty()
                && current.new_text.is_empty()
                && current.old_range.end == previous.old_range.start
        }
        EditIntent::DeleteForward => {
            previous.new_text.is_empty()
                && current.new_text.is_empty()
                && current.old_range.start == previous.old_range.start
        }
        EditIntent::Atomic => false,
    }
}
