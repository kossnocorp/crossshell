use super::*;

impl<'a> Cursor<'a> {
    pub(super) fn condition(&mut self) -> Parsed<'a, CshAstCondition<'a>> {
        self.pos += 2;
        let condition = self.condition_bp(0)?;
        self.arithmetic_space();
        if !self.rest().starts_with("]]") {
            return Err(self.expected("a closing ]]"));
        }
        self.pos += 2;
        Ok(condition)
    }

    fn condition_bp(&mut self, min: u8) -> Parsed<'a, CshAstCondition<'a>> {
        if self.depth >= 128 {
            return Err(self.expected("less deeply nested conditions"));
        }
        self.depth += 1;
        let result = self.condition_inner(min);
        self.depth -= 1;
        result
    }

    fn condition_inner(&mut self, min: u8) -> Parsed<'a, CshAstCondition<'a>> {
        use CshAstConditionKind as K;
        self.arithmetic_space();
        let start = self.pos;
        let kind = if self.keyword() == Some(Keyword::Bang) {
            self.pos += 1;
            K::Not(Box::new(self.condition_bp(3)?))
        } else if self.eat(b'(') {
            let inner = self.condition_bp(0)?;
            self.arithmetic_space();
            self.require_close_paren()?;
            inner.kind
        } else {
            let left = self.condition_word(false)?;
            self.arithmetic_space();
            let before = self.pos;
            let op = self.condition_operator();
            if let Some(operator) = op {
                self.arithmetic_space();
                let right = self.condition_word(operator == CshAstTestBinary::Regex)?;
                K::Binary {
                    left,
                    operator,
                    right,
                }
            } else if let CshAstWord::Literal(text) = &left {
                if let Some(operator) = test_unary(text) {
                    if self.condition_end() {
                        K::Word(left)
                    } else {
                        K::Unary {
                            operator,
                            operand: self.condition_word(false)?,
                        }
                    }
                } else {
                    self.pos = before;
                    K::Word(left)
                }
            } else {
                K::Word(left)
            }
        };
        let mut left = CshAstCondition {
            span: start..self.pos,
            kind,
        };
        let mut chain = 0;
        loop {
            self.arithmetic_space();
            let and = self.rest().starts_with("&&");
            let or = self.rest().starts_with("||");
            let precedence = if and { 2 } else { 1 };
            if (!and && !or) || precedence < min {
                break;
            }
            chain += 1;
            if chain > 128 {
                return Err(self.expected("a shorter condition operator chain"));
            }
            self.pos += 2;
            let right = self.condition_bp(precedence + 1)?;
            let kind = if and {
                K::And(Box::new(left), Box::new(right))
            } else {
                K::Or(Box::new(left), Box::new(right))
            };
            left = CshAstCondition {
                span: start..self.pos,
                kind,
            };
        }
        let mut pending = vec![(&left, 0)];
        while let Some((node, depth)) = pending.pop() {
            if depth >= 128 {
                return Err(self.expected("a shallower conditional tree"));
            }
            match &node.kind {
                K::Not(operand) => pending.push((operand, depth + 1)),
                K::And(left, right) | K::Or(left, right) => {
                    pending.push((left, depth + 1));
                    pending.push((right, depth + 1));
                }
                _ => {}
            }
        }
        Ok(left)
    }

    fn condition_end(&self) -> bool {
        self.rest().starts_with("]]")
            || self.rest().starts_with("&&")
            || self.rest().starts_with("||")
            || self.byte() == Some(b')')
            || self.byte().is_none()
    }

    fn condition_operator(&mut self) -> Option<CshAstTestBinary> {
        use CshAstTestBinary::*;
        let (text, op) = [
            ("==", PatternEqual),
            ("!=", PatternNotEqual),
            ("=~", Regex),
            ("=", PatternEqual),
            ("<", StringLess),
            (">", StringGreater),
            ("-eq", Equal),
            ("-ne", NotEqual),
            ("-lt", Less),
            ("-le", LessEqual),
            ("-gt", Greater),
            ("-ge", GreaterEqual),
            ("-nt", Newer),
            ("-ot", Older),
            ("-ef", SameFile),
        ]
        .into_iter()
        .find(|(text, _)| {
            self.rest().starts_with(text)
                && self
                    .peek(text.len())
                    .is_none_or(|b| b.is_ascii_whitespace() || matches!(b, b'\'' | b'"' | b'$'))
        })?;
        self.pos += text.len();
        Some(op)
    }

    fn condition_word(&mut self, regex: bool) -> Parsed<'a, CshAstWord<'a>> {
        self.arithmetic_space();
        if self.condition_end() {
            return Err(self.expected("a conditional operand"));
        }
        let start = self.pos;
        let mut parts = Vec::new();
        let mut parentheses = 0;
        while let Some(b) = self.byte() {
            if b.is_ascii_whitespace() {
                break;
            }
            if !regex && matches!(b, b'(' | b')' | b'&' | b'|' | b'<' | b'>') {
                break;
            }
            if regex && b == b')' && parentheses == 0 {
                break;
            }
            if matches!(b, b'\'' | b'"' | b'$' | b'`' | b'\\')
                || (!regex
                    && (matches!(b, b'*' | b'?' | b'[')
                        || matches!(b, b'+' | b'@' | b'!') && self.peek(1) == Some(b'(')))
            {
                parts.push(self.word_part(false, self.source.len())?);
            } else {
                if regex && b == b'(' {
                    parentheses += 1;
                }
                if regex && b == b')' {
                    parentheses -= 1;
                }
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
        if self.pos == start {
            return Err(self.expected("a conditional operand"));
        }
        Ok(CshAstWord::concat(parts))
    }
}

fn test_unary(text: &str) -> Option<CshAstTestUnary> {
    use CshAstTestUnary::*;
    Some(match text {
        "-a" | "-e" => Exists,
        "-b" => Block,
        "-c" => Character,
        "-d" => Directory,
        "-f" => Regular,
        "-g" => SetGroupId,
        "-h" | "-L" => Symlink,
        "-k" => Sticky,
        "-p" => Fifo,
        "-r" => Readable,
        "-s" => NonemptyFile,
        "-t" => Terminal,
        "-u" => SetUserId,
        "-w" => Writable,
        "-x" => Executable,
        "-O" => OwnedByUser,
        "-G" => OwnedByGroup,
        "-N" => ModifiedSinceRead,
        "-S" => Socket,
        "-o" => OptionEnabled,
        "-v" => VariableSet,
        "-R" => NameReference,
        "-z" => EmptyString,
        "-n" => NonemptyString,
        _ => return None,
    })
}
