use std::{collections::BTreeSet, ops::Range};
use tree_sitter::{InputEdit, Query, QueryCursor, StreamingIterator, Tree};

use crate::{
    SyntaxResult,
    syntax_data::{DiagnosticData, SEMANTIC_NAMES},
};

pub(super) struct DocumentParser {
    parser: tree_sitter::Parser,
    query: Query,
    tree: Option<Tree>,
}

impl DocumentParser {
    pub(super) fn new() -> Result<Self, String> {
        let grammar = tree_sitter_json5_orchard::LANGUAGE.into();
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&grammar)
            .map_err(|error| error.to_string())?;
        let query = Query::new(&grammar, tree_sitter_json5_orchard::HIGHLIGHTS_QUERY)
            .map_err(|error| error.to_string())?;
        Ok(Self {
            parser,
            query,
            tree: None,
        })
    }

    pub(super) fn reset(&mut self) {
        self.tree = None;
    }

    pub(super) fn convert_to_json(
        &mut self,
        source: &str,
        view: &str,
        compact: bool,
    ) -> Result<String, String> {
        if source == view
            && let Some(tree) = &self.tree
        {
            return crate::json_conversion::convert(tree, source, compact);
        }
        // The editor tree uses display offsets. Validate the original source
        // when generated formatting differs, without replacing the editor tree.
        let tree = self
            .parser
            .parse(source, None)
            .ok_or("JSON5 parsing was cancelled")?;
        crate::json_conversion::convert(&tree, source, compact)
    }

    pub(super) fn edit(&mut self, edit: &InputEdit) {
        if let Some(tree) = &mut self.tree {
            tree.edit(edit);
        }
    }

    pub(super) fn analyze(&mut self, source: &str) -> SyntaxResult {
        self.tree = self.parser.parse(source, self.tree.as_ref());
        let (folds, pairs, strings) = json5_structure(source);
        let mut captures = Vec::new();
        let mut escapes = Vec::new();
        let mut diagnostics = Vec::new();
        for range in strings {
            escapes.extend(scan_string_escapes(source, range.clone()));
            captures.push((range, 2));
        }
        escapes.sort_by_key(|(range, valid)| (range.start, range.end, *valid));
        escapes.dedup();
        // A rejected escape can make Tree-sitter recover the entire document
        // as one ERROR node. Repair only those known lexical errors in a
        // temporary parse, preserving byte offsets and newlines, so unrelated
        // syntax errors still get their own diagnostics. Keep the original
        // tree for incremental edits and never change the saved source.
        let recovered_source = self
            .tree
            .as_ref()
            .filter(|tree| tree.root_node().has_error())
            .and_then(|_| recover_invalid_escapes(source, &escapes));
        let recovered_tree = recovered_source
            .as_ref()
            .and_then(|source| self.parser.parse(source, None));
        let parse_source = recovered_source
            .as_deref()
            .filter(|_| recovered_tree.is_some())
            .unwrap_or(source.as_bytes());
        for (range, valid) in &escapes {
            if *valid {
                captures.push((range.clone(), 9));
            } else {
                diagnostics.push(DiagnosticData {
                    start: range.start,
                    end: range.end,
                    message: "Invalid JSON5 string escape".into(),
                    code: "json5/invalid-escape".into(),
                });
            }
        }
        if let Some(tree) = recovered_tree.as_ref().or(self.tree.as_ref()) {
            let names = self.query.capture_names();
            let mut cursor = QueryCursor::new();
            let mut matches = cursor.matches(&self.query, tree.root_node(), parse_source);
            while let Some(query_match) = matches.next() {
                for capture in query_match.captures() {
                    let name = names[capture.index as usize];
                    let range = capture.node.byte_range();
                    if range.is_empty() {
                        continue;
                    }
                    captures.push((range, semantic_id(name)));
                }
            }
            let mut pending = vec![tree.root_node()];
            while let Some(node) = pending.pop() {
                if !node.has_error() {
                    continue;
                }
                if node.is_error() || node.is_missing() {
                    let mut start = node.start_byte().min(source.len());
                    let mut end = node.end_byte().min(source.len());
                    if start == end {
                        if let Some(ch) = source[end..].chars().next() {
                            end += ch.len_utf8();
                        } else if let Some(ch) = source[..start].chars().next_back() {
                            start -= ch.len_utf8();
                        }
                    }
                    diagnostics.push(DiagnosticData {
                        start,
                        end,
                        message: if node.is_missing() {
                            format!("Expected `{}`", node.kind())
                        } else {
                            "Invalid JSON5 syntax".into()
                        },
                        code: "json5/syntax".into(),
                    });
                    continue;
                }
                for index in (0..node.child_count()).rev() {
                    if let Some(child) = node.child(index)
                        && child.has_error()
                    {
                        pending.push(child);
                    }
                }
            }
        }
        let mut endpoints = Vec::with_capacity(pairs.len() * 2);
        for (open, close) in pairs {
            endpoints.push((open, close));
            endpoints.push((close, open));
        }
        endpoints.sort_unstable();
        SyntaxResult {
            highlights: flatten_highlights(captures),
            folds: folds.into_iter().flatten().collect(),
            brackets: endpoints
                .into_iter()
                .flat_map(|(at, matching)| [at as u32, matching as u32])
                .collect(),
            diagnostics: serde_json::to_string(&diagnostics).expect("serializable diagnostics"),
        }
    }
}

fn semantic_id(capture: &str) -> u32 {
    let name = match capture {
        "constant.numeric" => "number",
        "constant.builtin.boolean" => "boolean",
        "constant.builtin" => "constant",
        name => name,
    };
    SEMANTIC_NAMES
        .iter()
        .position(|entry| *entry == name)
        .unwrap_or(10) as u32
}

// Resolve overlap once in the worker, rather than searching all captures for
// every visible segment on every rendered frame.
fn flatten_highlights(captures: Vec<(Range<usize>, u32)>) -> Vec<u32> {
    let mut events = Vec::with_capacity(captures.len() * 2);
    for (index, (range, _)) in captures.iter().enumerate() {
        events.push((range.start, true, index));
        events.push((range.end, false, index));
    }
    events.sort_unstable();
    let mut active = BTreeSet::new();
    let mut runs: Vec<u32> = Vec::new();
    let mut previous = 0;
    for (position, start, index) in events {
        if position > previous
            && let Some(&(_, _, semantic, _)) = active.first()
        {
            if runs.len() >= 3
                && runs[runs.len() - 2] == previous as u32
                && runs[runs.len() - 1] == semantic
            {
                let end_index = runs.len() - 2;
                runs[end_index] = position as u32;
            } else {
                runs.extend([previous as u32, position as u32, semantic]);
            }
        }
        let (range, semantic) = &captures[index];
        // Escape spans override their enclosing key or string. A key's base
        // color still takes precedence over the generic string capture.
        let priority = match *semantic {
            9 => 0,
            1 => 1,
            2 => 2,
            _ => 3,
        };
        let key = (priority, range.len(), *semantic, index);
        if start {
            active.insert(key);
        } else {
            active.remove(&key);
        }
        previous = position;
    }
    runs
}

fn scan_string_escapes(source: &str, range: Range<usize>) -> Vec<(Range<usize>, bool)> {
    let node_start = range.start;
    let node_end = range.end.min(source.len());
    if node_start >= node_end {
        return Vec::new();
    }

    let bytes = source.as_bytes();
    let mut start = node_start + usize::from(matches!(bytes[node_start], b'\'' | b'"'));
    let mut end = node_end;
    if end > start && matches!(bytes[end - 1], b'\'' | b'"') && bytes[end - 1] == bytes[node_start]
    {
        end -= 1;
    }

    let mut escapes = Vec::new();
    while start < end {
        if bytes[start] != b'\\' {
            start += 1;
            continue;
        }

        let escape_start = start;
        let Some(&next) = bytes.get(start + 1).filter(|_| start + 1 < end) else {
            escapes.push((escape_start..end, false));
            break;
        };
        start += 2;
        let valid = match next {
            b'\'' | b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' | b'v' => true,
            b'0' => !bytes.get(start).is_some_and(u8::is_ascii_digit),
            b'x' | b'u' => {
                let digits = if next == b'x' { 2 } else { 4 };
                let digits_start = start;
                while start < end
                    && start - digits_start < digits
                    && bytes[start].is_ascii_hexdigit()
                {
                    start += 1;
                }
                // Leave a following backslash for the next iteration rather
                // than swallowing a second invalid escape as a hex digit.
                start - digits_start == digits
            }
            b'\n' => true,
            b'\r' => {
                if bytes.get(start) == Some(&b'\n') && start < end {
                    start += 1;
                }
                true
            }
            _ => {
                // Diagnostic endpoints must remain UTF-8 character boundaries.
                let next = source[escape_start + 1..end].chars().next().unwrap();
                start = escape_start + 1 + next.len_utf8();
                false
            }
        };
        escapes.push((escape_start..start, valid));
    }
    escapes
}

fn recover_invalid_escapes(source: &str, escapes: &[(Range<usize>, bool)]) -> Option<Vec<u8>> {
    let mut recovered = None;
    for (range, valid) in escapes {
        if *valid {
            continue;
        }
        let bytes = recovered.get_or_insert_with(|| source.as_bytes().to_vec());
        if range.start + 1 < range.end {
            let next = source[range.start + 1..range.end].chars().next().unwrap();
            // Replace the escape code with `n` (a valid escape) and pad a
            // non-ASCII code's remaining bytes without changing source offsets.
            bytes[range.start + 1] = b'n';
            bytes[range.start + 2..range.start + 1 + next.len_utf8()].fill(b' ');
        } else {
            bytes[range.start] = b' ';
        }
    }
    recovered
}

fn json5_structure(source: &str) -> (Vec<[u32; 4]>, Vec<(usize, usize)>, Vec<Range<usize>>) {
    let bytes = source.as_bytes();
    let mut index = 0;
    let mut line = 0;
    let mut line_start = 0;
    let mut quote = None;
    let mut quote_start = 0;
    let mut strings = Vec::new();
    let mut escaped = false;
    let mut line_comment = false;
    let mut block_comment = false;
    let mut stack = Vec::<(u8, usize, usize, usize)>::new();
    let mut folds = Vec::new();
    let mut pairs = Vec::new();

    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'\n' {
            line += 1;
            line_start = index + 1;
        }
        if line_comment {
            if byte == b'\n' {
                line_comment = false;
            }
            index += 1;
            continue;
        }
        if block_comment {
            if byte == b'*' && bytes.get(index + 1) == Some(&b'/') {
                block_comment = false;
                index += 2;
            } else {
                index += 1;
            }
            continue;
        }
        if let Some(delimiter) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == delimiter {
                quote = None;
                strings.push(quote_start..index + 1);
            }
            index += 1;
            continue;
        }

        match byte {
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                line_comment = true;
                index += 2;
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                block_comment = true;
                index += 2;
            }
            b'\'' | b'"' => {
                quote = Some(byte);
                quote_start = index;
                index += 1;
            }
            b'{' | b'[' => {
                stack.push((byte, line, index, index - line_start));
                index += 1;
            }
            b'}' | b']' => {
                let expected = if byte == b'}' { b'{' } else { b'[' };
                if stack
                    .last()
                    .is_some_and(|(open, _, _, _)| *open == expected)
                    && let Some((_, start_line, start, start_column)) = stack.pop()
                {
                    pairs.push((start, index));
                    if line > start_line {
                        folds.push([
                            start_line as u32,
                            line as u32,
                            start_column as u32,
                            (index - line_start + 1) as u32,
                        ]);
                    }
                }
                index += 1;
            }
            _ => index += 1,
        }
    }

    folds.sort_unstable();
    pairs.sort_unstable();
    if quote.is_some() {
        strings.push(quote_start..source.len());
    }
    (folds, pairs, strings)
}
