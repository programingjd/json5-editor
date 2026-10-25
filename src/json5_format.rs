pub(super) struct SourceProjection {
    source: String,
    view: String,
    /// Maps every byte boundary in `view` to a byte boundary in `source`.
    source_offsets: Vec<usize>,
}

impl SourceProjection {
    pub(super) fn new(source: &str, width: usize) -> Self {
        let (view, source_offsets) = auto_indent_with_map(source, width);
        Self {
            source: source.to_owned(),
            view,
            source_offsets,
        }
    }

    pub(super) fn view(&self) -> &str {
        &self.view
    }

    pub(super) fn source(&self) -> &str {
        &self.source
    }

    /// Apply one editor-buffer edit to the corresponding range in the source.
    pub(super) fn apply_edit(&mut self, view_range: std::ops::Range<usize>, replacement: &str) {
        let source_start = self.source_offsets[view_range.start];
        let source_end = self.source_offsets[view_range.end];
        self.source
            .replace_range(source_start..source_end, replacement);

        let source_delta = replacement.len() as isize - (source_end - source_start) as isize;
        let mut source_offsets =
            Vec::with_capacity(self.source_offsets.len() - view_range.len() + replacement.len());
        source_offsets.extend_from_slice(&self.source_offsets[..=view_range.start]);
        for byte in 1..=replacement.len() {
            source_offsets.push(source_start + byte);
        }
        let suffix_start = view_range
            .end
            .saturating_add(1)
            .min(self.source_offsets.len());
        source_offsets.extend(self.source_offsets[suffix_start..].iter().map(|offset| {
            if *offset <= source_start {
                *offset
            } else if *offset >= source_end {
                offset
                    .checked_add_signed(source_delta)
                    .unwrap_or(source_start)
            } else {
                source_start + replacement.len()
            }
        }));

        self.view.replace_range(view_range, replacement);
        self.source_offsets = source_offsets;
    }
}

pub(super) fn auto_indent(source: &str, width: usize) -> String {
    auto_indent_with_map(source, width).0
}

fn auto_indent_with_map(source: &str, width: usize) -> (String, Vec<usize>) {
    let characters: Vec<(usize, char)> = source.char_indices().collect();
    let mut output = String::with_capacity(source.len() + source.len() / 4);
    let mut source_offsets = Vec::with_capacity(output.capacity() + 1);
    source_offsets.push(0);
    let mut depth = 0usize;
    let mut ix = 0usize;
    let mut quote = None;
    let mut escaped = false;
    let mut line_comment = false;
    let mut block_comment = false;

    while ix < characters.len() {
        let (source_start, character) = characters[ix];
        let next = characters.get(ix + 1).map(|(_, character)| *character);
        let source_end = source_start + character.len_utf8();

        if line_comment {
            if character == '\n' {
                trim_trailing_space(&mut output, &mut source_offsets);
                append_source(
                    source,
                    source_start..source_end,
                    &mut output,
                    &mut source_offsets,
                );
                push_indent(&mut output, &mut source_offsets, depth, width, source_end);
                line_comment = false;
            } else {
                append_source(
                    source,
                    source_start..source_end,
                    &mut output,
                    &mut source_offsets,
                );
            }
        } else if block_comment {
            append_source(
                source,
                source_start..source_end,
                &mut output,
                &mut source_offsets,
            );
            if character == '*' && next == Some('/') {
                let (next_start, next_char) = characters[ix + 1];
                append_source(
                    source,
                    next_start..next_start + next_char.len_utf8(),
                    &mut output,
                    &mut source_offsets,
                );
                ix += 1;
                block_comment = false;
            } else if character == '\n' {
                push_indent(&mut output, &mut source_offsets, depth, width, source_end);
            }
        } else if let Some(delimiter) = quote {
            append_source(
                source,
                source_start..source_end,
                &mut output,
                &mut source_offsets,
            );
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == delimiter {
                quote = None;
            }
        } else if character == '/' && next == Some('/') {
            trim_trailing_space(&mut output, &mut source_offsets);
            if !output.ends_with('\n') && !output.is_empty() {
                push_virtual(&mut output, &mut source_offsets, ' ', source_start);
            }
            append_source(
                source,
                source_start..source_start + 2,
                &mut output,
                &mut source_offsets,
            );
            ix += 1;
            line_comment = true;
        } else if character == '/' && next == Some('*') {
            trim_trailing_space(&mut output, &mut source_offsets);
            if !output.ends_with('\n') && !output.is_empty() {
                push_virtual(&mut output, &mut source_offsets, ' ', source_start);
            }
            append_source(
                source,
                source_start..source_start + 2,
                &mut output,
                &mut source_offsets,
            );
            ix += 1;
            block_comment = true;
        } else if character == '"' || character == '\'' {
            append_source(
                source,
                source_start..source_end,
                &mut output,
                &mut source_offsets,
            );
            quote = Some(character);
        } else if character.is_whitespace() {
            // Whitespace outside strings and comments is supplied by the
            // formatting rules below.
        } else if character == '{' || character == '[' {
            append_source(
                source,
                source_start..source_end,
                &mut output,
                &mut source_offsets,
            );
            depth += 1;
            if next_non_whitespace(&characters, ix + 1)
                == Some(if character == '{' { '}' } else { ']' })
            {
                // Keep empty containers compact.
            } else {
                push_virtual(&mut output, &mut source_offsets, '\n', source_end);
                push_indent(&mut output, &mut source_offsets, depth, width, source_end);
            }
        } else if character == '}' || character == ']' {
            depth = depth.saturating_sub(1);
            trim_trailing_space(&mut output, &mut source_offsets);
            if !output.ends_with('\n')
                && !output.is_empty()
                && !matches!(output.chars().last(), Some('{' | '['))
            {
                push_virtual(&mut output, &mut source_offsets, '\n', source_start);
            }
            if output.ends_with('\n') {
                push_indent(&mut output, &mut source_offsets, depth, width, source_start);
            }
            append_source(
                source,
                source_start..source_end,
                &mut output,
                &mut source_offsets,
            );
        } else if character == ',' {
            trim_trailing_space(&mut output, &mut source_offsets);
            append_source(
                source,
                source_start..source_end,
                &mut output,
                &mut source_offsets,
            );
            if next_non_whitespace(&characters, ix + 1) == Some('/') {
                push_virtual(&mut output, &mut source_offsets, ' ', source_end);
            } else {
                push_virtual(&mut output, &mut source_offsets, '\n', source_end);
                push_indent(&mut output, &mut source_offsets, depth, width, source_end);
            }
        } else if character == ':' {
            trim_trailing_space(&mut output, &mut source_offsets);
            append_source(
                source,
                source_start..source_end,
                &mut output,
                &mut source_offsets,
            );
            push_virtual(&mut output, &mut source_offsets, ' ', source_end);
        } else {
            if ix > 0
                && characters[ix - 1].1.is_whitespace()
                && output
                    .chars()
                    .last()
                    .is_some_and(|previous| is_word(previous) && is_word(character))
            {
                push_virtual(&mut output, &mut source_offsets, ' ', source_start);
            }
            append_source(
                source,
                source_start..source_end,
                &mut output,
                &mut source_offsets,
            );
        }

        ix += 1;
    }

    trim_trailing_space(&mut output, &mut source_offsets);
    (output, source_offsets)
}

fn append_source(
    source: &str,
    range: std::ops::Range<usize>,
    output: &mut String,
    source_offsets: &mut Vec<usize>,
) {
    source_offsets[output.len()] = range.start;
    output.push_str(&source[range.clone()]);
    source_offsets.extend(range.start + 1..=range.end);
}

fn push_virtual(
    output: &mut String,
    source_offsets: &mut Vec<usize>,
    character: char,
    source_anchor: usize,
) {
    source_offsets[output.len()] = source_anchor;
    output.push(character);
    source_offsets.extend(std::iter::repeat_n(source_anchor, character.len_utf8()));
}

pub(super) fn compact(source: &str) -> String {
    let characters: Vec<char> = source.chars().collect();
    let mut output = String::with_capacity(source.len());
    let mut ix = 0usize;
    let mut quote = None;
    let mut escaped = false;
    let mut line_comment = false;
    let mut block_comment = false;

    while ix < characters.len() {
        let character = characters[ix];
        let next = characters.get(ix + 1).copied();

        if line_comment {
            if character == '\n' {
                output.push('\n');
                line_comment = false;
            } else {
                output.push(character);
            }
        } else if block_comment {
            output.push(character);
            if character == '*' && next == Some('/') {
                output.push('/');
                ix += 1;
                block_comment = false;
            }
        } else if let Some(delimiter) = quote {
            output.push(character);
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == delimiter {
                quote = None;
            }
        } else if character == '/' && next == Some('/') {
            output.push_str("//");
            ix += 1;
            line_comment = true;
        } else if character == '/' && next == Some('*') {
            output.push_str("/*");
            ix += 1;
            block_comment = true;
        } else if character == '"' || character == '\'' {
            output.push(character);
            quote = Some(character);
        } else if character.is_whitespace() {
            if characters.get(ix + 1).copied().is_some_and(is_word)
                && output.chars().last().is_some_and(is_word)
                && !output.ends_with(' ')
            {
                output.push(' ');
            }
        } else {
            output.push(character);
        }
        ix += 1;
    }

    output.trim().to_owned()
}

#[allow(dead_code)] // The gallery editor does not have a file-save action yet.
pub(super) fn format_for_save(source: &str, strip_indentation: bool) -> String {
    if strip_indentation {
        compact(source)
    } else {
        source.to_owned()
    }
}

fn next_non_whitespace(characters: &[(usize, char)], start: usize) -> Option<char> {
    characters[start..]
        .iter()
        .map(|(_, character)| *character)
        .find(|character| !character.is_whitespace())
}

fn push_indent(
    output: &mut String,
    source_offsets: &mut Vec<usize>,
    depth: usize,
    width: usize,
    source_anchor: usize,
) {
    for _ in 0..depth.saturating_mul(width) {
        push_virtual(output, source_offsets, ' ', source_anchor);
    }
}

fn trim_trailing_space(output: &mut String, source_offsets: &mut Vec<usize>) {
    while output.ends_with([' ', '\t']) {
        output.pop();
        source_offsets.pop();
    }
}

fn is_word(character: char) -> bool {
    character.is_alphanumeric() || matches!(character, '_' | '$')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_one_line_json5_without_touching_strings_or_comments() {
        let formatted = auto_indent(
            r#"{key:'a,b', nested:[1,2,], // comment
flag:true}"#,
            2,
        );

        assert_eq!(
            formatted,
            "{\n  key: 'a,b',\n  nested: [\n    1,\n    2,\n  ], // comment\n  flag: true\n}"
        );
    }

    #[test]
    fn compact_preserves_required_token_separation_and_comments() {
        assert_eq!(
            compact("{ key: true, value: 'a b' }"),
            "{key:true,value:'a b'}"
        );
        assert_eq!(
            compact("{ key: true // note\n, next: false }"),
            "{key:true// note\n,next:false}"
        );
    }

    #[test]
    fn save_format_can_preserve_or_strip_indentation() {
        let source = "{\n  key: true\n}";
        assert_eq!(format_for_save(source, false), source);
        assert_eq!(format_for_save(source, true), "{key:true}");
    }
}
