use super::*;

impl<'a> Cursor<'a> {
    pub(super) fn parameter(&mut self, quoted: bool) -> Parsed<'a, CshAstWord<'a>> {
        use CshAstParameterMode as M;
        use CshAstParameterOperation as O;
        self.pos += 2;
        let mut mode = M::Value;
        if matches!(self.byte(), Some(b'#' | b'!'))
            && matches!(
                self.peek(1),
                Some(
                    b'a'..=b'z'
                    | b'A'..=b'Z'
                    | b'0'..=b'9'
                    | b'_'
                    | b'@'
                    | b'*'
                    | b'#'
                    | b'?'
                    | b'$'
                    | b'!'
                    | b'-',
                )
            )
        {
            mode = if self.byte() == Some(b'#') {
                M::Length
            } else {
                M::Indirect
            };
            self.pos += 1;
        }
        let start = self.pos;
        if matches!(
            self.byte(),
            Some(b'@' | b'*' | b'#' | b'?' | b'-' | b'$' | b'!')
        ) {
            self.pos += 1;
        } else {
            while matches!(
                self.byte(),
                Some(b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_')
            ) {
                self.pos += 1;
            }
        }
        if self.pos == start {
            return Err(self.expected("a parameter name"));
        }
        let name = &self.source[start..self.pos];
        if name.as_bytes()[0].is_ascii_digit() && !name.bytes().all(|b| b.is_ascii_digit()) {
            return Err(self.expected("a positional parameter number"));
        }
        let subscript = if self.eat(b'[') {
            let word = self.parameter_operand(b']', false, false)?;
            if word == CshAstWord::Literal("".into()) {
                return Err(self.expected("a parameter subscript"));
            }
            if !self.eat(b']') {
                return Err(self.expected("]"));
            }
            Some(word)
        } else {
            None
        };
        if mode == M::Indirect {
            if matches!(self.byte(), Some(b'@' | b'*')) && self.peek(1) == Some(b'}') {
                mode = M::Names {
                    separate: self.byte() == Some(b'@'),
                };
                self.pos += 1;
            } else if let Some(CshAstWord::Literal(s)) = &subscript
                && (s == "@" || s == "*")
            {
                mode = M::Indices { separate: s == "@" };
            }
        }
        let operation = match self.byte() {
            Some(b'}') => O::None,
            Some(b':' | b'-' | b'=' | b'?' | b'+')
                if self.byte() != Some(b':')
                    || matches!(self.peek(1), Some(b'-' | b'=' | b'?' | b'+')) =>
            {
                let test_empty = self.eat(b':');
                let operator = match self.byte() {
                    Some(b'-') => CshAstDefaultOperator::Use,
                    Some(b'=') => CshAstDefaultOperator::Assign,
                    Some(b'?') => CshAstDefaultOperator::Error,
                    _ => CshAstDefaultOperator::Alternate,
                };
                self.pos += 1;
                O::Default {
                    operator,
                    test_empty,
                    word: self.parameter_operand(b'}', false, quoted)?,
                }
            }
            Some(b':') => {
                self.pos += 1;
                let offset = self.arithmetic_expression(self.source.len())?;
                self.arithmetic_space();
                let length = if self.eat(b':') {
                    Some(self.arithmetic_expression(self.source.len())?)
                } else {
                    None
                };
                self.arithmetic_space();
                O::Slice { offset, length }
            }
            Some(b'#' | b'%') => {
                let symbol = self.byte().unwrap();
                self.pos += 1;
                let longest = self.eat(symbol);
                O::Trim {
                    suffix: symbol == b'%',
                    longest,
                    pattern: self.parameter_operand(b'}', true, false)?,
                }
            }
            Some(b'/') => {
                self.pos += 1;
                let anchor = match self.byte() {
                    Some(b'/') => {
                        self.pos += 1;
                        CshAstReplaceAnchor::All
                    }
                    Some(b'#') => {
                        self.pos += 1;
                        CshAstReplaceAnchor::Prefix
                    }
                    Some(b'%') => {
                        self.pos += 1;
                        CshAstReplaceAnchor::Suffix
                    }
                    _ => CshAstReplaceAnchor::First,
                };
                let pattern = self.parameter_operand(b'/', true, false)?;
                let replacement = if self.eat(b'/') {
                    self.parameter_operand(b'}', false, false)?
                } else {
                    CshAstWord::Literal("".into())
                };
                O::Replace {
                    anchor,
                    pattern,
                    replacement,
                }
            }
            Some(b'^' | b',') => {
                let symbol = self.byte().unwrap();
                self.pos += 1;
                let all = self.eat(symbol);
                O::Case {
                    upper: symbol == b'^',
                    all,
                    pattern: self.parameter_operand(b'}', true, false)?,
                }
            }
            Some(b'@') => {
                use CshAstParameterTransform::*;
                self.pos += 1;
                let transform = match self.byte() {
                    Some(b'Q') => Quote,
                    Some(b'E') => Escape,
                    Some(b'P') => Prompt,
                    Some(b'A') => Assignment,
                    Some(b'a') => Attributes,
                    Some(b'U') => Upper,
                    Some(b'u') => UpperFirst,
                    Some(b'L') => Lower,
                    Some(b'K') => KeyValues,
                    Some(b'k') => Words,
                    _ => return Err(self.expected("a parameter transformation")),
                };
                self.pos += 1;
                O::Transform(transform)
            }
            _ => return Err(self.expected("a parameter operator or }")),
        };
        if mode == M::Length && operation != O::None {
            return Err(self.expected("} after a parameter length"));
        }
        if !self.eat(b'}') {
            return Err(self.expected("a closing parameter brace"));
        }
        Ok(CshAstWord::Parameter(Box::new(CshAstParameter {
            name,
            subscript,
            mode,
            operation,
        })))
    }

    /// Delimiter-aware parameter operands. Spaces and shell operators are text;
    /// expansions and quotes are consumed as units, so their delimiters cannot
    /// terminate this operand. Pattern positions additionally expose glob syntax.
    fn parameter_operand(
        &mut self,
        stop: u8,
        pattern: bool,
        quoted: bool,
    ) -> Parsed<'a, CshAstWord<'a>> {
        let mut parts = Vec::new();
        let mut brackets = 0;
        while let Some(b) = self.byte() {
            if (b == stop && brackets == 0) || b == b'}' {
                break;
            }
            if parts.is_empty()
                && !quoted
                && b == b'~'
                && let Some(tilde) = self.tilde(Some(stop))
            {
                parts.push(tilde);
                continue;
            }
            if stop == b']' && b == b'[' {
                brackets += 1;
            }
            if stop == b']' && b == b']' {
                brackets -= 1;
            }
            if matches!(b, b'$' | b'`' | b'"' | b'\\')
                || (b == b'\'' && !quoted)
                || (pattern
                    && (matches!(b, b'*' | b'?' | b'[')
                        || matches!(b, b'+' | b'@' | b'!') && self.peek(1) == Some(b'(')))
            {
                let inherited_quote = quoted
                    && (b == b'\\' && self.peek(1) != Some(b'}')
                        || b == b'$' && !matches!(self.peek(1), Some(b'\'' | b'"')));
                parts.push(self.word_part(inherited_quote, self.source.len())?);
            } else {
                let c = self.rest().chars().next().unwrap();
                self.pos += c.len_utf8();
                match parts.last_mut() {
                    Some(CshAstWord::Literal(text)) => text.to_mut().push(c),
                    _ => parts.push(CshAstWord::Literal(
                        self.source[self.pos - c.len_utf8()..self.pos].into(),
                    )),
                }
            }
        }
        Ok(CshAstWord::concat(parts))
    }
}
