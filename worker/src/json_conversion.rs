//! Strict JSON output from the existing JSON5 concrete syntax tree.
use std::fmt::Write as _;

use tree_sitter::{Node, Tree};

pub(super) fn convert(tree: &Tree, source: &str, compact: bool) -> Result<String, String> {
    let root = tree.root_node();
    if root.has_error() {
        return Err("Cannot convert invalid JSON5 syntax".into());
    }
    let mut output = String::with_capacity(source.len());
    let mut cursor = tree.walk();
    let mut depth = 0usize;
    let mut previous = None;

    // Walk iteratively so deeply nested input does not recurse on the WASM stack.
    'visit: loop {
        let node = cursor.node();
        match node.kind() {
            "file" | "object" | "array" | "member" => {
                if cursor.goto_first_child() {
                    continue;
                }
            }
            "comment" => {}
            kind => {
                if kind == "," && is_trailing_comma(node) {
                    // JSON5 permits a comma immediately before a container closes.
                } else {
                    if !compact && matches!(previous, Some('{' | '[')) && !matches!(kind, "}" | "]")
                    {
                        newline(&mut output, depth);
                    }
                    match kind {
                        "{" | "[" => {
                            output.push(kind.as_bytes()[0] as char);
                            depth += 1;
                        }
                        "}" | "]" => {
                            depth = depth.saturating_sub(1);
                            if !compact && !matches!(previous, Some('{' | '[')) {
                                newline(&mut output, depth);
                            }
                            output.push(kind.as_bytes()[0] as char);
                        }
                        "," => {
                            output.push(',');
                            if !compact {
                                newline(&mut output, depth);
                            }
                        }
                        ":" => output.push_str(if compact { ":" } else { ": " }),
                        "true" | "false" | "null" => output.push_str(kind),
                        "string" | "identifier" => {
                            write_string(
                                node.utf8_text(source.as_bytes())
                                    .map_err(|error| error.to_string())?,
                                kind == "string",
                                &mut output,
                            )?;
                        }
                        "number" => {
                            write_number(
                                node.utf8_text(source.as_bytes())
                                    .map_err(|error| error.to_string())?,
                                &mut output,
                            )?;
                        }
                        _ => return Err(format!("Cannot convert JSON5 node `{kind}`")),
                    }
                    previous = Some(if kind.len() == 1 {
                        kind.as_bytes()[0] as char
                    } else {
                        'v'
                    });
                }
            }
        }
        if cursor.goto_next_sibling() {
            continue;
        }
        loop {
            if !cursor.goto_parent() {
                break 'visit;
            }
            if cursor.goto_next_sibling() {
                continue 'visit;
            }
        }
    }
    if output.is_empty() {
        return Err("Cannot convert an empty JSON5 document".into());
    }
    Ok(output)
}

fn is_trailing_comma(node: Node<'_>) -> bool {
    let mut next = node.next_sibling();
    while let Some(node) = next {
        if node.kind() != "comment" {
            return matches!(node.kind(), "}" | "]");
        }
        next = node.next_sibling();
    }
    false
}

fn newline(output: &mut String, depth: usize) {
    output.push('\n');
    output.extend(std::iter::repeat_n(' ', depth * 2));
}

fn write_string(token: &str, quoted: bool, output: &mut String) -> Result<(), String> {
    let content = if quoted {
        let delimiter = token
            .as_bytes()
            .first()
            .copied()
            .ok_or("Missing string delimiter")?;
        if !matches!(delimiter, b'\'' | b'"')
            || token.len() < 2
            || token.as_bytes().last() != Some(&delimiter)
        {
            return Err("Unterminated JSON5 string".into());
        }
        &token[1..token.len() - 1]
    } else {
        token
    };
    if !content.contains('\\') {
        output.push_str(&serde_json::to_string(content).map_err(|error| error.to_string())?);
        return Ok(());
    }
    output.push('"');
    let mut characters = content.chars().peekable();
    while let Some(character) = characters.next() {
        if character != '\\' {
            write_character(character, output);
            continue;
        }
        let escaped = characters.next().ok_or("Incomplete JSON5 escape")?;
        if !quoted && escaped != 'u' {
            return Err("Invalid JSON5 identifier escape".into());
        }
        match escaped {
            'u' | 'x' => {
                output.push_str(if escaped == 'x' { "\\u00" } else { "\\u" });
                for _ in 0..if escaped == 'x' { 2 } else { 4 } {
                    let digit = characters
                        .next()
                        .filter(char::is_ascii_hexdigit)
                        .ok_or("Invalid JSON5 hexadecimal escape")?;
                    output.push(digit);
                }
                // Keep UTF-16 escapes encoded, including valid surrogate pairs
                // and lone surrogates that cannot be held in a Rust String.
            }
            '\'' | '"' | '\\' | '/' => write_character(escaped, output),
            'b' | 'f' | 'n' | 'r' | 't' => {
                output.push('\\');
                output.push(escaped);
            }
            'v' => output.push_str("\\u000b"),
            '0' if !characters.peek().is_some_and(char::is_ascii_digit) => {
                output.push_str("\\u0000")
            }
            '\n' | '\u{2028}' | '\u{2029}' => {}
            '\r' => {
                if characters.peek() == Some(&'\n') {
                    characters.next();
                }
            }
            '0'..='9' => return Err("Invalid JSON5 numeric escape".into()),
            _ => write_character(escaped, output),
        }
    }
    output.push('"');
    Ok(())
}

fn write_character(character: char, output: &mut String) {
    match character {
        '"' => output.push_str("\\\""),
        '\\' => output.push_str("\\\\"),
        '\n' => output.push_str("\\n"),
        '\r' => output.push_str("\\r"),
        '\t' => output.push_str("\\t"),
        '\u{8}' => output.push_str("\\b"),
        '\u{c}' => output.push_str("\\f"),
        character if character < ' ' => {
            let _ = write!(output, "\\u{:04x}", character as u32);
        }
        character => output.push(character),
    }
}

fn write_number(token: &str, output: &mut String) -> Result<(), String> {
    let unsigned = token.strip_prefix(['+', '-']).unwrap_or(token);
    if matches!(unsigned, "Infinity" | "NaN") {
        output.push_str("null");
        return Ok(());
    }
    if let Some(hex) = unsigned
        .strip_prefix("0x")
        .or_else(|| unsigned.strip_prefix("0X"))
    {
        // Base 10^9 limbs preserve integer digits without passing through f64.
        let mut decimal = vec![0u32];
        for character in hex.chars() {
            let mut carry = character
                .to_digit(16)
                .ok_or("Invalid JSON5 hexadecimal number")? as u64;
            for limb in &mut decimal {
                let value = *limb as u64 * 16 + carry;
                *limb = (value % 1_000_000_000) as u32;
                carry = value / 1_000_000_000;
            }
            if carry > 0 {
                decimal.push(carry as u32);
            }
        }
        if token.starts_with('-') {
            output.push('-');
        }
        let _ = write!(output, "{}", decimal.last().unwrap());
        for limb in decimal[..decimal.len() - 1].iter().rev() {
            let _ = write!(output, "{limb:09}");
        }
    } else {
        if token.starts_with('-') {
            output.push('-');
        }
        let exponent = unsigned.find(['e', 'E']).unwrap_or(unsigned.len());
        let mantissa = &unsigned[..exponent];
        if mantissa.starts_with('.') {
            output.push('0');
        }
        output.push_str(mantissa.strip_suffix('.').unwrap_or(mantissa));
        output.push_str(&unsigned[exponent..]);
    }
    Ok(())
}
