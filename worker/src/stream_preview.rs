/// Append-only presentation while the original response is arriving. This
/// lexical state survives chunk boundaries; the saved source is kept elsewhere.
#[derive(Default)]
pub(super) struct StreamPreview {
    depth: usize,
    quote: Option<char>,
    escaped: bool,
    slash: bool,
    line_comment: bool,
    block_comment: bool,
    star: bool,
    newline: bool,
    space: bool,
    last: Option<char>,
}

impl StreamPreview {
    pub(super) fn push(&mut self, chunk: &str) -> String {
        let mut output = String::with_capacity(chunk.len() + chunk.len() / 4);
        for character in chunk.chars() {
            if let Some(quote) = self.quote {
                self.emit(character, &mut output);
                if self.escaped {
                    self.escaped = false;
                } else if character == '\\' {
                    self.escaped = true;
                } else if character == quote {
                    self.quote = None;
                }
                continue;
            }
            if self.line_comment {
                self.emit(character, &mut output);
                if character == '\n' {
                    self.line_comment = false;
                    self.newline = true;
                }
                continue;
            }
            if self.block_comment {
                self.emit(character, &mut output);
                if self.star && character == '/' {
                    self.block_comment = false;
                }
                self.star = character == '*';
                continue;
            }
            if self.slash {
                self.slash = false;
                self.prefix(false, &mut output);
                self.emit('/', &mut output);
                if matches!(character, '/' | '*') {
                    self.emit(character, &mut output);
                    self.line_comment = character == '/';
                    self.block_comment = character == '*';
                    self.star = false;
                    continue;
                }
            }
            if character.is_whitespace() {
                self.space |= self
                    .last
                    .is_some_and(|last| last.is_alphanumeric() || last == '_');
                continue;
            }
            if character == '/' {
                self.slash = true;
                continue;
            }
            let closing = matches!(character, '}' | ']');
            if closing {
                self.depth = self.depth.saturating_sub(1);
                if matches!(self.last, Some('{' | '[')) {
                    self.newline = false;
                } else {
                    self.newline = true;
                }
                self.space = false;
            } else if matches!(character, ',' | ':') {
                self.space = false;
                self.newline = false;
            }
            self.prefix(closing, &mut output);
            self.emit(character, &mut output);
            match character {
                '"' | '\'' => self.quote = Some(character),
                '{' | '[' => {
                    self.depth += 1;
                    self.newline = true;
                }
                ',' => self.newline = true,
                ':' => self.space = true,
                _ => {}
            }
        }
        output
    }

    fn prefix(&mut self, closing: bool, output: &mut String) {
        if self.newline {
            if self.last != Some('\n') {
                self.emit('\n', output);
            }
            for _ in 0..self.depth.saturating_mul(2) {
                self.emit(' ', output);
            }
        } else if self.space && !closing {
            self.emit(' ', output);
        }
        self.newline = false;
        self.space = false;
    }

    fn emit(&mut self, character: char, output: &mut String) {
        output.push(character);
        self.last = Some(character);
    }
}
