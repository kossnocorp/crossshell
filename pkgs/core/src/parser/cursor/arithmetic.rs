use super::*;

impl<'a> Cursor<'a> {
    pub(super) fn arithmetic_space(&mut self) {
        loop {
            self.space();
            if !matches!(self.byte(), Some(b'\n' | b'\x0b' | b'\x0c')) {
                break;
            }
            self.pos += 1;
        }
    }

    pub(super) fn arithmetic_expression(&mut self, end: usize) -> Parsed<'a, CshAstArithmetic<'a>> {
        self.arithmetic_bp(end, 0)
    }

    fn arithmetic_bp(&mut self, end: usize, min: u8) -> Parsed<'a, CshAstArithmetic<'a>> {
        if self.depth >= 128 {
            return Err(self.expected("less deeply nested arithmetic"));
        }
        self.depth += 1;
        let result = self.arithmetic_bp_inner(end, min);
        self.depth -= 1;
        result
    }

    fn arithmetic_bp_inner(&mut self, end: usize, min: u8) -> Parsed<'a, CshAstArithmetic<'a>> {
        use CshAstArithmeticKind as K;
        use CshAstArithmeticUnary as U;
        self.arithmetic_space();
        let start = self.pos;
        if start >= end {
            return Err(self.expected("an arithmetic operand"));
        }
        let unary = [
            ("++", U::PreIncrement),
            ("--", U::PreDecrement),
            ("+", U::Plus),
            ("-", U::Minus),
            ("!", U::Not),
            ("~", U::BitNot),
        ]
        .into_iter()
        .find(|(s, _)| self.rest().starts_with(s));
        let kind = if let Some((text, operator)) = unary {
            self.pos += text.len();
            let operand = self.arithmetic_bp(end, 17)?;
            if matches!(operator, U::PreIncrement | U::PreDecrement) && !arithmetic_lvalue(&operand)
            {
                return Err(self.expected("an arithmetic variable"));
            }
            K::Unary {
                operator,
                operand: Box::new(operand),
            }
        } else if self.eat(b'(') {
            let inner = self.arithmetic_bp(end, 0)?;
            self.arithmetic_space();
            self.require_close_paren()?;
            K::Group(Box::new(inner))
        } else {
            let mut parts = Vec::new();
            while self.pos < end {
                match self.byte() {
                    Some(b'$' | b'`' | b'\'' | b'"' | b'\\') => {
                        parts.push(self.word_part(false, end)?)
                    }
                    Some(b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'#' | b'@') => {
                        let from = self.pos;
                        while self.pos < end
                            && matches!(
                                self.byte(),
                                Some(b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'#' | b'@')
                            )
                        {
                            self.pos += 1;
                        }
                        parts.push(CshAstWord::Literal(self.source[from..self.pos].into()));
                    }
                    _ => break,
                }
            }
            if parts.is_empty() {
                return Err(self.expected("an arithmetic operand"));
            }
            match CshAstWord::concat(parts) {
                CshAstWord::Literal(text) if text.as_bytes()[0].is_ascii_digit() => {
                    let (radix, digits) = arithmetic_number(text)
                        .ok_or_else(|| self.expected("a valid base-2 through base-64 integer"))?;
                    K::Number { radix, digits }
                }
                CshAstWord::Literal(text) => {
                    if !text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
                        return Err(self.expected("an arithmetic variable name"));
                    }
                    K::Variable(text)
                }
                word => K::Expansion(Box::new(word)),
            }
        };
        let mut left = CshAstArithmetic {
            span: start..self.pos,
            kind,
        };
        let mut chain = 0;
        loop {
            self.arithmetic_space();
            if self.pos >= end {
                break;
            }
            chain += 1;
            if chain > 128 {
                return Err(self.expected("a shorter arithmetic operator chain"));
            }
            if self.byte() == Some(b'[') && min <= 17 {
                self.pos += 1;
                let index = self.arithmetic_bp(end, 0)?;
                self.arithmetic_space();
                if !self.eat(b']') {
                    return Err(self.expected("]"));
                }
                left = CshAstArithmetic {
                    span: start..self.pos,
                    kind: K::Subscript {
                        array: Box::new(left),
                        index: Box::new(index),
                    },
                };
                continue;
            }
            if min <= 17 && (self.rest().starts_with("++") || self.rest().starts_with("--")) {
                if !arithmetic_lvalue(&left) {
                    return Err(self.expected("an arithmetic variable"));
                }
                let operator = if self.byte() == Some(b'+') {
                    U::PostIncrement
                } else {
                    U::PostDecrement
                };
                self.pos += 2;
                left = CshAstArithmetic {
                    span: start..self.pos,
                    kind: K::Unary {
                        operator,
                        operand: Box::new(left),
                    },
                };
                continue;
            }
            if min <= 3 && self.eat(b'?') {
                let then_value = self.arithmetic_bp(end, 0)?;
                self.arithmetic_space();
                if !self.eat(b':') {
                    return Err(self.expected(":"));
                }
                let else_value = self.arithmetic_bp(end, 3)?;
                left = CshAstArithmetic {
                    span: start..self.pos,
                    kind: K::Conditional {
                        condition: Box::new(left),
                        then_value: Box::new(then_value),
                        else_value: Box::new(else_value),
                    },
                };
                continue;
            }
            let Some((text, operator, precedence, right_associative)) =
                arithmetic_operator(self.rest())
            else {
                break;
            };
            if precedence < min {
                break;
            }
            if precedence == 2 && !arithmetic_lvalue(&left) {
                return Err(self.expected("an arithmetic assignment target"));
            }
            self.pos += text.len();
            let right = self.arithmetic_bp(end, precedence + u8::from(!right_associative))?;
            left = CshAstArithmetic {
                span: start..self.pos,
                kind: K::Binary {
                    left: Box::new(left),
                    operator,
                    right: Box::new(right),
                },
            };
        }
        // Pratt recursion alone does not bound the height of left-associated
        // trees. Bound their actual height too, including parenthesized chains.
        let mut pending = vec![(&left, 0)];
        while let Some((node, depth)) = pending.pop() {
            if depth >= 128 {
                return Err(self.expected("a shallower arithmetic tree"));
            }
            match &node.kind {
                K::Unary { operand, .. } | K::Group(operand) => pending.push((operand, depth + 1)),
                K::Subscript {
                    array: left,
                    index: right,
                }
                | K::Binary { left, right, .. } => {
                    pending.push((left, depth + 1));
                    pending.push((right, depth + 1));
                }
                K::Conditional {
                    condition,
                    then_value,
                    else_value,
                } => {
                    pending.push((condition, depth + 1));
                    pending.push((then_value, depth + 1));
                    pending.push((else_value, depth + 1));
                }
                _ => {}
            }
        }
        Ok(left)
    }

    pub(super) fn arithmetic_command(&mut self) -> Parsed<'a, CshAstArithmetic<'a>> {
        self.pos += 2;
        self.arithmetic_space();
        let expression = if self.rest().starts_with("))") {
            CshAstArithmetic {
                span: self.pos..self.pos,
                kind: CshAstArithmeticKind::Number {
                    radix: 10,
                    digits: "0".into(),
                },
            }
        } else {
            self.arithmetic_expression(self.source.len())?
        };
        self.arithmetic_space();
        self.require_close_paren()?;
        self.require_close_paren()?;
        Ok(expression)
    }

    pub(super) fn arithmetic_for(&mut self) -> Parsed<'a, CshAstArithmeticFor<'a>> {
        self.pos += 2;
        let mut clause = |delimiter| -> Parsed<'a, Option<CshAstArithmetic<'a>>> {
            self.arithmetic_space();
            let value = if self.byte() == Some(delimiter) {
                None
            } else {
                Some(self.arithmetic_expression(self.source.len())?)
            };
            self.arithmetic_space();
            if !self.eat(delimiter) {
                return Err(self.expected("an arithmetic-for clause delimiter"));
            }
            Ok(value)
        };
        let init = clause(b';')?;
        let condition = clause(b';')?;
        let update = clause(b')')?;
        self.require_close_paren()?;
        Ok(CshAstArithmeticFor {
            init,
            condition,
            update,
        })
    }
}

fn arithmetic_lvalue(value: &CshAstArithmetic) -> bool {
    if let CshAstArithmeticKind::Group(inner) = &value.kind {
        return arithmetic_lvalue(inner);
    }
    matches!(
        value.kind,
        CshAstArithmeticKind::Variable(_)
            | CshAstArithmeticKind::Subscript { .. }
            | CshAstArithmeticKind::Expansion(_)
    )
}

fn arithmetic_number(text: Cow<'_, str>) -> Option<(u32, Cow<'_, str>)> {
    let (radix, digits) = if let Some((base, digits)) = text.split_once('#') {
        (base.parse().ok()?, digits)
    } else if text.starts_with("0x") || text.starts_with("0X") {
        (16, &text[2..])
    } else if text.len() > 1 && text.starts_with('0') {
        (8, text.as_ref())
    } else {
        (10, text.as_ref())
    };
    if !(2..=64).contains(&radix) || digits.is_empty() {
        return None;
    }
    for c in digits.chars() {
        let digit = match c {
            '0'..='9' => c as u32 - '0' as u32,
            'a'..='z' => c as u32 - 'a' as u32 + 10,
            'A'..='Z' => c as u32 - 'A' as u32 + if radix <= 36 { 10 } else { 36 },
            '@' => 62,
            '_' => 63,
            _ => return None,
        };
        if digit >= radix {
            return None;
        }
    }
    let offset = text.len() - digits.len();
    let digits = match text {
        Cow::Borrowed(text) => Cow::Borrowed(&text[offset..]),
        Cow::Owned(mut text) => {
            text.drain(..offset);
            Cow::Owned(text)
        }
    };
    Some((radix, digits))
}

fn arithmetic_operator(source: &str) -> Option<(&'static str, CshAstArithmeticBinary, u8, bool)> {
    use CshAstArithmeticBinary::*;
    [
        ("<<=", ShiftLeftAssign, 2, true),
        (">>=", ShiftRightAssign, 2, true),
        ("+=", AddAssign, 2, true),
        ("-=", SubtractAssign, 2, true),
        ("*=", MultiplyAssign, 2, true),
        ("/=", DivideAssign, 2, true),
        ("%=", RemainderAssign, 2, true),
        ("&=", BitAndAssign, 2, true),
        ("^=", BitXorAssign, 2, true),
        ("|=", BitOrAssign, 2, true),
        ("||", Or, 4, false),
        ("&&", And, 5, false),
        ("==", Equal, 9, false),
        ("!=", NotEqual, 9, false),
        ("<=", LessEqual, 10, false),
        (">=", GreaterEqual, 10, false),
        ("<<", ShiftLeft, 11, false),
        (">>", ShiftRight, 11, false),
        ("**", Power, 16, true),
        (",", Comma, 1, false),
        ("=", Assign, 2, true),
        ("|", BitOr, 6, false),
        ("^", BitXor, 7, false),
        ("&", BitAnd, 8, false),
        ("<", Less, 10, false),
        (">", Greater, 10, false),
        ("+", Add, 12, false),
        ("-", Subtract, 12, false),
        ("*", Multiply, 13, false),
        ("/", Divide, 13, false),
        ("%", Remainder, 13, false),
    ]
    .into_iter()
    .find(|(text, _, _, _)| source.starts_with(text))
}
