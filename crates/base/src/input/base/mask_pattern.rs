use std::borrow::Cow;

use gpui::SharedString;

#[derive(Clone, PartialEq, Debug)]
pub enum MaskToken {
    // Digit0,
    Digit,

    Letter,

    LetterOrDigit,

    Sep(char),

    Any,
}

#[allow(unused)]
impl MaskToken {
    pub fn is_any(&self) -> bool {
        matches!(self, MaskToken::Any)
    }

    fn is_match(&self, ch: char) -> bool {
        match self {
            MaskToken::Digit => ch.is_ascii_digit(),
            MaskToken::Letter => ch.is_ascii_alphabetic(),
            MaskToken::LetterOrDigit => ch.is_ascii_alphanumeric(),
            MaskToken::Any => true,
            MaskToken::Sep(c) => *c == ch,
        }
    }

    fn is_sep(&self) -> bool {
        matches!(self, MaskToken::Sep(_))
    }

    pub fn is_number(&self) -> bool {
        matches!(self, MaskToken::Digit)
    }

    pub fn placeholder(&self) -> char {
        match self {
            MaskToken::Sep(c) => *c,
            _ => '_',
        }
    }

    fn mask_char(&self, ch: char) -> char {
        match self {
            MaskToken::Digit | MaskToken::LetterOrDigit | MaskToken::Letter => ch,
            MaskToken::Sep(c) => *c,
            MaskToken::Any => ch,
        }
    }

    fn unmask_char(&self, ch: char) -> Option<char> {
        match self {
            MaskToken::Digit => Some(ch),
            MaskToken::Letter => Some(ch),
            MaskToken::LetterOrDigit => Some(ch),
            MaskToken::Any => Some(ch),
            _ => None,
        }
    }
}

#[derive(Clone, Default)]
pub enum MaskPattern {
    #[default]
    None,
    Pattern {
        pattern: SharedString,
        tokens: Vec<MaskToken>,
    },
    Number {
        separator: Option<char>,

        fraction: Option<usize>,
    },
}

impl From<&str> for MaskPattern {
    fn from(pattern: &str) -> Self {
        Self::new(pattern)
    }
}

impl MaskPattern {
    pub fn new(pattern: &str) -> Self {
        let tokens = pattern
            .chars()
            .map(|ch| match ch {
                // '0' => MaskToken::Digit0,
                '9' => MaskToken::Digit,
                'A' => MaskToken::Letter,
                '#' => MaskToken::LetterOrDigit,
                '*' => MaskToken::Any,
                _ => MaskToken::Sep(ch),
            })
            .collect();

        Self::Pattern {
            pattern: pattern.to_owned().into(),
            tokens,
        }
    }

    #[allow(unused)]
    fn tokens(&self) -> Option<&Vec<MaskToken>> {
        match self {
            Self::Pattern { tokens, .. } => Some(tokens),
            Self::Number { .. } => None,
            Self::None => None,
        }
    }

    pub fn number(sep: Option<char>) -> Self {
        Self::Number {
            separator: sep,
            fraction: None,
        }
    }

    pub fn placeholder(&self) -> Option<String> {
        match self {
            Self::Pattern { tokens, .. } => {
                Some(tokens.iter().map(|token| token.placeholder()).collect())
            }
            Self::Number { .. } => None,
            Self::None => None,
        }
    }

    pub fn is_none(&self) -> bool {
        match self {
            Self::Pattern { tokens, .. } => tokens.is_empty(),
            Self::Number { .. } => false,
            Self::None => true,
        }
    }

    pub fn is_valid(&self, mask_text: &str) -> bool {
        if self.is_none() {
            return true;
        }

        let mut text_index = 0;
        let mask_text_chars: Vec<char> = mask_text.chars().collect();
        match self {
            Self::Pattern { tokens, .. } => {
                for token in tokens {
                    if text_index >= mask_text_chars.len() {
                        break;
                    }

                    let ch = mask_text_chars[text_index];
                    if token.is_match(ch) {
                        text_index += 1;
                    }
                }
                text_index == mask_text_chars.len()
            }
            Self::Number { separator, .. } => {
                if mask_text.is_empty() {
                    return true;
                }

                // check if the text is valid number
                let mut parts = mask_text.split('.');
                let int_part = parts.next().unwrap_or("");
                let frac_part = parts.next();

                // only one dot is valid
                if parts.next().is_some() {
                    return false;
                }

                let sign_positions: Vec<usize> = int_part
                    .chars()
                    .enumerate()
                    .filter_map(|(i, ch)| match is_sign(&ch) {
                        true => Some(i),
                        false => None,
                    })
                    .collect();

                // only one sign is valid
                // sign is only valid at the beginning of the string
                if sign_positions.len() > 1 || sign_positions.first() > Some(&0) {
                    return false;
                }

                // check if the integer part is valid
                if !int_part.chars().enumerate().all(|(i, ch)| {
                    ch.is_ascii_digit() || is_sign(&ch) && i == 0 || Some(ch) == *separator
                }) {
                    return false;
                }

                // check if the fraction part is valid
                if let Some(frac) = frac_part {
                    if !frac
                        .chars()
                        .all(|ch| ch.is_ascii_digit() || Some(ch) == *separator)
                    {
                        return false;
                    }
                }

                true
            }
            Self::None => true,
        }
    }

    pub fn is_valid_at(&self, ch: char, pos: usize) -> bool {
        if self.is_none() {
            return true;
        }

        match self {
            Self::Pattern { tokens, .. } => {
                if let Some(token) = tokens.get(pos) {
                    if token.is_match(ch) {
                        return true;
                    }

                    if token.is_sep() {
                        // If next token is match, it's valid
                        if let Some(next_token) = tokens.get(pos + 1) {
                            if next_token.is_match(ch) {
                                return true;
                            }
                        }
                    }
                }

                false
            }
            Self::Number { .. } => true,
            Self::None => true,
        }
    }

    pub fn mask(&self, text: &str) -> SharedString {
        if self.is_none() {
            return text.to_owned().into();
        }

        match self {
            Self::Number {
                separator,
                fraction,
            } => {
                if let Some(sep) = *separator {
                    // Remove the existing group separator
                    let text = text.replace(sep, "");

                    let mut parts = text.split('.');
                    let int_part = parts.next().unwrap_or("");

                    // Limit the fraction part to the given range, if not enough, pad with 0
                    let frac_part = parts.next().map(|part| {
                        part.chars()
                            .take(fraction.unwrap_or(usize::MAX))
                            .collect::<String>()
                    });

                    // Reverse the integer part for easier grouping
                    let mut chars: Vec<char> = int_part.chars().rev().collect();

                    // Removing the sign from formatting to avoid cases such as: -,123
                    let maybe_signed = if let Some(pos) = chars.iter().position(is_sign) {
                        Some(chars.remove(pos))
                    } else {
                        None
                    };

                    let mut result = String::new();
                    for (i, ch) in chars.iter().enumerate() {
                        if i > 0 && i % 3 == 0 {
                            result.push(sep);
                        }
                        result.push(*ch);
                    }
                    let int_with_sep: String = result.chars().rev().collect();

                    let final_str = if let Some(frac) = frac_part {
                        if fraction == &Some(0) {
                            int_with_sep
                        } else {
                            format!("{}.{}", int_with_sep, frac)
                        }
                    } else {
                        int_with_sep
                    };

                    let final_str = if let Some(sign) = maybe_signed {
                        format!("{}{}", sign, final_str)
                    } else {
                        final_str
                    };

                    return final_str.into();
                }

                text.to_owned().into()
            }
            Self::Pattern { tokens, .. } => {
                let mut result = String::new();
                let mut text_index = 0;
                let text_chars: Vec<char> = text.chars().collect();
                for (pos, token) in tokens.iter().enumerate() {
                    if text_index >= text_chars.len() {
                        break;
                    }
                    let ch = text_chars[text_index];
                    // Break if expected char is not match
                    if !token.is_sep() && !self.is_valid_at(ch, pos) {
                        break;
                    }
                    let mask_ch = token.mask_char(ch);
                    result.push(mask_ch);
                    if ch == mask_ch {
                        text_index += 1;
                        continue;
                    }
                }
                result.into()
            }
            Self::None => text.to_owned().into(),
        }
    }

    pub fn unmask(&self, mask_text: &str) -> String {
        match self {
            Self::Number { separator, .. } => {
                if let Some(sep) = *separator {
                    let mut result = String::new();
                    for ch in mask_text.chars() {
                        if ch == sep {
                            continue;
                        }
                        result.push(ch);
                    }

                    if result.contains('.') {
                        result = result.trim_end_matches('0').to_string();
                    }
                    return result;
                }

                return mask_text.to_owned();
            }
            Self::Pattern { tokens, .. } => {
                let mut result = String::new();
                let mask_text_chars: Vec<char> = mask_text.chars().collect();
                for (text_index, token) in tokens.iter().enumerate() {
                    if text_index >= mask_text_chars.len() {
                        break;
                    }
                    let ch = mask_text_chars[text_index];
                    let unmask_ch = token.unmask_char(ch);
                    if let Some(ch) = unmask_ch {
                        result.push(ch);
                    }
                }
                result
            }
            Self::None => mask_text.to_owned(),
        }
    }
}

#[inline]
fn is_sign(ch: &char) -> bool {
    matches!(ch, '+' | '-')
}

#[doc(hidden)]
pub(super) fn normalize_number_input(text: &str) -> Cow<'_, str> {
    #[inline]
    fn normalize_char(ch: char) -> Option<char> {
        match ch {
            // Full-width digits 0-9
            '\u{FF10}'..='\u{FF19}' => char::from_u32(ch as u32 - 0xFF10 + '0' as u32),
            // Full-width plus ＋
            '\u{FF0B}' => Some('+'),
            // Full-width hyphen － and minus sign −
            '\u{FF0D}' | '\u{2212}' => Some('-'),
            // Full-width dot ． and ideographic full stop 。
            '\u{FF0E}' | '\u{3002}' => Some('.'),
            // Full-width comma ，
            '\u{FF0C}' => Some(','),
            _ => None,
        }
    }

    if !text.chars().any(|ch| normalize_char(ch).is_some()) {
        return Cow::Borrowed(text);
    }

    Cow::Owned(
        text.chars()
            .map(|ch| normalize_char(ch).unwrap_or(ch))
            .collect(),
    )
}
