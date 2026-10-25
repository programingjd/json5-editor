#[derive(Debug)]
pub struct History<T> {
    entries: Vec<T>,
    forward_entries: Vec<T>,
    max_entries: usize,
}

impl<T> History<T> {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            forward_entries: Vec::new(),
            max_entries: 1000,
        }
    }

    pub fn max_entries(mut self, max_entries: usize) -> Self {
        self.max_entries = max_entries;
        self.enforce_max_entries();
        self
    }

    pub fn push(&mut self, entry: T) {
        self.forward_entries.clear();
        if self.max_entries == 0 {
            return;
        }
        self.entries.push(entry);
        self.enforce_max_entries();
    }

    pub fn current(&self) -> Option<&T> {
        self.entries.last()
    }

    pub fn replace_current(&mut self, entry: T) {
        match self.entries.last_mut() {
            Some(current) => *current = entry,
            None => self.push(entry),
        }
    }

    pub fn remove_current(&mut self) -> Option<T> {
        self.entries.pop()
    }

    pub fn can_back(&self) -> bool {
        self.entries.len() > 1
    }

    pub fn can_forward(&self) -> bool {
        !self.forward_entries.is_empty()
    }

    pub fn back(&mut self) -> Option<T>
    where
        T: Clone,
    {
        if self.entries.len() <= 1 {
            return None;
        }
        self.forward_entries.push(self.entries.pop().unwrap());
        self.current().cloned()
    }

    pub fn forward(&mut self) -> Option<T>
    where
        T: Clone,
    {
        if self.max_entries == 0 {
            return None;
        }
        let entry = self.forward_entries.pop()?;
        self.entries.push(entry);
        self.enforce_max_entries();
        self.current().cloned()
    }

    pub fn entries(&self) -> impl DoubleEndedIterator<Item = &T> + ExactSizeIterator {
        self.entries.iter()
    }

    pub fn forward_entries(&self) -> impl DoubleEndedIterator<Item = &T> + ExactSizeIterator {
        self.forward_entries.iter().rev()
    }

    pub fn retain(&mut self, mut keep: impl FnMut(&T) -> bool) {
        self.entries.retain(&mut keep);
        self.forward_entries.retain(&mut keep);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.forward_entries.clear();
    }

    fn enforce_max_entries(&mut self) {
        let excess = self.entries.len().saturating_sub(self.max_entries);
        if excess > 0 {
            self.entries.drain(..excess);
        }
    }
}

impl<T> Default for History<T> {
    fn default() -> Self {
        Self::new()
    }
}
