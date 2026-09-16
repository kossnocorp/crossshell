use super::*;

impl<'a> Cursor<'a> {
    pub(super) fn word(&mut self) -> Parsed<'a, Option<CshAstWord>> {
        if self.byte() == Some(b'#') {
            return Ok(None);
        }
        self.assignment_word()
    }

    // The value starts inside an existing assignment word: a leading `#` is
    // literal here, whereas it starts a comment at a command-word boundary.
    pub(super) fn assignment_word(&mut self) -> Parsed<'a, Option<CshAstWord>> {
        let start = self.pos;
        let mut parts = Vec::new();
        loop {
            match self.byte() {
                Some(b'(') if matches!(parts.last(), Some(CshAstWord::Literal(s)) if s.ends_with('=')) =>
                {
                    parts.push(self.array_word()?);
                }
                None | Some(b' ' | b'\t' | b'\r' | b'\n' | b';' | b'|' | b'&' | b'(' | b')') => {
                    break;
                }
                Some(b'`') if self.backtick => break,
                Some(b'<' | b'>') if self.peek(1) != Some(b'(') => break,
                Some(b) if b >= 128 && self.rest().chars().next().unwrap().is_whitespace() => break,
                _ => {
                    let before = self.pos;
                    parts.push(self.word_part(false, self.source.len())?);
                    if before == self.pos {
                        break;
                    }
                }
            }
        }
        Ok((self.pos != start).then(|| CshAstWord::concat(parts)))
    }

    pub(super) fn array_word(&mut self) -> Parsed<'a, CshAstWord> {
        if self.depth >= 128 {
            return Err(self.expected("less deeply nested shell syntax"));
        }
        self.depth += 1;
        self.pos += 1;
        let mut elements = Vec::new();
        loop {
            self.continuation()?;
            if self.eat(b')') {
                break;
            }
            elements.push(self.required_word()?);
        }
        self.depth -= 1;
        Ok(CshAstWord::Array(elements))
    }

    pub(super) fn word_part(&mut self, quoted: bool, end: usize) -> Parsed<'a, CshAstWord> {
        if self.depth >= 128 {
            return Err(self.expected("less deeply nested shell syntax"));
        }
        self.depth += 1;
        let result = self.word_part_inner(quoted, end);
        self.depth -= 1;
        result
    }

    fn word_part_inner(&mut self, quoted: bool, end: usize) -> Parsed<'a, CshAstWord> {
        use CshAstWord as W;
        match self.byte() {
            Some(b'\'') if !quoted => {
                let mut text = String::new();
                self.single(&mut text)?;
                Ok(W::SingleQuoted(text))
            }
            Some(b'"') if !quoted => {
                let opening = self.pos;
                self.pos += 1;
                let mut parts = Vec::new();
                while self.byte() != Some(b'"') {
                    if self.byte().is_none() {
                        return Err(self.unclosed('"', opening));
                    }
                    parts.push(self.word_part(true, end)?);
                }
                self.pos += 1;
                Ok(W::DoubleQuoted(Box::new(W::concat(parts))))
            }
            Some(b'\\') => {
                let mut text = String::new();
                self.escape(&mut text, quoted)?;
                Ok(W::Escaped(text))
            }
            Some(b'$') if !quoted && self.peek(1) == Some(b'\'') => {
                let opening = self.pos + 1;
                self.pos += 2;
                let start = self.pos;
                loop {
                    match self.byte() {
                        None => return Err(self.unclosed('\'', opening)),
                        Some(b'\'') => break,
                        Some(b'\\') => self.skip_escape()?,
                        _ => self.pos += 1,
                    }
                }
                let text = self.source[start..self.pos].to_owned();
                self.pos += 1;
                Ok(W::AnsiCQuoted(text))
            }
            Some(b'$') if !quoted && self.peek(1) == Some(b'"') => {
                self.pos += 1;
                let W::DoubleQuoted(word) = self.word_part(false, end)? else {
                    unreachable!()
                };
                Ok(W::LocaleQuoted(word))
            }
            Some(b'$') => self.word_expansion(),
            Some(b'`') => self.command_word(true, None),
            Some(b'<' | b'>') if !quoted && self.peek(1) == Some(b'(') => {
                self.command_word(false, Some(self.byte().unwrap()))
            }
            Some(b'?' | b'*' | b'+' | b'@' | b'!') if !quoted && self.peek(1) == Some(b'(') => {
                let operator = self.byte().unwrap() as char;
                self.pos += 2;
                let start = self.pos;
                self.balanced(b')')?;
                let end = self.pos - 1;
                self.pos = start;
                let pattern = self.expansion_operand(end)?;
                self.pos = end + 1;
                Ok(W::ExtendedGlob {
                    operator,
                    pattern: Box::new(pattern),
                })
            }
            _ => {
                let start = self.pos;
                while self.pos < end {
                    let b = self.byte().unwrap();
                    if quoted {
                        if matches!(b, b'"' | b'\\' | b'$' | b'`') {
                            break;
                        }
                        self.pos += 1;
                    } else {
                        match WORD_CLASS[b as usize] {
                            BARE => self.pos += 1,
                            GLOB if self.peek(1) != Some(b'(') => self.pos += 1,
                            UNICODE => {
                                let c = self.rest().chars().next().unwrap();
                                if c.is_whitespace() {
                                    break;
                                }
                                self.pos += c.len_utf8();
                            }
                            _ => break,
                        }
                    }
                }
                let text = self.source[start..self.pos].to_owned();
                Ok(if !quoted && text.contains(['*', '?', '[']) {
                    W::Pattern(text)
                } else {
                    W::Literal(text)
                })
            }
        }
    }

    /// Expansion operands allow spaces and shell punctuation as literal text.
    fn expansion_operand(&mut self, end: usize) -> Parsed<'a, CshAstWord> {
        let mut parts = Vec::new();
        while self.pos < end {
            if matches!(self.byte(), Some(b'$' | b'`' | b'\'' | b'"' | b'\\')) {
                parts.push(self.word_part(false, end)?);
            } else {
                let start = self.pos;
                while self.pos < end
                    && !matches!(self.byte(), Some(b'$' | b'`' | b'\'' | b'"' | b'\\'))
                {
                    self.pos += 1;
                }
                parts.push(CshAstWord::Literal(self.source[start..self.pos].to_owned()));
            }
        }
        Ok(CshAstWord::concat(parts))
    }

    fn word_expansion(&mut self) -> Parsed<'a, CshAstWord> {
        use CshAstWord as W;
        match self.peek(1) {
            Some(b'(') if self.peek(2) != Some(b'(') => self.command_word(false, None),
            Some(b'(') => {
                self.pos += 3;
                let start = self.pos;
                self.balanced(b')')?;
                let end = self.pos - 1;
                self.require_close_paren()?;
                let after = self.pos;
                self.pos = start;
                let expression = self.expansion_operand(end)?;
                self.pos = after;
                Ok(W::ArithmeticExpansion(Box::new(expression)))
            }
            Some(b'{') => {
                self.pos += 2;
                let start = self.pos;
                self.balanced(b'}')?;
                let end = self.pos - 1;
                self.pos = start;
                let prefix = if matches!(self.byte(), Some(b'#' | b'!')) && self.pos + 1 < end {
                    self.pos += 1;
                    self.source[start..self.pos].to_owned()
                } else {
                    String::new()
                };
                let name_start = self.pos;
                if matches!(
                    self.byte(),
                    Some(b'@' | b'*' | b'#' | b'?' | b'-' | b'$' | b'!')
                ) {
                    self.pos += 1;
                } else {
                    while self.pos < end
                        && matches!(
                            self.byte(),
                            Some(b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_')
                        )
                    {
                        self.pos += 1;
                    }
                }
                let name = self.source[name_start..self.pos].to_owned();
                let suffix = self.expansion_operand(end)?;
                self.pos = end + 1;
                Ok(W::Parameter {
                    prefix,
                    name,
                    suffix: Box::new(suffix),
                })
            }
            Some(b'a'..=b'z' | b'A'..=b'Z' | b'_') => {
                self.pos += 1;
                let start = self.pos;
                while matches!(
                    self.byte(),
                    Some(b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_')
                ) {
                    self.pos += 1;
                }
                Ok(W::Variable(self.source[start..self.pos].to_owned()))
            }
            Some(b'0'..=b'9' | b'@' | b'*' | b'#' | b'?' | b'-' | b'$' | b'!') => {
                self.pos += 2;
                Ok(W::Variable(self.source[self.pos - 1..self.pos].to_owned()))
            }
            _ => {
                self.pos += 1;
                Ok(W::Literal("$".to_owned()))
            }
        }
    }

    fn command_word(&mut self, backticks: bool, process: Option<u8>) -> Parsed<'a, CshAstWord> {
        self.pos += if backticks { 1 } else { 2 };
        let outer_pending = std::mem::take(&mut self.pending_here_documents);
        let outer_backtick = self.backtick;
        self.backtick = backticks;
        let commands = self.list(
            if backticks {
                Stop::Backtick
            } else {
                Stop::Subshell
            },
            false,
        )?;
        if backticks {
            if !self.eat(b'`') {
                return Err(self.expected("a closing backtick"));
            }
        } else {
            self.require_close_paren()?;
        }
        if !self.pending_here_documents.is_empty() {
            return Err(self.expected("a here-document body"));
        }
        self.pending_here_documents = outer_pending;
        self.backtick = outer_backtick;
        Ok(if let Some(operator) = process {
            CshAstWord::ProcessSubstitution {
                operator: (operator as char).to_string(),
                commands,
            }
        } else {
            CshAstWord::CommandSubstitution {
                commands,
                backticks,
            }
        })
    }
}
