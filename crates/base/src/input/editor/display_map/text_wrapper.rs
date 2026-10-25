use super::inline_line::InputLine;
use gpui::Half;
use std::borrow::Cow;
use std::ops::Range;
use std::rc::Rc;

use gpui::{
    App, Font, LineFragment, Pixels, Point, ShapedLine, Size, TextAlign, Window, point, px, size,
};
use ropey::Rope;
use smallvec::SmallVec;
use sum_tree::{Bias, Dimensions, SumTree};
use unicode_segmentation::UnicodeSegmentation as _;

use crate::input::{
    Point as TreeSitterPoint, RopeExt,
    layout::{LastLayout, WhitespaceIndicators},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WrappingIndent {
    None,

    #[default]
    Same,
}

fn ascii_wrap_boundaries(
    text: &str,
    width: Pixels,
    cell_width: Pixels,
    wrapping_indent: WrappingIndent,
) -> Vec<gpui::Boundary> {
    if cell_width <= px(0.) {
        return Vec::new();
    }

    let columns = ((width / cell_width).floor() as usize).max(1);
    let indent = if wrapping_indent == WrappingIndent::Same {
        text.bytes()
            .take_while(|&byte| byte == b' ')
            .count()
            .min(gpui::LineWrapper::MAX_INDENT as usize)
    } else {
        0
    };
    let continuation_columns = columns.saturating_sub(indent).max(1);
    let mut boundaries = Vec::new();
    let mut start = 0;

    while start < text.len() {
        let available = if start == 0 {
            columns
        } else {
            continuation_columns
        };
        let end = (start + available).min(text.len());
        if end == text.len() {
            break;
        }

        let break_at = text[start..end]
            .rfind(' ')
            .map(|ix| start + ix + 1)
            .filter(|&ix| ix > start && (start != 0 || ix > indent))
            .unwrap_or(end);
        boundaries.push(gpui::Boundary {
            ix: break_at,
            next_indent: indent as u32,
        });
        start = break_at;
    }

    boundaries
}

fn measured_wrap_boundaries(
    text: &str,
    width: Pixels,
    wrapping_indent: WrappingIndent,
    mut measure: impl FnMut(&str) -> Pixels,
) -> Vec<gpui::Boundary> {
    // Most formatted JSON lines fit without wrapping. Measure short Unicode
    // lines once instead of shaping successively longer prefixes and building
    // grapheme/line-break tables. Bound this probe so a huge pasted line still
    // uses the local exponential search below.
    if text.len() <= 512 && measure(text) <= width {
        return Vec::new();
    }

    let indent = if wrapping_indent == WrappingIndent::Same {
        text.chars()
            .take_while(|&c| c == ' ')
            .count()
            .min(gpui::LineWrapper::MAX_INDENT as usize)
    } else {
        0
    };
    let indent_width = measure(&text[..indent]);
    let ends: Vec<usize> = text
        .grapheme_indices(true)
        .map(|(ix, grapheme)| ix + grapheme.len())
        .collect();
    let opportunities: Vec<usize> = unicode_linebreak::linebreaks(text)
        .map(|(ix, _)| ix)
        .filter(|ix| ends.binary_search(ix).is_ok())
        .collect();
    let mut result = Vec::new();
    let mut first = 0;
    let mut start = 0;
    while first < ends.len() {
        let available = if start == 0 {
            width
        } else {
            width - indent_width
        };
        // Find the fitting prefix locally. Exponential probing avoids shaping
        // the entire remaining logical line for every visual row of a long paste.
        let remaining = ends.len() - first;
        let mut low = 0;
        let mut high = 1;
        while measure(&text[start..ends[first + high - 1]]) <= available {
            low = high;
            if high == remaining {
                break;
            }
            high = (high * 2).min(remaining);
        }
        while low + 1 < high {
            let mid = (low + high) / 2;
            if measure(&text[start..ends[first + mid - 1]]) <= available {
                low = mid;
            } else {
                high = mid;
            }
        }
        // An indivisible grapheme wider than the viewport still consumes a row.
        let fitting_end = ends[first + low.max(1) - 1];
        if fitting_end == text.len() {
            break;
        }
        let candidate = opportunities.partition_point(|&ix| ix <= fitting_end);
        let end = candidate
            .checked_sub(1)
            .map(|ix| opportunities[ix])
            .filter(|&ix| ix > start && (start != 0 || ix > indent))
            .unwrap_or(fitting_end);
        result.push(gpui::Boundary {
            ix: end,
            next_indent: indent as u32,
        });
        first = ends.partition_point(|&ix| ix <= end);
        start = end;
    }
    result
}

fn shaped_wrap_boundaries(
    text: &str,
    width: Pixels,
    wrapping_indent: WrappingIndent,
    mut shape: impl FnMut(&str) -> std::sync::Arc<gpui::LineLayout>,
) -> Vec<gpui::Boundary> {
    // Bound eager shaping for very large pasted/minified logical lines.
    if text.len() > 16 * 1024 {
        return measured_wrap_boundaries(text, width, wrapping_indent, |text| shape(text).width);
    }
    let layout = shape(text);
    if layout.width <= width {
        return Vec::new();
    }
    let positions: Vec<_> = layout
        .runs
        .iter()
        .flat_map(|run| {
            run.glyphs
                .iter()
                .map(|glyph| (glyph.index, glyph.position.x))
        })
        .collect();
    // The prefix search requires increasing advances. Keep contextual/RTL
    // layouts on the existing measured path when that invariant does not hold.
    if positions
        .windows(2)
        .any(|pair| pair[0].0 > pair[1].0 || pair[0].1 > pair[1].1)
    {
        return measured_wrap_boundaries(text, width, wrapping_indent, |text| shape(text).width);
    }
    let x = |index| {
        positions
            .get(positions.partition_point(|&(at, _)| at < index))
            .map(|&(_, x)| x)
            .unwrap_or(layout.width)
    };
    let boundaries = measured_wrap_boundaries(text, width, wrapping_indent, |fragment| {
        let start = fragment.as_ptr() as usize - text.as_ptr() as usize;
        x(start + fragment.len()) - x(start)
    });
    // Shaping a fragment in isolation can change kerning or joining. Verify
    // each final row once; if it overflows, use the measured algorithm for
    // this logical line instead of letting painted text cross a wrap edge.
    let indent_width = boundaries
        .first()
        .map(|boundary| x(boundary.next_indent as usize) - x(0))
        .unwrap_or(px(0.));
    let mut start = 0;
    for end in boundaries
        .iter()
        .map(|boundary| boundary.ix)
        .chain(std::iter::once(text.len()))
    {
        let actual = shape(&text[start..end]).width;
        let available = if start == 0 {
            width
        } else {
            width - indent_width
        };
        if actual > available {
            return measured_wrap_boundaries(text, width, wrapping_indent, |text| {
                shape(text).width
            });
        }
        start = end;
    }
    boundaries
}

#[derive(Debug, Clone)]
pub(crate) struct LineItem {
    len: usize,

    // A width at which this unwrapped line fits. Exact when measured, conservative
    // for inline elements; invalidated along with the line on text/font changes.
    fit_width: Pixels,

    pub(crate) indent: u32,

    pub(crate) wrapped_lines: SmallVec<[Range<usize>; 1]>,
}

impl LineItem {
    #[inline]
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    #[inline]
    pub(crate) fn lines_len(&self) -> usize {
        self.wrapped_lines.len()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct LineSummary {
    buffer_rows: usize,

    wrap_rows: usize,

    bytes: usize,

    max_line_len: usize,

    longest_row: usize,

    max_unwrapped_width: Pixels,
}

impl sum_tree::Summary for LineSummary {
    type Context<'a> = &'a ();

    fn zero(_: &()) -> Self {
        LineSummary {
            buffer_rows: 0,
            wrap_rows: 0,
            bytes: 0,
            max_line_len: 0,
            longest_row: 0,
            max_unwrapped_width: px(0.),
        }
    }

    fn add_summary(&mut self, other: &Self, _: &()) {
        // Keep the leftmost row that achieves a strictly greater length
        if other.max_line_len > self.max_line_len {
            self.longest_row = self.buffer_rows + other.longest_row;
            self.max_line_len = other.max_line_len;
        }
        self.buffer_rows += other.buffer_rows;
        self.wrap_rows += other.wrap_rows;
        self.bytes += other.bytes;
        self.max_unwrapped_width = self.max_unwrapped_width.max(other.max_unwrapped_width);
    }
}

impl sum_tree::Item for LineItem {
    type Summary = LineSummary;

    fn summary(&self, _: &()) -> LineSummary {
        LineSummary {
            buffer_rows: 1,
            wrap_rows: self.lines_len(),
            bytes: self.len(),
            max_line_len: self.len(),
            longest_row: 0,
            max_unwrapped_width: if self.lines_len() == 1 {
                self.fit_width
            } else {
                px(0.)
            },
        }
    }
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct BufferRows(pub usize);

impl<'a> sum_tree::Dimension<'a, LineSummary> for BufferRows {
    fn zero(_: &()) -> Self {
        BufferRows(0)
    }

    fn add_summary(&mut self, summary: &'a LineSummary, _: &()) {
        self.0 += summary.buffer_rows;
    }
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct WrapRows(pub usize);

impl<'a> sum_tree::Dimension<'a, LineSummary> for WrapRows {
    fn zero(_: &()) -> Self {
        WrapRows(0)
    }

    fn add_summary(&mut self, summary: &'a LineSummary, _: &()) {
        self.0 += summary.wrap_rows;
    }
}

#[derive(Clone)]
pub(crate) struct TextWrapper {
    revision: u64,
    text: Rope,
    font: Font,
    font_size: Pixels,

    wrap_width: Option<Pixels>,
    wrapping_indent: WrappingIndent,

    pub(crate) lines: SumTree<LineItem>,

    inline_metrics: Rc<[(Range<usize>, Pixels)]>,
    _initialized: bool,
}

/// A private wrapping snapshot. Its line tree shares unchanged nodes with the live layout.
pub(crate) struct RewrapJob {
    target: TextWrapper,
    source_revision: u64,
    next_row: usize,
    rewrapped_lines: bool,
    preparing: bool,
    text_system: Option<gpui::WindowTextSystem>,
    processed_rows: usize,
    polls: usize,
    started: Option<web_time::Instant>,
    work_duration: std::time::Duration,
}

impl RewrapJob {
    fn needs_rewrap(summary: &LineSummary, width: Option<Pixels>) -> bool {
        summary.wrap_rows > summary.buffer_rows
            || width.is_some_and(|width| summary.max_unwrapped_width > width)
    }

    pub(crate) fn can_reuse_all_lines(&self) -> bool {
        !Self::needs_rewrap(self.target.lines.summary(), self.target.wrap_width)
    }

    pub(crate) fn advance(&mut self, cx: &mut App) -> bool {
        let started = web_time::Instant::now();
        self.started.get_or_insert(started);
        self.polls += 1;
        let rows = self.target.text.len_lines(ropey::LineType::LF);
        // Work between browser frames. Never walk the full document in one poll.
        while self.next_row < rows && started.elapsed() < std::time::Duration::from_millis(8) {
            let width = self.target.wrap_width;
            let first = if self.preparing {
                self.next_row
            } else {
                let mut cursor = self.target.lines.cursor::<BufferRows>(&());
                cursor.seek(&BufferRows(self.next_row), Bias::Right);
                if cursor
                    .item_summary()
                    .is_some_and(|s| !Self::needs_rewrap(s, width))
                {
                    cursor.search_forward(|s| Self::needs_rewrap(s, width));
                }
                if cursor.item().is_none() {
                    self.next_row = rows;
                    break;
                }
                cursor.start().0
            };
            let start = self.target.text.line_start_offset(first);
            let mut end_row = first + 1;
            let mut cursor = self.target.lines.cursor::<BufferRows>(&());
            if !self.preparing {
                cursor.seek(&BufferRows(first), Bias::Right);
            }
            while end_row < rows && end_row < first + 16 {
                if !self.preparing {
                    cursor.next();
                    if cursor
                        .item_summary()
                        .is_none_or(|s| !Self::needs_rewrap(s, width))
                    {
                        break;
                    }
                }
                if self.target.text.line_end_offset(end_row) - start > 16 * 1024 {
                    break;
                }
                end_row += 1;
            }
            drop(cursor);
            let end = self.target.text.line_end_offset(end_row - 1);
            let text = self.target.text.clone();
            let text_system = self
                .text_system
                .get_or_insert_with(|| gpui::WindowTextSystem::new(cx.text_system().clone()));
            self.target.update_range_measured(
                &text,
                &(start..end),
                end - start,
                self.preparing,
                text_system,
                cx,
            );
            self.processed_rows += end_row - first;
            self.rewrapped_lines = true;
            self.next_row = end_row;
        }
        self.work_duration += started.elapsed();
        self.next_row == rows
    }

    pub(crate) fn is_complete(&self) -> bool {
        self.next_row == self.target.text.len_lines(ropey::LineType::LF)
    }

    pub(crate) fn processed_bytes(&self) -> usize {
        if self.is_complete() {
            self.target.text.len()
        } else {
            self.target.text.line_start_offset(self.next_row)
        }
    }

    pub(crate) fn has_complete_layout(&self) -> bool {
        let summary = self.target.lines.summary();
        summary.buffer_rows == self.target.text.len_lines(ropey::LineType::LF)
            && summary.bytes + summary.buffer_rows.saturating_sub(1) == self.target.text.len()
    }

    pub(crate) fn text(&self) -> &Rope {
        &self.target.text
    }

    pub(crate) fn into_wrapper(self) -> TextWrapper {
        debug_assert!(self.is_complete());
        self.target
    }

    pub(crate) fn report_timing(&self, restarts: usize) {
        let elapsed = self
            .started
            .map_or(0., |started| started.elapsed().as_secs_f64() * 1000.);
        tracing::info!(
            "[JSON5 layout] {} / {} rows rewrapped; {} batches; wrapping {:.1} ms; elapsed {:.1} ms; {} restarts",
            self.processed_rows,
            self.target.lines_count(),
            self.polls,
            self.work_duration.as_secs_f64() * 1000.,
            elapsed,
            restarts,
        );
    }
}

impl TextWrapper {
    pub(crate) fn begin_rewrap(
        &self,
        width: Option<Pixels>,
        indent: WrappingIndent,
    ) -> Option<RewrapJob> {
        if self.wrap_width == width && self.wrapping_indent == indent {
            return None;
        }
        let mut target = self.clone();
        target.wrap_width = width;
        target.wrapping_indent = indent;
        Some(RewrapJob {
            target,
            source_revision: self.revision,
            next_row: 0,
            rewrapped_lines: false,
            preparing: false,
            text_system: None,
            processed_rows: 0,
            polls: 0,
            started: None,
            work_duration: std::time::Duration::ZERO,
        })
    }
    pub(crate) fn begin_preparation(
        &self,
        text: Rope,
        width: Option<Pixels>,
        indent: WrappingIndent,
    ) -> RewrapJob {
        let mut target = Self::new(self.font.clone(), self.font_size, width);
        target.text = text;
        target.wrapping_indent = indent;
        target._initialized = true;
        RewrapJob {
            target,
            source_revision: self.revision,
            next_row: 0,
            rewrapped_lines: false,
            preparing: true,
            text_system: None,
            processed_rows: 0,
            polls: 0,
            started: None,
            work_duration: std::time::Duration::ZERO,
        }
    }

    pub(crate) fn preparation_matches(
        &self,
        job: &RewrapJob,
        width: Option<Pixels>,
        indent: WrappingIndent,
    ) -> bool {
        self.font == job.target.font
            && self.font_size == job.target.font_size
            && width == job.target.wrap_width
            && indent == job.target.wrapping_indent
    }

    pub(crate) fn rewrap_is_current(&self, job: &RewrapJob) -> bool {
        self.revision == job.source_revision
    }
    pub(crate) fn finish_rewrap(&mut self, mut job: RewrapJob) -> bool {
        debug_assert!(self.rewrap_is_current(&job));
        job.target.revision = self.revision.wrapping_add(1);
        *self = job.target;
        job.rewrapped_lines
    }

    fn rewrap_now(&mut self, width: Option<Pixels>, indent: WrappingIndent, cx: &mut App) -> bool {
        let Some(mut job) = self.begin_rewrap(width, indent) else {
            return false;
        };
        while !job.advance(cx) {}
        self.finish_rewrap(job)
    }
}

#[allow(unused)]
impl TextWrapper {
    pub(crate) fn new(font: Font, font_size: Pixels, wrap_width: Option<Pixels>) -> Self {
        Self {
            revision: 0,
            text: Rope::new(),
            font,
            font_size,
            wrap_width,
            wrapping_indent: WrappingIndent::default(),
            lines: SumTree::new(&()),
            inline_metrics: Rc::from([]),
            _initialized: false,
        }
    }

    #[inline]
    pub(crate) fn set_default_text(&mut self, text: &Rope) {
        self.text = text.clone();
        self.revision = self.revision.wrapping_add(1);
    }

    #[inline]
    pub(crate) fn text(&self) -> &Rope {
        &self.text
    }

    #[inline]
    pub(crate) fn len(&self) -> usize {
        self.lines.summary().wrap_rows
    }

    #[inline]
    pub(crate) fn lines_count(&self) -> usize {
        self.lines.summary().buffer_rows
    }

    #[inline]
    pub(crate) fn longest_row(&self) -> usize {
        self.lines.summary().longest_row
    }

    #[inline]
    pub(crate) fn line(&self, row: usize) -> Option<&LineItem> {
        let mut cursor = self.lines.cursor::<BufferRows>(&());
        cursor.seek(&BufferRows(row), Bias::Right);
        cursor.item()
    }

    #[inline]
    pub(crate) fn iter_lines(&self) -> impl Iterator<Item = &LineItem> {
        self.lines.iter()
    }

    pub(crate) fn buffer_line_to_first_wrap_row(&self, row: usize) -> usize {
        let mut cursor = self.lines.cursor::<Dimensions<BufferRows, WrapRows>>(&());
        cursor.seek(&BufferRows(row), Bias::Right);
        cursor.start().1.0
    }

    pub(crate) fn buffer_line_to_wrap_row_range(&self, row: usize) -> Range<usize> {
        let mut cursor = self.lines.cursor::<Dimensions<BufferRows, WrapRows>>(&());
        cursor.seek(&BufferRows(row), Bias::Right);
        let start = cursor.start().1.0;
        let len = cursor.item().map(|l| l.lines_len()).unwrap_or(0);
        start..start + len
    }

    pub(crate) fn wrap_row_to_buffer_line(&self, wrap_row: usize) -> usize {
        let mut cursor = self.lines.cursor::<Dimensions<WrapRows, BufferRows>>(&());
        cursor.seek(&WrapRows(wrap_row), Bias::Right);
        match cursor.item() {
            Some(_) => cursor.start().1.0,
            None => self.lines_count().saturating_sub(1),
        }
    }

    pub(crate) fn set_wrap_width(&mut self, wrap_width: Option<Pixels>, cx: &mut App) -> bool {
        self.rewrap_now(wrap_width, self.wrapping_indent, cx)
    }

    pub(crate) fn set_wrapping_indent(
        &mut self,
        wrapping_indent: WrappingIndent,
        cx: &mut App,
    ) -> bool {
        self.rewrap_now(self.wrap_width, wrapping_indent, cx)
    }

    pub(crate) fn set_font(&mut self, font: Font, font_size: Pixels, cx: &mut App) -> bool {
        if self.font.eq(&font) && self.font_size == font_size {
            return false;
        }

        self.font = font;
        self.font_size = font_size;
        self.update_all(&self.text.clone(), cx);
        true
    }

    pub(crate) fn prepare_if_need(&mut self, text: &Rope, cx: &mut App) -> bool {
        if self._initialized {
            return false;
        }
        self._initialized = true;
        self.update_all(text, cx);
        true
    }

    pub(crate) fn update(
        &mut self,
        changed_text: &Rope,
        range: &Range<usize>,
        new_text: &Rope,
        cx: &mut App,
    ) {
        self.update_range(changed_text, range, new_text.len(), cx);
    }

    fn update_range(
        &mut self,
        changed_text: &Rope,
        range: &Range<usize>,
        new_len: usize,
        cx: &mut App,
    ) {
        let text_system = gpui::WindowTextSystem::new(cx.text_system().clone());
        self.update_range_measured(changed_text, range, new_len, false, &text_system, cx);
    }

    fn update_range_measured(
        &mut self,
        changed_text: &Rope,
        range: &Range<usize>,
        new_len: usize,
        append: bool,
        text_system: &gpui::WindowTextSystem,
        cx: &mut App,
    ) {
        let mut line_wrapper = cx
            .text_system()
            .line_wrapper(self.font.clone(), self.font_size);
        let metrics = self.inline_metrics.clone();
        let font = self.font.clone();
        let font_size = self.font_size;
        // The code editor uses a monospace font. Measure one ASCII cell once
        // so ordinary short lines can skip shaping and Unicode line-break
        // analysis entirely during a full-document wrap pass.
        let ascii_cell_width = text_system
            .layout_line(
                "M",
                font_size,
                &[gpui::TextRun {
                    len: 1,
                    font: font.clone(),
                    color: gpui::black(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                }],
                None,
            )
            .width;
        let wrapping_indent = self.wrapping_indent;
        self._update(
            changed_text,
            range,
            new_len,
            append,
            &mut |line_str, wrap_width, line_start| {
                let mut fragments = Vec::new();
                let mut offset = 0;
                let first = metrics.partition_point(|(r, _)| r.end <= line_start);
                for (range, width) in &metrics[first..] {
                    if range.start >= line_start + line_str.len() {
                        break;
                    }
                    if range.start < line_start || range.end > line_start + line_str.len() {
                        continue;
                    }
                    let range = range.start - line_start..range.end - line_start;
                    if !line_str.is_char_boundary(range.start)
                        || !line_str.is_char_boundary(range.end)
                    {
                        continue;
                    }
                    if offset < range.start {
                        fragments.push(LineFragment::text(&line_str[offset..range.start]));
                    }
                    fragments.push(LineFragment::element(*width, range.len()));
                    offset = range.end;
                }
                if fragments.is_empty() {
                    if line_str
                        .as_bytes()
                        .iter()
                        .all(|byte| (b' '..=b'~').contains(byte))
                    {
                        return (
                            ascii_wrap_boundaries(
                                line_str,
                                wrap_width,
                                ascii_cell_width,
                                wrapping_indent,
                            ),
                            ascii_cell_width * line_str.len() as f32,
                        );
                    }

                    let mut fit_width = wrap_width;
                    let boundaries =
                        shaped_wrap_boundaries(line_str, wrap_width, wrapping_indent, |text| {
                            let layout = text_system.layout_line(
                                text,
                                font_size,
                                &[gpui::TextRun {
                                    len: text.len(),
                                    font: font.clone(),
                                    color: gpui::black(),
                                    background_color: None,
                                    underline: None,
                                    strikethrough: None,
                                }],
                                None,
                            );
                            if text.len() == line_str.len() {
                                fit_width = layout.width;
                            }
                            layout
                        });
                    return (boundaries, fit_width);
                }
                if offset < line_str.len() {
                    fragments.push(LineFragment::text(&line_str[offset..]));
                }
                (
                    line_wrapper
                        .wrap_line(&fragments, wrap_width, gpui::IndentAdjustment::SameIndent)
                        .collect(),
                    wrap_width,
                )
            },
        );
    }

    pub(crate) fn adjust_inline_metrics(&mut self, range: &Range<usize>, new_len: usize) {
        if self.inline_metrics.is_empty() {
            return;
        }
        let shift = new_len as isize - range.len() as isize;
        self.inline_metrics = self
            .inline_metrics
            .iter()
            .filter_map(|(token, width)| {
                if token.start < range.end && range.start < token.end {
                    return None;
                }
                let token = if token.start >= range.end {
                    token.start.checked_add_signed(shift)?..token.end.checked_add_signed(shift)?
                } else {
                    token.clone()
                };
                Some((token, *width))
            })
            .collect();
    }

    pub(crate) fn set_inline_metrics(
        &mut self,
        metrics: Rc<[(Range<usize>, Pixels)]>,
        cx: &mut App,
    ) -> bool {
        if self.inline_metrics == metrics {
            return false;
        }
        // Only rows whose element geometry changed need another wrap pass.
        let mut affected = Vec::new();
        let (mut old, mut new) = (
            self.inline_metrics.iter().peekable(),
            metrics.iter().peekable(),
        );
        while old.peek().is_some() || new.peek().is_some() {
            match (old.peek(), new.peek()) {
                (Some(a), Some(b)) if a == b => {
                    old.next();
                    new.next();
                }
                (Some(a), Some(b)) if a.0.start <= b.0.start => {
                    affected.push(a.0.clone());
                    old.next();
                }
                (Some(_), Some(b)) | (None, Some(b)) => {
                    affected.push(b.0.clone());
                    new.next();
                }
                (Some(a), None) => {
                    affected.push(a.0.clone());
                    old.next();
                }
                (None, None) => break,
            }
        }
        self.inline_metrics = metrics;
        self.revision = self.revision.wrapping_add(1);
        let text = self.text.clone();
        let mut rows: Vec<usize> = affected
            .iter()
            .map(|r| text.offset_to_point(r.start.min(text.len())).row)
            .collect();
        rows.sort_unstable();
        rows.dedup();
        for row in rows {
            let start = text.line_start_offset(row);
            let end = text.line_end_offset(row);
            self.update(
                &text,
                &(start..end),
                &Rope::from(text.slice(start..end).to_string()),
                cx,
            );
        }
        true
    }

    fn _update<F>(
        &mut self,
        changed_text: &Rope,
        range: &Range<usize>,
        new_len: usize,
        append: bool,
        wrap_line: &mut F,
    ) where
        F: FnMut(&str, Pixels, usize) -> (Vec<gpui::Boundary>, Pixels),
    {
        // Remove the old changed lines.
        let buffer_line_count = self.lines_count();
        let start_row = self.text.offset_to_point(range.start).row;
        let start_row = start_row.min(buffer_line_count.saturating_sub(1));
        let end_row = self.text.offset_to_point(range.end).row;
        let end_row = end_row.min(buffer_line_count.saturating_sub(1));

        // To add the new lines.
        let new_start_row = changed_text.offset_to_point(range.start).row;
        let new_end_row = changed_text.offset_to_point(range.start + new_len).row;

        let mut new_lines = Vec::with_capacity(new_end_row.saturating_sub(new_start_row) + 1);
        let wrap_width = self.wrap_width;
        let changed_line_count = changed_text.len_lines(ropey::LineType::LF);
        let mut line_start_offset = changed_text.line_start_offset(new_start_row);

        // line not contains `\n`.
        for row in new_start_row..=new_end_row {
            let line = changed_text.slice_line(row);
            let mut wrapped_lines = SmallVec::<[Range<usize>; 1]>::new();
            let mut prev_boundary_ix = 0;
            let mut indent_chars = 0;
            let mut fit_width = px(f32::MAX);

            // If wrap_width is Pixels::MAX, skip wrapping to disable word wrap
            if let Some(wrap_width) = wrap_width {
                // Borrowed for lines within a single rope chunk.
                let line_str: Cow<str> = line.into();
                let (boundaries, measured_width) =
                    wrap_line(&line_str, wrap_width, line_start_offset);
                fit_width = measured_width;
                match self.wrapping_indent {
                    WrappingIndent::Same => {
                        // Here only have wrapped line, if there is no wrap meet, the `line_wraps`
                        // result will empty.
                        for boundary in boundaries {
                            wrapped_lines.push(prev_boundary_ix..boundary.ix);
                            prev_boundary_ix = boundary.ix;
                            indent_chars = boundary.next_indent;
                        }
                    }
                    WrappingIndent::None => {
                        // The first visual line keeps the line's leading indentation, so it is
                        // wrapped as is.
                        if let Some(first_ix) = boundaries.first().map(|b| b.ix) {
                            wrapped_lines.push(prev_boundary_ix..first_ix);
                            prev_boundary_ix = first_ix;

                            for boundary in wrap_line(
                                &line_str[first_ix..],
                                wrap_width,
                                line_start_offset + first_ix,
                            )
                            .0
                            {
                                let ix = first_ix + boundary.ix;
                                wrapped_lines.push(prev_boundary_ix..ix);
                                prev_boundary_ix = ix;
                            }
                        }
                    }
                }
            }

            // Reset of the line
            if prev_boundary_ix < line.len() || prev_boundary_ix == 0 {
                wrapped_lines.push(prev_boundary_ix..line.len());
            }

            new_lines.push(LineItem {
                len: line.len(),
                fit_width,
                indent: indent_chars,
                wrapped_lines,
            });

            // Advancing through the rope avoids a logarithmic line-to-byte
            // lookup for every row in large documents. `slice_line` excludes
            // LF but keeps CR in CRLF, so each non-final row adds one byte.
            line_start_offset += line.len() + usize::from(row + 1 < changed_line_count);
        }

        if append {
            self.lines.extend(new_lines, &());
        } else if self.lines.is_empty() {
            self.lines = SumTree::from_iter(new_lines, &());
        } else {
            let mut cursor = self.lines.cursor::<BufferRows>(&());
            let mut new_tree = cursor.slice(&BufferRows(start_row), Bias::Right);
            // Skip the replaced rows
            cursor.seek_forward(&BufferRows(end_row + 1), Bias::Right);
            new_tree.extend(new_lines, &());
            // Untouched rows after the edit
            new_tree.append(cursor.suffix(), &());
            drop(cursor);
            self.lines = new_tree;
        }

        self.text = changed_text.clone();
        self.revision = self.revision.wrapping_add(1);
    }

    fn update_all(&mut self, text: &Rope, cx: &mut App) {
        self.update(text, &(0..text.len()), &text, cx);
    }

    pub(crate) fn offset_to_display_point(&self, offset: usize) -> WrapDisplayPoint {
        self.offset_to_display_point_with_affinity(offset, false)
    }

    pub(crate) fn offset_to_display_point_with_affinity(
        &self,
        offset: usize,
        line_end_affinity: bool,
    ) -> WrapDisplayPoint {
        let row = self.text.offset_to_point(offset).row;
        let start = self.text.line_start_offset(row);

        // Seek to buffer row
        let mut cursor = self.lines.cursor::<Dimensions<BufferRows, WrapRows>>(&());
        cursor.seek(&BufferRows(row), Bias::Right);
        let wrapped_row = cursor.start().1.0;
        let Some(line) = cursor.item() else {
            return WrapDisplayPoint::new(wrapped_row, 0, 0);
        };

        let local_offset = offset.saturating_sub(start);
        for (ix, range) in line.wrapped_lines.iter().enumerate() {
            // With affinity the boundary offset closes the current row instead of opening the
            // next one, so the range is matched inclusively.
            let matches =
                range.contains(&local_offset) || (line_end_affinity && local_offset == range.end);
            if matches {
                return WrapDisplayPoint::new(
                    wrapped_row + ix,
                    ix,
                    local_offset.saturating_sub(range.start),
                );
            }
        }

        // Otherwise return the eof of the line.
        let last_range = line.wrapped_lines.last().unwrap_or(&(0..0));
        let ix = line.lines_len().saturating_sub(1);
        return WrapDisplayPoint::new(wrapped_row + ix, ix, last_range.len());
    }

    pub(crate) fn display_point_to_offset(&self, point: WrapDisplayPoint) -> usize {
        // Seek to wrap row `point.row`
        let mut cursor = self.lines.cursor::<Dimensions<WrapRows, BufferRows>>(&());
        cursor.seek(&WrapRows(point.row), Bias::Right);
        let Some(line) = cursor.item() else {
            return self.text.len();
        };
        let wrapped_row = cursor.start().0.0;
        let row = cursor.start().1.0;

        let line_start = self.text.line_start_offset(row);
        let local_row = point.row.saturating_sub(wrapped_row);
        if let Some(range) = line.wrapped_lines.get(local_row) {
            line_start + (range.start + point.column).min(range.end)
        } else {
            // If not found, return the end of the line.
            line_start + line.len()
        }
    }

    pub(crate) fn display_point_to_point(&self, point: WrapDisplayPoint) -> TreeSitterPoint {
        let offset = self.display_point_to_offset(point);
        self.text.offset_to_point(offset)
    }

    pub(crate) fn point_to_display_point(&self, point: TreeSitterPoint) -> WrapDisplayPoint {
        let offset = self.text.point_to_offset(point);
        self.offset_to_display_point(offset)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WrapDisplayPoint {
    pub row: usize,

    pub local_row: usize,

    pub column: usize,
}

impl WrapDisplayPoint {
    pub(crate) fn new(row: usize, local_row: usize, column: usize) -> Self {
        Self {
            row,
            local_row,
            column,
        }
    }
}

pub(crate) struct LineLayout {
    len: usize,

    pub(crate) wrapped_lines: SmallVec<[InputLine; 1]>,

    pub(crate) wrap_indent: Pixels,
    pub(crate) longest_width: Pixels,
    pub(crate) whitespace_indicators: Option<WhitespaceIndicators>,

    pub(crate) whitespace_chars: Vec<(usize, Pixels, bool)>,

    has_background: bool,
}

impl LineLayout {
    pub(crate) fn new() -> Self {
        Self {
            len: 0,
            longest_width: px(0.),
            wrapped_lines: SmallVec::new(),
            wrap_indent: px(0.),
            whitespace_chars: Vec::new(),
            whitespace_indicators: None,
            has_background: false,
        }
    }

    pub(crate) fn with_background(mut self, has_background: bool) -> Self {
        self.has_background = has_background;
        self
    }

    pub(crate) fn wrap_indent(mut self, wrap_indent: Pixels) -> Self {
        self.wrap_indent = wrap_indent;
        self
    }

    #[inline]
    fn line_indent(&self, line_index: usize) -> Pixels {
        if line_index == 0 {
            px(0.)
        } else {
            self.wrap_indent
        }
    }

    pub(crate) fn lines(mut self, wrapped_lines: SmallVec<[ShapedLine; 1]>) -> Self {
        self.set_wrapped_lines(wrapped_lines);
        self
    }

    pub(crate) fn set_wrapped_lines(&mut self, wrapped_lines: SmallVec<[ShapedLine; 1]>) {
        self.len = wrapped_lines.iter().map(|l| l.len).sum();
        let width = wrapped_lines
            .iter()
            .map(|l| l.width)
            .max()
            .unwrap_or_default();
        self.longest_width = width;
        self.wrapped_lines = wrapped_lines.into_iter().map(InputLine::from).collect();
    }

    pub(crate) fn inline_lines(mut self, lines: SmallVec<[InputLine; 1]>) -> Self {
        self.len = lines.iter().map(|line| line.len).sum();
        self.longest_width = lines
            .iter()
            .map(|line| line.width)
            .max()
            .unwrap_or_default();
        self.wrapped_lines = lines;
        self
    }

    pub(crate) fn with_whitespaces(mut self, indicators: Option<WhitespaceIndicators>) -> Self {
        self.whitespace_indicators = indicators;
        let Some(indicators) = self.whitespace_indicators.as_ref() else {
            return self;
        };

        let space_indicator_offset = indicators.space.width.half();

        for (line_index, wrapped_line) in self.wrapped_lines.iter().enumerate() {
            for (relative_offset, c) in wrapped_line.text.char_indices() {
                if matches!(c, ' ' | '\t') {
                    let is_tab = c == '\t';
                    let start_x = wrapped_line.x_for_index(relative_offset);
                    let end_x = wrapped_line.x_for_index(relative_offset + c.len_utf8());
                    // Center the indicator in the actual character's space
                    let x_position = if c == ' ' {
                        (start_x + end_x).half() - space_indicator_offset
                    } else {
                        start_x
                    };

                    self.whitespace_chars.push((line_index, x_position, is_tab));
                }
            }
        }
        self
    }

    #[inline]
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn position_for_index(
        &self,
        offset: usize,
        last_layout: &LastLayout,
        line_end_affinity: bool,
    ) -> Option<Point<Pixels>> {
        let mut acc_len = 0;
        let mut offset_y = px(0.);

        let x_offset = last_layout.alignment_offset(self.longest_width);

        for (i, line) in self.wrapped_lines.iter().enumerate() {
            let is_last = i + 1 == self.wrapped_lines.len();

            let matches = if line.len == 0 {
                // Empty visual lines still own their boundary offset.
                offset == acc_len
            } else if is_last || line_end_affinity {
                // Inclusive: cursor can sit at end of this visual line.
                offset >= acc_len && offset <= acc_len + line.len
            } else {
                // Exclusive: boundary offset belongs to the next visual line.
                offset >= acc_len && offset < acc_len + line.len
            };

            if matches {
                let x = line.x_for_index(offset.saturating_sub(acc_len))
                    + x_offset
                    + self.line_indent(i);
                return Some(point(x, offset_y));
            }

            // Always advance by actual line length. The last line gets +1 so the
            // cursor can be placed after the final character.
            acc_len += if is_last { line.len + 1 } else { line.len };
            offset_y += last_layout.line_height;
        }

        None
    }

    pub(crate) fn closest_index_for_x(&self, x: Pixels, last_layout: &LastLayout) -> usize {
        let mut acc_len = 0;
        let x_offset = last_layout.alignment_offset(self.longest_width);
        let x = x - x_offset;

        for (i, line) in self.wrapped_lines.iter().enumerate() {
            let line_indent = self.line_indent(i);
            if x <= line_indent + line.width {
                return acc_len + line.closest_index_for_x(x - line_indent);
            }
            acc_len += line.len;
        }

        acc_len
    }

    fn wrapped_line_at(
        &self,
        pos: Point<Pixels>,
        last_layout: &LastLayout,
    ) -> Option<(usize, usize, Pixels)> {
        let mut offset = 0;
        let mut line_top = px(0.);
        let x_offset = last_layout.alignment_offset(self.longest_width);

        for (i, line) in self.wrapped_lines.iter().enumerate() {
            let line_bottom = line_top + last_layout.line_height;
            if pos.y >= line_top && pos.y < line_bottom {
                return Some((i, offset, pos.x - x_offset - self.line_indent(i)));
            }

            offset += line.len;
            line_top = line_bottom;
        }

        None
    }

    pub(crate) fn closest_index_for_position(
        &self,
        pos: Point<Pixels>,
        last_layout: &LastLayout,
    ) -> Option<(usize, bool)> {
        let (i, offset, x) = self.wrapped_line_at(pos, last_layout)?;
        let line = &self.wrapped_lines[i];
        let ix = line.closest_index_for_x(x);
        let line_end_affinity = i + 1 < self.wrapped_lines.len() && ix == line.len;

        Some((offset + ix, line_end_affinity))
    }

    pub(crate) fn columns_past_line_end(
        &self,
        pos: Point<Pixels>,
        last_layout: &LastLayout,
    ) -> usize {
        let Some((i, _, x)) = self.wrapped_line_at(pos, last_layout) else {
            return 0;
        };

        if i + 1 < self.wrapped_lines.len() || last_layout.space_width <= px(0.) {
            return 0;
        }

        let past_end = x - self.wrapped_lines[i].width;
        if past_end <= px(0.) {
            return 0;
        }

        (past_end / last_layout.space_width).round() as usize
    }

    pub(crate) fn index_for_position(
        &self,
        pos: Point<Pixels>,
        last_layout: &LastLayout,
    ) -> Option<usize> {
        let (i, offset, x) = self.wrapped_line_at(pos, last_layout)?;

        Some(offset + self.wrapped_lines[i].index_for_x(x)?)
    }

    pub(crate) fn size(&self, line_height: Pixels) -> Size<Pixels> {
        let width = self
            .wrapped_lines
            .iter()
            .enumerate()
            .map(|(ix, line)| line.width + self.line_indent(ix))
            .max()
            .unwrap_or(self.longest_width);
        size(width, self.wrapped_lines.len() * line_height)
    }

    pub(crate) fn paint_background(
        &self,
        pos: Point<Pixels>,
        line_height: Pixels,
        text_align: TextAlign,
        align_width: Option<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) {
        // Painting a background walks every glyph and pushes a scene layer, so skip the
        // whole pass for lines that have no background color to paint.
        if !self.has_background {
            return;
        }

        for (ix, line) in self.wrapped_lines.iter().enumerate() {
            _ = line.paint_background(
                pos + point(self.line_indent(ix), ix * line_height),
                line_height,
                text_align,
                align_width,
                window,
                cx,
            );
        }
    }

    pub(crate) fn paint(
        &self,
        pos: Point<Pixels>,
        line_height: Pixels,
        text_align: TextAlign,
        align_width: Option<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) {
        for (ix, line) in self.wrapped_lines.iter().enumerate() {
            _ = line.paint(
                pos + point(self.line_indent(ix), ix * line_height),
                line_height,
                text_align,
                align_width,
                window,
                cx,
            );
        }

        // Paint whitespace indicators
        if let Some(indicators) = self.whitespace_indicators.as_ref() {
            for (line_index, x_position, is_tab) in &self.whitespace_chars {
                let invisible = if *is_tab {
                    indicators.tab.clone()
                } else {
                    indicators.space.clone()
                };

                let origin = point(
                    pos.x + *x_position + self.line_indent(*line_index),
                    pos.y + *line_index as f32 * line_height,
                );

                _ = invisible.paint(origin, line_height, text_align, align_width, window, cx);
            }
        }
    }
}
