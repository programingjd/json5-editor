use std::ops::Range;

use ropey::{LineType, Rope, RopeSlice};
use sum_tree::Bias;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Point {
    pub row: usize,
    pub column: usize,
}

impl Point {
    pub fn new(row: usize, column: usize) -> Self {
        Self { row, column }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct InputEdit {
    pub start_byte: usize,
    pub old_end_byte: usize,
    pub new_end_byte: usize,
    pub start_position: Point,
    pub old_end_position: Point,
    pub new_end_position: Point,
}

use lsp_types::Position;

pub struct RopeLines<'a> {
    rope: &'a Rope,
    row: usize,
    end_row: usize,
}

impl<'a> RopeLines<'a> {
    pub fn new(rope: &'a Rope) -> Self {
        let end_row = rope.lines_len();
        Self {
            row: 0,
            end_row,
            rope,
        }
    }
}
impl<'a> Iterator for RopeLines<'a> {
    type Item = RopeSlice<'a>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.row >= self.end_row {
            return None;
        }

        let line = self.rope.slice_line(self.row);
        self.row += 1;
        Some(line)
    }

    #[inline]
    fn nth(&mut self, n: usize) -> Option<Self::Item> {
        self.row = self.row.saturating_add(n);
        self.next()
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.end_row - self.row;
        (len, Some(len))
    }
}

impl std::iter::ExactSizeIterator for RopeLines<'_> {}
impl std::iter::FusedIterator for RopeLines<'_> {}

pub trait RopeExt {
    fn line_start_offset(&self, row: usize) -> usize;

    fn line_end_offset(&self, row: usize) -> usize;

    fn slice_line(&self, row: usize) -> RopeSlice<'_>;

    fn slice_lines(&self, rows_range: Range<usize>) -> RopeSlice<'_>;

    fn iter_lines(&self) -> RopeLines<'_>;

    fn lines_len(&self) -> usize;

    fn line_len(&self, row: usize) -> usize;

    fn replace(&mut self, range: Range<usize>, new_text: &str);

    fn char_at(&self, offset: usize) -> Option<char>;

    fn position_to_offset(&self, line_col: &Position) -> usize;

    fn offset_to_position(&self, offset: usize) -> Position;

    fn offset_to_point(&self, offset: usize) -> Point;

    fn point_to_offset(&self, point: Point) -> usize;

    fn word_range(&self, offset: usize) -> Option<Range<usize>>;

    fn word_at(&self, offset: usize) -> String;

    fn offset_utf16_to_offset(&self, offset_utf16: usize) -> usize;

    fn offset_to_offset_utf16(&self, offset: usize) -> usize;

    fn clip_offset(&self, offset: usize, bias: Bias) -> usize;

    fn char_index_to_offset(&self, char_index: usize) -> usize;

    fn offset_to_char_index(&self, offset: usize) -> usize;
}

impl RopeExt for Rope {
    fn slice_line(&self, row: usize) -> RopeSlice<'_> {
        let total_lines = self.lines_len();
        if row >= total_lines {
            return self.slice(0..0);
        }

        let line = self.line(row, LineType::LF);
        if line.len() > 0 {
            let line_end = line.len() - 1;
            if line.is_char_boundary(line_end) && line.char(line_end) == '\n' {
                return line.slice(..line_end);
            }
        }

        line
    }

    fn slice_lines(&self, rows_range: Range<usize>) -> RopeSlice<'_> {
        let start = self.line_start_offset(rows_range.start);
        let end = self.line_end_offset(rows_range.end.saturating_sub(1));
        self.slice(start..end)
    }

    fn iter_lines(&self) -> RopeLines<'_> {
        RopeLines::new(&self)
    }

    fn line_len(&self, row: usize) -> usize {
        self.slice_line(row).len()
    }

    fn line_start_offset(&self, row: usize) -> usize {
        self.point_to_offset(Point::new(row, 0))
    }

    fn offset_to_point(&self, offset: usize) -> Point {
        let offset = self.clip_offset(offset, Bias::Left);
        let row = self.byte_to_line_idx(offset, LineType::LF);
        let line_start = self.line_to_byte_idx(row, LineType::LF);
        let column = offset.saturating_sub(line_start);
        Point::new(row, column)
    }

    fn point_to_offset(&self, point: Point) -> usize {
        if point.row >= self.lines_len() {
            return self.len();
        }

        let line_start = self.line_to_byte_idx(point.row, LineType::LF);
        line_start + point.column
    }

    fn position_to_offset(&self, pos: &Position) -> usize {
        let line = self.slice_line(pos.line as usize);
        // Clamp out-of-range columns, then use Ropey's index to avoid rescanning long lines.
        self.line_start_offset(pos.line as usize)
            + line.char_to_byte_idx((pos.character as usize).min(line.len_chars()))
    }

    fn offset_to_position(&self, offset: usize) -> Position {
        let point = self.offset_to_point(offset);
        let line = self.slice_line(point.row);
        let offset = line.utf16_to_byte_idx(line.byte_to_utf16_idx(point.column));
        let character = line.slice(..offset).chars().count();
        Position::new(point.row as u32, character as u32)
    }

    fn line_end_offset(&self, row: usize) -> usize {
        if row > self.lines_len() {
            return self.len();
        }

        self.line_start_offset(row) + self.line_len(row)
    }

    fn lines_len(&self) -> usize {
        self.len_lines(LineType::LF)
    }

    fn char_at(&self, offset: usize) -> Option<char> {
        if offset > self.len() {
            return None;
        }

        self.get_char(offset).ok()
    }

    fn word_range(&self, offset: usize) -> Option<Range<usize>> {
        if offset >= self.len() {
            return None;
        }

        let mut left = String::new();
        let offset = self.clip_offset(offset, Bias::Left);
        for c in self.chars_at(offset).reversed() {
            if c.is_alphanumeric() || c == '_' {
                left.insert(0, c);
            } else {
                break;
            }
        }
        let start = offset.saturating_sub(left.len());

        let right = self
            .chars_at(offset)
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect::<String>();

        let end = offset + right.len();

        if start == end { None } else { Some(start..end) }
    }

    fn word_at(&self, offset: usize) -> String {
        if let Some(range) = self.word_range(offset) {
            self.slice(range).to_string()
        } else {
            String::new()
        }
    }

    #[inline]
    fn offset_utf16_to_offset(&self, offset_utf16: usize) -> usize {
        if offset_utf16 > self.len_utf16() {
            return self.len();
        }

        self.utf16_to_byte_idx(offset_utf16)
    }

    #[inline]
    fn offset_to_offset_utf16(&self, offset: usize) -> usize {
        if offset > self.len() {
            return self.len_utf16();
        }

        self.byte_to_utf16_idx(offset)
    }

    fn replace(&mut self, range: Range<usize>, new_text: &str) {
        let range =
            self.clip_offset(range.start, Bias::Left)..self.clip_offset(range.end, Bias::Right);
        self.remove(range.clone());
        self.insert(range.start, new_text);
    }

    fn clip_offset(&self, offset: usize, bias: Bias) -> usize {
        if offset > self.len() {
            return self.len();
        }

        if self.is_char_boundary(offset) {
            return offset;
        }

        if bias == Bias::Left {
            self.floor_char_boundary(offset)
        } else {
            self.ceil_char_boundary(offset)
        }
    }

    fn char_index_to_offset(&self, char_offset: usize) -> usize {
        self.chars().take(char_offset).map(|c| c.len_utf8()).sum()
    }

    fn offset_to_char_index(&self, offset: usize) -> usize {
        let offset = self.clip_offset(offset, Bias::Right);
        self.slice(..offset).chars().count()
    }
}
