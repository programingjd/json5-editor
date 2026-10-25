use crate::input::EditorMode;
use std::{collections::BTreeMap, ops::Range};

use gpui::{App, Context, HighlightStyle, Hsla, WeakEntity};
use ropey::Rope;
use sum_tree::Bias;

use super::{InputBaseState, RopeExt as _};

#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RangeDecorationStyle {
    Fill,

    #[default]
    Frame,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RangeDecoration {
    range: Range<usize>,
    style: RangeDecorationStyle,
    color: Option<Hsla>,
}

impl RangeDecoration {
    pub fn new(range: Range<usize>) -> Self {
        Self {
            range,
            style: RangeDecorationStyle::default(),
            color: None,
        }
    }

    pub fn range(&self) -> &Range<usize> {
        &self.range
    }

    pub fn style(&self) -> RangeDecorationStyle {
        self.style
    }

    pub fn color(&self) -> Option<Hsla> {
        self.color
    }

    pub fn with_style(mut self, style: RangeDecorationStyle) -> Self {
        self.style = style;
        self
    }

    pub fn with_color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextDecoration {
    pub range: Range<usize>,
    pub style: HighlightStyle,
}

impl TextDecoration {
    pub fn new(range: Range<usize>, style: HighlightStyle) -> Self {
        Self { range, style }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct DecorationCollectionId(usize);

#[derive(Clone, Debug)]
pub struct TextDecorationCollection {
    state: WeakEntity<InputBaseState<EditorMode>>,
    id: DecorationCollectionId,
}

impl TextDecorationCollection {
    pub fn set(&self, decorations: Vec<TextDecoration>, cx: &mut App) {
        let _ = self.state.update(cx, |state, cx| {
            let decorations = normalize(&state.text, decorations);
            if state.extras.decorations.set(self.id, decorations) {
                cx.notify();
            }
        });
    }

    pub fn append(&self, decorations: Vec<TextDecoration>, cx: &mut App) {
        let _ = self.state.update(cx, |state, cx| {
            let decorations = normalize(&state.text, decorations);
            if state.extras.decorations.append(self.id, decorations) {
                cx.notify();
            }
        });
    }

    pub fn clear(&self, cx: &mut App) {
        self.set(Vec::new(), cx);
    }

    pub fn get_ranges(&self, cx: &App) -> Vec<Range<usize>> {
        self.state
            .read_with(cx, |state, _| {
                state
                    .extras
                    .decorations
                    .get(self.id)
                    .unwrap_or_default()
                    .iter()
                    .map(|decoration| decoration.range.clone())
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[derive(Clone, Debug)]
pub struct RangeDecorationCollection {
    state: WeakEntity<InputBaseState<EditorMode>>,
    id: DecorationCollectionId,
}

impl RangeDecorationCollection {
    pub fn set(&self, decorations: Vec<RangeDecoration>, cx: &mut App) {
        let _ = self.state.update(cx, |state, cx| {
            let decorations = normalize(&state.text, decorations);
            if state.extras.range_decorations.set(self.id, decorations) {
                cx.notify();
            }
        });
    }

    pub fn append(&self, decorations: Vec<RangeDecoration>, cx: &mut App) {
        let _ = self.state.update(cx, |state, cx| {
            let decorations = normalize(&state.text, decorations);
            if state.extras.range_decorations.append(self.id, decorations) {
                cx.notify();
            }
        });
    }

    pub fn clear(&self, cx: &mut App) {
        self.set(Vec::new(), cx);
    }

    pub fn dispose(&self, cx: &mut App) {
        let _ = self.state.update(cx, |state, cx| {
            if state
                .extras
                .range_decorations
                .entries
                .remove(&self.id)
                .is_some()
            {
                cx.notify();
            }
        });
    }

    pub fn get_ranges(&self, cx: &App) -> Vec<Range<usize>> {
        self.state
            .read_with(cx, |state, _| {
                state
                    .extras
                    .range_decorations
                    .get(self.id)
                    .unwrap_or_default()
                    .iter()
                    .map(|decoration| decoration.range.clone())
                    .collect()
            })
            .unwrap_or_default()
    }
}

pub(crate) trait TrackedDecoration {
    fn range(&self) -> &Range<usize>;
    fn range_mut(&mut self) -> &mut Range<usize>;
}

impl TrackedDecoration for TextDecoration {
    fn range(&self) -> &Range<usize> {
        &self.range
    }
    fn range_mut(&mut self) -> &mut Range<usize> {
        &mut self.range
    }
}

impl TrackedDecoration for RangeDecoration {
    fn range(&self) -> &Range<usize> {
        &self.range
    }
    fn range_mut(&mut self) -> &mut Range<usize> {
        &mut self.range
    }
}

struct DecorationIndex {
    indices: Vec<usize>,
    max_ends: Vec<usize>,
}

impl DecorationIndex {
    fn new<T: TrackedDecoration>(decorations: &[T]) -> Self {
        let mut indices: Vec<_> = (0..decorations.len()).collect();
        indices.sort_unstable_by_key(|&ix| (decorations[ix].range().start, ix));
        let mut index = Self {
            max_ends: vec![0; indices.len()],
            indices,
        };
        index.build(decorations, 0..decorations.len());
        index
    }

    fn build<T: TrackedDecoration>(&mut self, decorations: &[T], span: Range<usize>) -> usize {
        if span.is_empty() {
            return 0;
        }
        let mid = span.start + span.len() / 2;
        let end = decorations[self.indices[mid]]
            .range()
            .end
            .max(self.build(decorations, span.start..mid))
            .max(self.build(decorations, mid + 1..span.end));
        self.max_ends[mid] = end;
        end
    }

    // Returns the number of visited nodes, allowing deterministic complexity tests.
    fn query<T: TrackedDecoration>(
        &self,
        decorations: &[T],
        span: Range<usize>,
        range: &Range<usize>,
        matches: &mut Vec<usize>,
    ) -> usize {
        if span.is_empty() || range.is_empty() {
            return 0;
        }
        let mid = span.start + span.len() / 2;
        if self.max_ends[mid] <= range.start {
            return 1;
        }
        let mut visited = 1 + self.query(decorations, span.start..mid, range, matches);
        let ix = self.indices[mid];
        let candidate = decorations[ix].range();
        if candidate.start < range.end {
            if candidate.end > range.start {
                matches.push(ix);
            }
            visited += self.query(decorations, mid + 1..span.end, range, matches);
        }
        visited
    }
}

struct DecorationEntries<T> {
    decorations: Vec<T>,
    index: DecorationIndex,
}

impl<T: TrackedDecoration> DecorationEntries<T> {
    fn new(decorations: Vec<T>) -> Self {
        let index = DecorationIndex::new(&decorations);
        Self { decorations, index }
    }

    fn reindex(&mut self) {
        self.index = DecorationIndex::new(&self.decorations);
    }
}

pub(crate) struct DecorationCollections<T = TextDecoration> {
    entries: BTreeMap<DecorationCollectionId, DecorationEntries<T>>,
    next_id: usize,
}

impl<T> Default for DecorationCollections<T> {
    fn default() -> Self {
        Self {
            entries: BTreeMap::new(),
            next_id: 0,
        }
    }
}

impl<T: TrackedDecoration> DecorationCollections<T> {
    fn create(&mut self, decorations: Vec<T>) -> DecorationCollectionId {
        let id = DecorationCollectionId(self.next_id);
        self.next_id += 1;
        self.entries.insert(id, DecorationEntries::new(decorations));
        id
    }

    fn set(&mut self, id: DecorationCollectionId, decorations: Vec<T>) -> bool {
        let Some(current) = self.entries.get_mut(&id) else {
            return false;
        };
        *current = DecorationEntries::new(decorations);
        true
    }

    fn append(&mut self, id: DecorationCollectionId, decorations: Vec<T>) -> bool {
        let Some(current) = self.entries.get_mut(&id) else {
            return false;
        };
        current.decorations.extend(decorations);
        current.reindex();
        true
    }

    fn get(&self, id: DecorationCollectionId) -> Option<&[T]> {
        self.entries
            .get(&id)
            .map(|entry| entry.decorations.as_slice())
    }

    pub(super) fn adjust_for_edit(&mut self, edited_range: &Range<usize>, inserted_len: usize) {
        for entry in self.entries.values_mut() {
            let len = entry.decorations.len();
            if len == 0 || entry.index.max_ends[len / 2] <= edited_range.start {
                continue;
            }
            let mut remap = Vec::with_capacity(len);
            let mut retained = 0;
            entry.decorations.retain_mut(|decoration| {
                *decoration.range_mut() =
                    adjust_range_for_edit(decoration.range(), edited_range, inserted_len);
                let keep = !decoration.range().is_empty();
                remap.push(if keep { retained } else { usize::MAX });
                retained += usize::from(keep);
                keep
            });
            // Anchor transforms are monotone. Preserve start ordering and remap
            // removed entries rather than sorting on every keystroke: O(n).
            entry.index.indices.retain_mut(|ix| {
                *ix = remap[*ix];
                *ix != usize::MAX
            });
            entry.index.max_ends.resize(retained, 0);
            entry.index.build(&entry.decorations, 0..retained);
        }
    }

    pub(super) fn clear(&mut self) {
        for entry in self.entries.values_mut() {
            *entry = DecorationEntries::new(Vec::new());
        }
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = &[T]> {
        self.entries
            .values()
            .map(|entry| entry.decorations.as_slice())
    }

    pub(super) fn intersecting(&self, ranges: &[Range<usize>]) -> Vec<&T> {
        let mut result = Vec::new();
        for entry in self.entries.values() {
            let mut matches = Vec::new();
            for range in ranges {
                entry.index.query(
                    &entry.decorations,
                    0..entry.decorations.len(),
                    range,
                    &mut matches,
                );
            }
            matches.sort_unstable();
            matches.dedup();
            result.extend(matches.into_iter().map(|ix| &entry.decorations[ix]));
        }
        result
    }
}

fn adjust_range_for_edit(
    range: &Range<usize>,
    edited_range: &Range<usize>,
    inserted_len: usize,
) -> Range<usize> {
    let removed_len = edited_range.end.saturating_sub(edited_range.start);
    let shift = |offset: usize| {
        if inserted_len >= removed_len {
            offset.saturating_add(inserted_len - removed_len)
        } else {
            offset.saturating_sub(removed_len - inserted_len)
        }
    };

    if edited_range.is_empty() {
        let start = if range.start < edited_range.start {
            range.start
        } else {
            shift(range.start)
        };
        let end = if range.end <= edited_range.start {
            range.end
        } else {
            shift(range.end)
        };
        return start..end;
    }

    let inserted_end = edited_range.start + inserted_len;
    let start = if range.start <= edited_range.start {
        range.start
    } else if range.start >= edited_range.end {
        shift(range.start)
    } else {
        edited_range.start
    };
    let end = if range.end <= edited_range.start {
        range.end
    } else if range.end >= edited_range.end {
        shift(range.end)
    } else {
        inserted_end
    };
    start..end
}

fn normalize<T: TrackedDecoration>(text: &Rope, decorations: Vec<T>) -> Vec<T> {
    decorations
        .into_iter()
        .filter_map(|mut decoration| {
            // Reject reversed ranges before clipping, which could otherwise turn a
            // reversed pair within a multibyte character into a nonempty range.
            if decoration.range().is_empty() {
                return None;
            }
            let range = text.clip_offset(decoration.range().start, Bias::Left)
                ..text.clip_offset(decoration.range().end, Bias::Right);
            if range.is_empty() {
                return None;
            }
            *decoration.range_mut() = range;
            Some(decoration)
        })
        .collect()
}

impl InputBaseState<EditorMode> {
    pub fn create_range_decorations_collection(
        &mut self,
        decorations: Vec<RangeDecoration>,
        cx: &mut Context<Self>,
    ) -> RangeDecorationCollection {
        let id = self
            .extras
            .range_decorations
            .create(normalize(&self.text, decorations));
        cx.notify();
        RangeDecorationCollection {
            state: cx.entity().downgrade(),
            id,
        }
    }

    pub fn create_decorations_collection(
        &mut self,
        decorations: Vec<TextDecoration>,
        cx: &mut Context<Self>,
    ) -> TextDecorationCollection {
        let decorations = normalize(&self.text, decorations);
        let id = self.extras.decorations.create(decorations);
        cx.notify();
        TextDecorationCollection {
            state: cx.entity().downgrade(),
            id,
        }
    }
}
