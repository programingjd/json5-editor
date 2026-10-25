//! Application-supplied highlights over the text a [`TextViewState`] renders,
//! and scrolling one of its ranges into view.
//!
//! Ranges address the rendered text, the string plain copy produces: an
//! application searches [`TextViewState::rendered_text`] and hands the ranges
//! it found back. Each range is split into the text leaves it covers (a
//! paragraph, a heading, a code block, a table cell), which paint it as a
//! background behind their glyphs, so a highlight never changes layout. Text
//! outside every leaf (the separators between blocks and cells, custom blocks,
//! HTML blocks, inline objects) is left unpainted.
//!
//! An application that keeps ranges of the Markdown source instead, such as
//! those [`TextViewState::selected_source_range`] returns, converts them with
//! [`RenderedText::range_for_source`], which reads the source position the
//! parser recorded for each rendered character.
//!
//! [`TextViewState`]: super::TextViewState
//! [`TextViewState::rendered_text`]: super::TextViewState::rendered_text
//! [`TextViewState::selected_source_range`]: super::TextViewState::selected_source_range

#[cfg(not(target_family = "wasm"))]
use std::time::Instant;
use std::{
    ops::Range,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};
#[cfg(target_family = "wasm")]
use web_time::Instant;

use gpui::{Bounds, EntityId, Hsla, Pixels, SharedString};

use super::{
    document::ParsedDocument,
    node::{BlockNode, Paragraph, SourceSegment},
    stream_fade::{TextLeaf, TextLeafKey, text_leaves},
};

#[derive(Clone)]
pub struct RenderedText {
    owner: EntityId,
    revision: usize,
    document: ParsedDocument,
    index: Arc<OnceLock<RenderedIndex>>,
}

impl std::fmt::Debug for RenderedText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderedText")
            .field("owner", &self.owner)
            .field("revision", &self.revision)
            .finish_non_exhaustive()
    }
}

impl RenderedText {
    pub(super) fn new(
        owner: EntityId,
        revision: usize,
        document: ParsedDocument,
        index: Arc<OnceLock<RenderedIndex>>,
    ) -> Self {
        Self {
            owner,
            revision,
            document,
            index,
        }
    }

    pub fn as_str(&self) -> &str {
        &self.index().text
    }

    pub fn len(&self) -> usize {
        self.index().text.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index().text.is_empty()
    }

    pub fn source(&self) -> &str {
        &self.document.source
    }

    pub fn range_for_source(&self, source: Range<usize>) -> Option<Range<usize>> {
        let text = self.source();
        if source.start >= source.end
            || source.end > text.len()
            || !text.is_char_boundary(source.start)
            || !text.is_char_boundary(source.end)
        {
            return None;
        }
        self.index().range_for_source(&source)
    }

    pub(super) fn index(&self) -> &RenderedIndex {
        self.index
            .get_or_init(|| RenderedIndex::new(&self.document))
    }
}

impl PartialEq for RenderedText {
    fn eq(&self, other: &Self) -> bool {
        self.owner == other.owner && self.revision == other.revision
    }
}

impl Eq for RenderedText {}

#[derive(Clone, Debug, PartialEq)]
pub struct RangeHighlight {
    range: Range<usize>,
    background: Hsla,
}

impl RangeHighlight {
    pub fn new(range: Range<usize>, background: impl Into<Hsla>) -> Self {
        Self {
            range,
            background: background.into(),
        }
    }

    pub fn range(&self) -> Range<usize> {
        self.range.clone()
    }

    pub fn background(&self) -> Hsla {
        self.background
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum RangeHighlightError {
    Unsupported,

    InvalidRange(usize),
}

impl std::fmt::Display for RangeHighlightError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported => f.write_str("HTML views do not support ranges of their text"),
            Self::InvalidRange(ix) => write!(f, "range {ix} is not a range of the text"),
        }
    }
}

impl std::error::Error for RangeHighlightError {}

#[derive(Debug, Default)]
pub(super) struct RenderedIndex {
    text: SharedString,

    leaves: Vec<LeafSpan>,

    blocks: Vec<Range<usize>>,

    source_map: Vec<SourceSegment>,
}

#[derive(Debug)]
struct LeafSpan {
    range: Range<usize>,
    key: TextLeafKey,

    objects: Vec<Range<usize>>,
}

impl LeafSpan {
    fn text_offset_near(&self, offset: usize) -> Option<usize> {
        let object_at = |offset: usize| self.objects.iter().find(|object| object.contains(&offset));
        let mut after = offset;
        while let Some(object) = object_at(after) {
            after = object.end;
        }
        if after < self.range.len() {
            return Some(after);
        }
        let mut before = offset;
        while let Some(object) = object_at(before) {
            before = object.start.checked_sub(1)?;
        }
        Some(before)
    }
}

impl RenderedIndex {
    pub(super) fn new(document: &ParsedDocument) -> Self {
        let mut builder = IndexBuilder::default();
        let mut blocks = Vec::with_capacity(document.blocks.len());
        for block in document.blocks.iter() {
            let start = builder.text.len();
            builder.push_block(block);
            blocks.push(start..builder.text.len());
        }
        let index = Self {
            text: builder.text.into(),
            leaves: builder.leaves,
            blocks,
            source_map: builder.source_map,
        };
        debug_assert_eq!(index.text.as_ref(), document.text());
        index
    }

    fn resolve(&self, range: &Range<usize>) -> Option<Vec<(TextLeafKey, Range<usize>)>> {
        if range.start > range.end
            || range.end > self.text.len()
            || !self.text.is_char_boundary(range.start)
            || !self.text.is_char_boundary(range.end)
        {
            return None;
        }

        let first = self
            .leaves
            .partition_point(|leaf| leaf.range.end <= range.start);
        let mut pieces = Vec::new();
        for leaf in &self.leaves[first..] {
            if leaf.range.start >= range.end {
                break;
            }
            let end = range.end.min(leaf.range.end) - leaf.range.start;
            let mut cursor = range.start.max(leaf.range.start) - leaf.range.start;
            for object in &leaf.objects {
                if object.start >= end {
                    break;
                }
                if object.end <= cursor {
                    continue;
                }
                if object.start > cursor {
                    pieces.push((leaf.key, cursor..object.start));
                }
                cursor = object.end;
            }
            if cursor < end {
                pieces.push((leaf.key, cursor..end));
            }
        }
        Some(pieces)
    }

    fn locate(&self, range: &Range<usize>) -> Option<RevealTarget> {
        if let Some((key, leaf_range)) = self.resolve(range)?.into_iter().next() {
            return Some(RevealTarget::Line {
                key,
                offset: leaf_range.start,
            });
        }
        let block_ix = self
            .blocks
            .partition_point(|block| block.end <= range.start)
            .min(self.blocks.len().checked_sub(1)?);
        let block_start = self.blocks[block_ix].start;
        let ix = self
            .leaves
            .partition_point(|leaf| leaf.range.end <= range.start);
        let leaf_offset = match self.leaves.get(ix) {
            Some(leaf) if leaf.range.contains(&range.start) => {
                Some((leaf, range.start - leaf.range.start))
            }
            // A position after the text of a leaf, on the separators after
            // it or at the end of the text, is on the line of the last
            // character before it in its block.
            _ => ix
                .checked_sub(1)
                .and_then(|ix| self.leaves.get(ix))
                .filter(|leaf| leaf.range.start >= block_start)
                .and_then(|leaf| {
                    let (last, _) = self.text[leaf.range.clone()].char_indices().last()?;
                    Some((leaf, last))
                }),
        };
        if let Some((leaf, offset)) = leaf_offset
            && let Some(offset) = leaf.text_offset_near(offset)
        {
            return Some(RevealTarget::Line {
                key: leaf.key,
                offset,
            });
        }
        Some(RevealTarget::Block { ix: block_ix })
    }

    fn range_for_source(&self, source: &Range<usize>) -> Option<Range<usize>> {
        self.source_map
            .iter()
            .filter(|segment| {
                segment.source.start < source.end && segment.source.end > source.start
            })
            .map(|segment| {
                if !segment.linear {
                    return segment.rendered.clone();
                }
                let start = segment.source.start.max(source.start) - segment.source.start;
                let end = segment.source.end.min(source.end) - segment.source.start;
                self.text
                    .floor_char_boundary(segment.rendered.start + start)
                    ..self.text.ceil_char_boundary(segment.rendered.start + end)
            })
            .reduce(|found, range| found.start.min(range.start)..found.end.max(range.end))
    }
}

#[derive(Default)]
struct IndexBuilder {
    text: String,
    leaves: Vec<LeafSpan>,
    source_map: Vec<SourceSegment>,
}

impl IndexBuilder {
    fn push_block(&mut self, block: &BlockNode) {
        let start = self.text.len();
        match block {
            BlockNode::Root { children, .. } | BlockNode::Blockquote { children, .. } => {
                for child in children {
                    self.push_block(child);
                }
            }
            BlockNode::List { children, .. } | BlockNode::ListItem { children, .. } => {
                for child in children {
                    self.push_block(child);
                }
                return;
            }
            BlockNode::Paragraph(paragraph) => {
                self.push_paragraph(
                    paragraph,
                    paragraph.span.map(|span| TextLeafKey::block(span.start)),
                );
            }
            BlockNode::Heading { children, span, .. } => {
                self.push_paragraph(children, span.map(|span| TextLeafKey::block(span.start)));
            }
            BlockNode::Table(table) => {
                let mut ordinal = 0;
                for row in table.children.iter().filter(|row| !row.children.is_empty()) {
                    for (ix, cell) in row.children.iter().enumerate() {
                        if ix > 0 {
                            self.text.push(' ');
                        }
                        self.push_paragraph(
                            &cell.children,
                            table
                                .span
                                .map(|span| TextLeafKey::table_cell(span.start, ordinal)),
                        );
                        ordinal += 1;
                    }
                    self.text.push('\n');
                }
            }
            BlockNode::CodeBlock(code_block) => {
                let code = code_block.code();
                self.push_source_segments(self.text.len(), &code, &code_block.source_segments);
                self.push_leaf(
                    &code,
                    code_block.span.map(|span| TextLeafKey::block(span.start)),
                    Vec::new(),
                );
            }
            BlockNode::Custom { node, .. } => {
                self.push_object_source(self.text.len(), node.as_text(), node.source_range());
                self.text.push_str(node.as_text());
            }
            BlockNode::Definition { .. }
            | BlockNode::Break { .. }
            | BlockNode::HorizontalRule { .. }
            | BlockNode::Unknown => {}
        }
        if self.text.len() > start {
            self.text.push('\n');
        }
    }

    fn push_paragraph(&mut self, paragraph: &Paragraph, key: Option<TextLeafKey>) {
        let start = self.text.len();
        let mut text = String::new();
        let mut objects = Vec::new();
        for child in &paragraph.children {
            let offset = start + text.len();
            if let Some(custom) = &child.custom {
                objects.push(text.len()..text.len() + child.text.len());
                self.push_object_source(offset, &child.text, custom.source_range());
            } else {
                self.push_source_segments(offset, &child.text, &child.source_segments);
            }
            text.push_str(&child.text);
        }
        self.push_leaf(&text, key, objects);
    }

    fn push_source_segments(&mut self, offset: usize, text: &str, segments: &[SourceSegment]) {
        self.source_map.extend(
            segments
                .iter()
                .filter(|segment| {
                    !segment.rendered.is_empty()
                        && !segment.source.is_empty()
                        && segment.rendered.end <= text.len()
                        && text.is_char_boundary(segment.rendered.start)
                        && text.is_char_boundary(segment.rendered.end)
                })
                .map(|segment| SourceSegment {
                    rendered: offset + segment.rendered.start..offset + segment.rendered.end,
                    ..segment.clone()
                }),
        );
    }

    fn push_object_source(&mut self, offset: usize, text: &str, source: Option<Range<usize>>) {
        if let Some(source) = source {
            self.push_source_segments(
                offset,
                text,
                &[SourceSegment {
                    rendered: 0..text.len(),
                    source,
                    linear: false,
                }],
            );
        }
    }

    fn push_leaf(&mut self, text: &str, key: Option<TextLeafKey>, objects: Vec<Range<usize>>) {
        let start = self.text.len();
        self.text.push_str(text);
        if let Some(key) = key
            && !text.is_empty()
        {
            self.leaves.push(LeafSpan {
                range: start..self.text.len(),
                key,
                objects,
            });
        }
    }
}

fn table_row_source_ends(blocks: &[BlockNode], rows: &mut Vec<(TextLeafKey, Option<usize>)>) {
    for block in blocks {
        match block {
            BlockNode::Table(table) => {
                let Some(span) = table.span else {
                    continue;
                };
                let mut cell_count = 0;
                for row in &table.children {
                    if row.children.is_empty() {
                        continue;
                    }
                    cell_count += row.children.len();
                    let end = row
                        .children
                        .iter()
                        .filter_map(|cell| paragraph_source_end(&cell.children))
                        .max();
                    rows.push((TextLeafKey::table_cell(span.start, cell_count - 1), end));
                }
            }
            BlockNode::Root { children, .. }
            | BlockNode::Blockquote { children, .. }
            | BlockNode::List { children, .. }
            | BlockNode::ListItem { children, .. } => table_row_source_ends(children, rows),
            _ => {}
        }
    }
}

fn paragraph_source_end(paragraph: &Paragraph) -> Option<usize> {
    paragraph
        .children
        .iter()
        .flat_map(|node| {
            node.source_segments
                .iter()
                .map(|segment| segment.source.end)
                .chain(
                    node.custom
                        .as_ref()
                        .and_then(|custom| custom.source_range())
                        .map(|range| range.end),
                )
        })
        .max()
}

#[derive(Debug, Default)]
pub(crate) struct RangeHighlightFrame {
    leaves: Vec<(TextLeafKey, Vec<(Range<usize>, Hsla)>)>,
}

impl RangeHighlightFrame {
    pub(super) fn new(
        text: &RenderedText,
        highlights: impl IntoIterator<Item = RangeHighlight>,
    ) -> Result<Option<Self>, RangeHighlightError> {
        let mut pieces = Vec::new();
        for (ix, highlight) in highlights.into_iter().enumerate() {
            let leaf_ranges = text
                .index()
                .resolve(&highlight.range)
                .ok_or(RangeHighlightError::InvalidRange(ix))?;
            pieces.extend(
                leaf_ranges
                    .into_iter()
                    .map(|(key, range)| (key, range, highlight.background)),
            );
        }

        // Stable, so each leaf keeps the application's order.
        pieces.sort_by_key(|(key, _, _)| *key);
        let mut leaves: Vec<(TextLeafKey, Vec<(Range<usize>, Hsla)>)> = Vec::new();
        for (key, range, background) in pieces {
            match leaves.last_mut() {
                Some((last, backgrounds)) if *last == key => backgrounds.push((range, background)),
                _ => leaves.push((key, vec![(range, background)])),
            }
        }
        Ok((!leaves.is_empty()).then_some(Self { leaves }))
    }

    pub(crate) fn backgrounds(&self, key: TextLeafKey) -> &[(Range<usize>, Hsla)] {
        self.leaves
            .binary_search_by_key(&key, |(leaf, _)| *leaf)
            .map_or(&[], |ix| self.leaves[ix].1.as_slice())
    }

    pub(super) fn remap(&self, remap: &LeafRemap) -> Option<Self> {
        let mut leaves = self
            .leaves
            .iter()
            .filter_map(|(key, backgrounds)| {
                let (new_key, unchanged) = remap.leaf(*key)?;
                let clipped = backgrounds
                    .iter()
                    .filter(|(range, _)| range.start < unchanged)
                    .map(|(range, background)| (range.start..range.end.min(unchanged), *background))
                    .collect::<Vec<_>>();
                (!clipped.is_empty()).then_some((new_key, clipped))
            })
            .collect::<Vec<_>>();
        // Moving keys keeps their order, but stay safe for the binary search.
        leaves.sort_by_key(|(key, _)| *key);
        (!leaves.is_empty()).then_some(Self { leaves })
    }
}

pub(super) struct LeafRemap<'a> {
    old_len: usize,
    new_len: usize,

    tail_start: Option<usize>,
    unchanged_prefix: usize,
    unchanged_suffix: usize,
    old_leaves: Vec<(TextLeafKey, TextLeaf<'a>)>,
    new_leaves: Vec<(TextLeafKey, TextLeaf<'a>)>,

    table_rows: Vec<(TextLeafKey, Option<usize>)>,
}

impl<'a> LeafRemap<'a> {
    pub(super) fn new(old: &'a ParsedDocument, new: &'a ParsedDocument, tail_only: bool) -> Self {
        let (old_len, new_len) = (old.source.len(), new.source.len());
        // An append starts after the old source, and parses its last block
        // again, or only the new text when that block has no span.
        let tail_start = tail_only.then(|| {
            old.blocks
                .last()
                .and_then(BlockNode::span)
                .map_or(old_len, |span| span.start)
        });
        let (unchanged_prefix, unchanged_suffix) = if tail_only {
            (old_len, 0)
        } else {
            let prefix = old
                .source
                .bytes()
                .zip(new.source.bytes())
                .take_while(|(old, new)| old == new)
                .count();
            let shorter = old_len.min(new_len);
            if prefix == shorter {
                // One source extends the other, as when text is appended.
                (prefix, 0)
            } else {
                // Where the two overlap, as when a deleted block starts like
                // the block after it, the end wins: the blocks after a change
                // keep following their text rather than their offset.
                let suffix = old
                    .source
                    .bytes()
                    .rev()
                    .zip(new.source.bytes().rev())
                    .take_while(|(old, new)| old == new)
                    .count()
                    .min(shorter);
                (prefix.min(shorter - suffix), suffix)
            }
        };

        fn leaves_from(
            document: &ParsedDocument,
            tail_start: Option<usize>,
        ) -> Vec<(TextLeafKey, TextLeaf<'_>)> {
            let mut leaves = Vec::new();
            for block in document.blocks.iter().rev() {
                if let Some(tail_start) = tail_start
                    && block.span().is_none_or(|span| span.start < tail_start)
                {
                    break;
                }
                text_leaves(block, &mut leaves);
            }
            leaves.sort_by_key(|(key, _)| *key);
            leaves
        }

        let mut table_rows = Vec::new();
        if !tail_only {
            table_row_source_ends(&old.blocks, &mut table_rows);
            table_rows.sort_by_key(|(key, _)| *key);
        }

        Self {
            old_len,
            new_len,
            tail_start,
            unchanged_prefix,
            unchanged_suffix,
            old_leaves: leaves_from(old, tail_start),
            new_leaves: leaves_from(new, tail_start),
            table_rows,
        }
    }

    pub(super) fn leaf(&self, key: TextLeafKey) -> Option<(TextLeafKey, usize)> {
        if self
            .tail_start
            .is_some_and(|tail_start| key.block_start() < tail_start)
        {
            return Some((key, usize::MAX));
        }
        let new_key = key.moved_to(self.moved(key.block_start())?);
        let old_leaf = Self::find(&self.old_leaves, key)?;
        // A table's cells are only known by their place in it, so after a
        // change inside the table a cell is the same one only when the source
        // of its whole row ends before that change.
        if key.cell_ix().is_some()
            && key.block_start() < self.unchanged_prefix
            && self.tail_start.is_none()
            && self
                .row_source_end(key)
                .is_none_or(|end| end > self.unchanged_prefix)
        {
            return None;
        }
        let new_leaf = Self::find(&self.new_leaves, new_key)?;
        Some((new_key, new_leaf.common_prefix_len(old_leaf)))
    }

    fn row_source_end(&self, key: TextLeafKey) -> Option<usize> {
        let ix = self.table_rows.partition_point(|(last, _)| *last < key);
        let (last, end) = self.table_rows.get(ix)?;
        if last.block_start() != key.block_start() {
            return None;
        }
        *end
    }

    fn moved(&self, start: usize) -> Option<usize> {
        if start < self.unchanged_prefix {
            Some(start)
        } else if start >= self.old_len - self.unchanged_suffix {
            Some(start + self.new_len - self.old_len)
        } else {
            None
        }
    }

    fn find<'b>(
        leaves: &'b [(TextLeafKey, TextLeaf<'a>)],
        key: TextLeafKey,
    ) -> Option<&'b TextLeaf<'a>> {
        let ix = leaves.binary_search_by_key(&key, |(leaf, _)| *leaf).ok()?;
        Some(&leaves[ix].1)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum RevealTarget {
    Line { key: TextLeafKey, offset: usize },

    Block { ix: usize },
}

const REVEAL_TIMEOUT: Duration = Duration::from_secs(1);

const REVEAL_ATTEMPTS: usize = 8;

#[derive(Clone, Copy, Debug)]
struct RevealReport {
    line: Bounds<Pixels>,
    visible: bool,
}

#[derive(Debug)]
pub(super) struct PendingReveal {
    target: RevealTarget,
    requested_at: Instant,

    report: Arc<Mutex<Option<RevealReport>>>,
    attempts: usize,
}

pub(super) enum RevealProgress {
    Shown,

    Hidden(Bounds<Pixels>),

    NotLaidOut,
}

impl PendingReveal {
    pub(super) fn new(text: &RenderedText, range: &Range<usize>, now: Instant) -> Option<Self> {
        Some(Self {
            target: text.index().locate(range)?,
            requested_at: now,
            report: Arc::default(),
            attempts: 0,
        })
    }

    pub(super) fn is_expired(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.requested_at) > REVEAL_TIMEOUT
            || self.attempts >= REVEAL_ATTEMPTS
    }

    pub(super) fn is_block(&self) -> bool {
        matches!(self.target, RevealTarget::Block { .. })
    }

    pub(super) fn block_ix(&self, document: &ParsedDocument) -> Option<usize> {
        match self.target {
            RevealTarget::Line { key, .. } => document.blocks.iter().rposition(|block| {
                block
                    .span()
                    .is_some_and(|span| span.start <= key.block_start())
            }),
            RevealTarget::Block { ix } => (ix < document.blocks.len()).then_some(ix),
        }
    }

    pub(super) fn was_laid_out(&self) -> bool {
        self.report.lock().is_ok_and(|report| report.is_some())
    }

    pub(super) fn request(&self) -> Option<RevealRequest> {
        if let Ok(mut report) = self.report.lock() {
            *report = None;
        }
        let RevealTarget::Line { key, offset } = self.target else {
            return None;
        };
        Some(RevealRequest {
            key,
            offset,
            report: self.report.clone(),
        })
    }

    pub(super) fn progress(&mut self) -> RevealProgress {
        let report = self.report.lock().ok().and_then(|report| *report);
        match report {
            Some(report) if report.visible => RevealProgress::Shown,
            Some(report) => {
                self.attempts += 1;
                RevealProgress::Hidden(report.line)
            }
            None => RevealProgress::NotLaidOut,
        }
    }

    pub(super) fn remap(mut self, remap: &LeafRemap) -> Option<Self> {
        let RevealTarget::Line { key, offset } = self.target else {
            return None;
        };
        let (key, unchanged) = remap.leaf(key)?;
        (offset < unchanged).then_some(())?;
        self.target = RevealTarget::Line { key, offset };
        Some(self)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct RevealRequest {
    key: TextLeafKey,
    offset: usize,
    report: Arc<Mutex<Option<RevealReport>>>,
}

impl RevealRequest {
    pub(crate) fn at(
        &self,
        key: Option<TextLeafKey>,
        start: usize,
        end: usize,
    ) -> Option<RevealAt> {
        if key != Some(self.key) {
            return None;
        }
        RevealAt {
            offset: self.offset,
            report: self.report.clone(),
        }
        .rebase(start, end)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct RevealAt {
    offset: usize,
    report: Arc<Mutex<Option<RevealReport>>>,
}

impl RevealAt {
    pub(crate) fn offset(&self) -> usize {
        self.offset
    }

    pub(crate) fn rebase(&self, start: usize, end: usize) -> Option<Self> {
        (start..end).contains(&self.offset).then(|| Self {
            offset: self.offset - start,
            report: self.report.clone(),
        })
    }

    pub(crate) fn clamp(&self, start: usize, end: usize) -> Self {
        Self {
            offset: self.offset.clamp(start, end) - start,
            report: self.report.clone(),
        }
    }

    pub(crate) fn report(&self, line: Bounds<Pixels>, visible: bool) {
        if let Ok(mut report) = self.report.lock() {
            *report = Some(RevealReport { line, visible });
        }
    }
}
