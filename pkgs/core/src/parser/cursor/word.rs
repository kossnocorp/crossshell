use super::*;

impl<'a> Cursor<'a> {
    pub(super) fn assignment(&mut self) -> Parsed<'a, Option<CshAstAssignment<'a>>> {
        if !matches!(self.byte(), Some(b'a'..=b'z' | b'A'..=b'Z' | b'_')) {
            return Ok(None);
        }
        let len = self
            .rest()
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_')
            .count();
        let (operator, width) = match (self.peek(len), self.peek(len + 1)) {
            (Some(b'='), _) => (CshAstAssignmentOperator::Set, 1),
            (Some(b'+'), Some(b'=')) => (CshAstAssignmentOperator::Append, 2),
            _ => return Ok(None),
        };
        let name = &self.source[self.pos..self.pos + len];
        self.pos += len + width;
        let value = self.assignment_value()?;
        Ok(Some(CshAstAssignment {
            name,
            operator,
            value,
        }))
    }

    fn assignment_value(&mut self) -> Parsed<'a, CshAstWord<'a>> {
        if self.byte() == Some(b'(') {
            self.array_word()
        } else {
            Ok(self
                .assignment_word()?
                .unwrap_or_else(|| CshAstWord::Literal("".into())))
        }
    }

    pub(super) fn word(&mut self) -> Parsed<'a, Option<CshAstWord<'a>>> {
        if self.byte() == Some(b'#') {
            return Ok(None);
        }
        self.shell_word(false)
    }

    // The value starts inside an existing assignment word: a leading `#` is
    // literal here, whereas it starts a comment at a command-word boundary.
    pub(super) fn assignment_word(&mut self) -> Parsed<'a, Option<CshAstWord<'a>>> {
        self.shell_word(true)
    }

    fn shell_word(&mut self, assignment: bool) -> Parsed<'a, Option<CshAstWord<'a>>> {
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
                    if (parts.is_empty()
                        || matches!(
                            parts.last(),
                            Some(
                                CshAstWord::BraceAlternatives(_)
                                    | CshAstWord::BraceSequence(_)
                                    | CshAstWord::Concat(_)
                            )
                        )
                        || assignment
                            && matches!(parts.last(), Some(CshAstWord::Literal(text)) if text.ends_with(':')))
                        && let Some(tilde) = self.tilde(None)
                    {
                        parts.push(tilde);
                        continue;
                    }
                    if assignment && self.byte() == Some(b':') {
                        self.pos += 1;
                        parts.push(CshAstWord::Literal(":".into()));
                        continue;
                    }
                    parts.push(self.word_part(false, self.source.len())?);
                    if before == self.pos {
                        break;
                    }
                }
            }
        }
        Ok((self.pos != start).then(|| CshAstWord::concat(parts)))
    }

    pub(super) fn array_word(&mut self) -> Parsed<'a, CshAstWord<'a>> {
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
            if let Some((end, operator, width)) = self.array_key_end() {
                self.pos += 1;
                let key = self.expansion_operand(end)?;
                self.pos = end + width;
                let value = self.assignment_value()?;
                elements.push(CshAstWord::KeyedElement {
                    key: Box::new(key),
                    operator,
                    value: Box::new(value),
                });
            } else {
                elements.push(self.required_word()?);
            }
        }
        self.depth -= 1;
        Ok(CshAstWord::Array(elements))
    }

    /// Look ahead without allocating expression nodes. Brackets in quotes or
    /// nested expansions cannot terminate the key. Only `]=`/`]+=` syntax
    /// introduces a keyed element; an ordinary `[abc]` array word is a glob.
    fn array_key_end(&self) -> Option<(usize, CshAstAssignmentOperator, usize)> {
        if self.byte() != Some(b'[') {
            return None;
        }
        let bytes = self.source.as_bytes();
        let mut pos = self.pos + 1;
        let mut closings = vec![b']'];
        let mut quote = None;
        while let Some(&b) = bytes.get(pos) {
            if b == b'\\' && quote != Some(b'\'') {
                pos += 2;
                continue;
            }
            if let Some(q) = quote {
                if b == q {
                    quote = None;
                }
            } else {
                match b {
                    b'\'' | b'"' | b'`' => quote = Some(b),
                    b'[' => closings.push(b']'),
                    b'(' => closings.push(b')'),
                    b'{' => closings.push(b'}'),
                    b']' | b')' | b'}' if closings.last() == Some(&b) => {
                        closings.pop();
                        if closings.is_empty() {
                            return match (bytes.get(pos + 1), bytes.get(pos + 2)) {
                                (Some(b'='), _) => Some((pos, CshAstAssignmentOperator::Set, 2)),
                                (Some(b'+'), Some(b'=')) => {
                                    Some((pos, CshAstAssignmentOperator::Append, 3))
                                }
                                _ => None,
                            };
                        }
                    }
                    b'\n' | b';' if closings.len() == 1 => return None,
                    _ => {}
                }
            }
            pos += 1;
        }
        None
    }

    pub(super) fn word_part(&mut self, quoted: bool, end: usize) -> Parsed<'a, CshAstWord<'a>> {
        if self.depth >= 128 {
            return Err(self.expected("less deeply nested shell syntax"));
        }
        self.depth += 1;
        let result = self.word_part_inner(quoted, end);
        self.depth -= 1;
        result
    }

    fn word_part_inner(&mut self, quoted: bool, end: usize) -> Parsed<'a, CshAstWord<'a>> {
        use CshAstWord as W;
        match self.byte() {
            Some(b'\'') if !quoted => Ok(W::SingleQuoted(self.single_text()?)),
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
            Some(b'\\') => Ok(W::Escaped(self.escape_text(quoted)?)),
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
                let text = &self.source[start..self.pos];
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
            Some(b'$') => self.word_expansion(quoted),
            Some(b'{') if !quoted => self.brace_word(),
            Some(b',' | b'}' | b':') if !quoted => {
                self.pos += 1;
                Ok(W::Literal(self.source[self.pos - 1..self.pos].into()))
            }
            Some(b'`') => self.command_word(true, None),
            Some(b'<' | b'>') if !quoted && self.peek(1) == Some(b'(') => {
                self.command_word(false, Some(self.byte().unwrap()))
            }
            Some(b'?' | b'*' | b'+' | b'@' | b'!') if !quoted && self.peek(1) == Some(b'(') => {
                self.extended_glob()
            }
            Some(b'*') if !quoted => {
                self.pos += 1;
                let glob = if self.pos < end && self.peek(1) != Some(b'(') && self.eat(b'*') {
                    CshAstGlob::GlobStar
                } else {
                    CshAstGlob::Star
                };
                Ok(W::Glob(glob))
            }
            Some(b'?') if !quoted => {
                self.pos += 1;
                Ok(W::Glob(CshAstGlob::QuestionMark))
            }
            Some(b'[') if !quoted => self.glob_class(end),
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
                        if matches!(b, b'*' | b'?' | b'[' | b'{' | b'}' | b',' | b':') {
                            break;
                        }
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
                let text = self.source[start..self.pos].into();
                Ok(W::Literal(text))
            }
        }
    }

    /// Expansion operands allow spaces and shell punctuation as literal text.
    fn expansion_operand(&mut self, end: usize) -> Parsed<'a, CshAstWord<'a>> {
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
                parts.push(CshAstWord::Literal(self.source[start..self.pos].into()));
            }
        }
        Ok(CshAstWord::concat(parts))
    }

    fn word_expansion(&mut self, quoted: bool) -> Parsed<'a, CshAstWord<'a>> {
        use CshAstWord as W;
        match self.peek(1) {
            Some(b'(') if self.peek(2) != Some(b'(') => self.command_word(false, None),
            Some(b'(') => {
                self.pos += 1;
                let expression = self.arithmetic_command()?;
                Ok(W::ArithmeticExpansion(Box::new(expression)))
            }
            Some(b'{') => self.parameter(quoted),
            Some(b'a'..=b'z' | b'A'..=b'Z' | b'_') => {
                self.pos += 1;
                let start = self.pos;
                while matches!(
                    self.byte(),
                    Some(b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_')
                ) {
                    self.pos += 1;
                }
                Ok(W::Variable(&self.source[start..self.pos]))
            }
            Some(b'0'..=b'9' | b'@' | b'*' | b'#' | b'?' | b'-' | b'$' | b'!') => {
                self.pos += 2;
                Ok(W::Variable(&self.source[self.pos - 1..self.pos]))
            }
            _ => {
                self.pos += 1;
                Ok(W::Literal("$".into()))
            }
        }
    }

    fn command_word(&mut self, backticks: bool, process: Option<u8>) -> Parsed<'a, CshAstWord<'a>> {
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
                operator: if operator == b'<' { "<" } else { ">" },
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
