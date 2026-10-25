use instant::{Duration, Instant};

#[derive(Debug)]
pub struct UndoHistory<T> {
    undos: Vec<Vec<T>>,
    redos: Vec<Vec<T>>,
    last_changed_at: Option<Instant>,
    max_undos: usize,
    group_interval: Option<Duration>,
    grouping: bool,
    ignoring: bool,
}

impl<T> UndoHistory<T> {
    pub fn new() -> Self {
        Self {
            undos: Vec::new(),
            redos: Vec::new(),
            last_changed_at: None,
            max_undos: 1000,
            group_interval: None,
            grouping: false,
            ignoring: false,
        }
    }

    pub fn max_undos(mut self, max_undos: usize) -> Self {
        self.max_undos = max_undos;
        self.enforce_max_undos();
        self
    }

    pub fn group_interval(mut self, group_interval: Duration) -> Self {
        self.group_interval = Some(group_interval);
        self
    }

    pub fn start_grouping(&mut self) {
        self.grouping = true;
    }

    pub fn end_grouping(&mut self) {
        self.grouping = false;
    }

    pub fn is_ignoring(&self) -> bool {
        self.ignoring
    }

    pub fn set_ignoring(&mut self, ignoring: bool) {
        self.ignoring = ignoring;
    }

    pub fn push(&mut self, item: T) {
        if self.ignoring || self.max_undos == 0 {
            return;
        }

        let group_with_previous = self.grouping
            || self.last_changed_at.is_some_and(|last_changed_at| {
                self.group_interval
                    .is_some_and(|interval| last_changed_at.elapsed() <= interval)
            });

        if group_with_previous && !self.undos.is_empty() {
            self.undos.last_mut().unwrap().push(item);
        } else {
            self.undos.push(vec![item]);
            self.enforce_max_undos();
        }

        self.last_changed_at = Some(Instant::now());
        self.redos.clear();
    }

    pub fn undo(&mut self) -> Option<Vec<T>>
    where
        T: Clone,
    {
        let transaction = self.undos.pop()?;
        let changes = transaction.iter().rev().cloned().collect();
        self.redos.push(transaction);
        self.last_changed_at = None;
        Some(changes)
    }

    pub fn redo(&mut self) -> Option<Vec<T>>
    where
        T: Clone,
    {
        if self.max_undos == 0 {
            return None;
        }
        let transaction = self.redos.pop()?;
        let changes = transaction.clone();
        self.undos.push(transaction);
        self.enforce_max_undos();
        self.last_changed_at = None;
        Some(changes)
    }

    pub fn can_undo(&self) -> bool {
        !self.undos.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redos.is_empty()
    }

    pub fn clear(&mut self) {
        self.undos.clear();
        self.redos.clear();
        self.last_changed_at = None;
    }

    fn enforce_max_undos(&mut self) {
        let excess = self.undos.len().saturating_sub(self.max_undos);
        if excess > 0 {
            self.undos.drain(..excess);
        }
    }
}

impl<T> Default for UndoHistory<T> {
    fn default() -> Self {
        Self::new()
    }
}
