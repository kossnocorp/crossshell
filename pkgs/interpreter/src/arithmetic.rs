use crate::{eval::Evaluator, io::Io};
use anyhow::{Result, bail};
use crossshell::*;

impl Evaluator<'_, '_, '_> {
    pub fn arithmetic(&mut self, a: &CshAstArithmetic<'_>, io: &Io) -> Result<i64> {
        self.arithmetic_inner(a, io, 0, true)
    }

    fn arithmetic_text(&mut self, text: &str, io: &Io, depth: usize) -> Result<i64> {
        if text.trim().is_empty() {
            return Ok(0);
        }
        if depth > 64 {
            bail!("Arithmetic variable recursion exceeds 64 levels");
        }
        let source = format!("(({text}))");
        let ast = CshParser::parse(&source)
            .map_err(|e| anyhow::anyhow!("Invalid arithmetic value: {e}"))?;
        if ast.commands.len() != 1 {
            bail!("Invalid arithmetic value");
        }
        let CshAstExpression::Arithmetic(a) = &ast[ast.commands[0]] else {
            bail!("Invalid arithmetic value");
        };
        // Variable values may contain arithmetic syntax, but may not introduce
        // command-substitution node IDs from a different owning AST.
        self.arithmetic_inner(a, io, depth + 1, false)
    }

    fn arithmetic_inner(
        &mut self,
        a: &CshAstArithmetic<'_>,
        io: &Io,
        depth: usize,
        expansions: bool,
    ) -> Result<i64> {
        use CshAstArithmeticBinary as B;
        use CshAstArithmeticKind as A;
        use CshAstArithmeticUnary as U;
        Ok(match &a.kind {
            A::Group(a) => self.arithmetic_inner(a, io, depth, expansions)?,
            A::Number { radix, digits } => {
                let mut value = 0i64;
                for c in digits.chars() {
                    let digit = if *radix <= 36 {
                        c.to_digit(*radix)
                    } else {
                        "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ@_"
                            .find(c)
                            .map(|i| i as u32)
                            .filter(|i| *i < *radix)
                    }
                    .ok_or_else(|| anyhow::anyhow!("Invalid base-{radix} digit: {c}"))?;
                    value = value
                        .wrapping_mul(i64::from(*radix))
                        .wrapping_add(i64::from(digit));
                }
                value
            }
            A::Variable(n) => {
                let value = self.value(n)?;
                if value.is_none() && self.nounset {
                    bail!("{n}: unbound variable");
                }
                self.arithmetic_text(value.as_deref().unwrap_or_default(), io, depth + 1)?
            }
            A::Expansion(w) => {
                if !expansions {
                    bail!("Shell expansions inside arithmetic variable values are not supported");
                }
                let value = self.scalar(w, io)?;
                self.arithmetic_text(&value, io, depth + 1)?
            }
            A::Subscript { .. } => bail!("Unsupported arithmetic: array subscript"),
            A::Unary { operator, operand } => {
                let value = self.arithmetic_inner(operand, io, depth, expansions)?;
                match operator {
                    U::Plus => value,
                    U::Minus => value.wrapping_neg(),
                    U::Not => i64::from(value == 0),
                    U::BitNot => !value,
                    U::PreIncrement | U::PreDecrement | U::PostIncrement | U::PostDecrement => {
                        let name = lvalue(operand)?;
                        let next = if matches!(operator, U::PreIncrement | U::PostIncrement) {
                            value.wrapping_add(1)
                        } else {
                            value.wrapping_sub(1)
                        };
                        self.set(name, next.to_string());
                        if matches!(operator, U::PreIncrement | U::PreDecrement) {
                            next
                        } else {
                            value
                        }
                    }
                }
            }
            A::Conditional {
                condition,
                then_value,
                else_value,
            } => {
                let branch = if self.arithmetic_inner(condition, io, depth, expansions)? != 0 {
                    then_value
                } else {
                    else_value
                };
                self.arithmetic_inner(branch, io, depth, expansions)?
            }
            A::Binary {
                left,
                operator,
                right,
            } => {
                if *operator == B::Assign {
                    let name = lvalue(left)?;
                    let value = self.arithmetic_inner(right, io, depth, expansions)?;
                    self.set(name, value.to_string());
                    return Ok(value);
                }
                let lhs = self.arithmetic_inner(left, io, depth, expansions)?;
                if *operator == B::And && lhs == 0 {
                    return Ok(0);
                }
                if *operator == B::Or && lhs != 0 {
                    return Ok(1);
                }
                let rhs = self.arithmetic_inner(right, io, depth, expansions)?;
                let (operator, assign) = match operator {
                    B::AddAssign => (B::Add, true),
                    B::SubtractAssign => (B::Subtract, true),
                    B::MultiplyAssign => (B::Multiply, true),
                    B::DivideAssign => (B::Divide, true),
                    B::RemainderAssign => (B::Remainder, true),
                    B::ShiftLeftAssign => (B::ShiftLeft, true),
                    B::ShiftRightAssign => (B::ShiftRight, true),
                    B::BitAndAssign => (B::BitAnd, true),
                    B::BitXorAssign => (B::BitXor, true),
                    B::BitOrAssign => (B::BitOr, true),
                    op => (*op, false),
                };
                let value = match operator {
                    B::Comma => rhs,
                    B::Add => lhs.wrapping_add(rhs),
                    B::Subtract => lhs.wrapping_sub(rhs),
                    B::Multiply => lhs.wrapping_mul(rhs),
                    B::Divide => {
                        if rhs == 0 {
                            bail!("Division by zero");
                        }
                        lhs.wrapping_div(rhs)
                    }
                    B::Remainder => {
                        if rhs == 0 {
                            bail!("Division by zero");
                        }
                        lhs.wrapping_rem(rhs)
                    }
                    B::Power => {
                        if rhs < 0 {
                            bail!("Negative exponent");
                        }
                        let mut base = lhs;
                        let mut exponent = rhs as u64;
                        let mut result = 1i64;
                        while exponent > 0 {
                            if exponent & 1 != 0 {
                                result = result.wrapping_mul(base);
                            }
                            base = base.wrapping_mul(base);
                            exponent >>= 1;
                        }
                        result
                    }
                    B::ShiftLeft => lhs.wrapping_shl(rhs as u32),
                    B::ShiftRight => lhs.wrapping_shr(rhs as u32),
                    B::BitAnd => lhs & rhs,
                    B::BitOr => lhs | rhs,
                    B::BitXor => lhs ^ rhs,
                    B::And | B::Or => i64::from(rhs != 0),
                    B::Equal => i64::from(lhs == rhs),
                    B::NotEqual => i64::from(lhs != rhs),
                    B::Less => i64::from(lhs < rhs),
                    B::LessEqual => i64::from(lhs <= rhs),
                    B::Greater => i64::from(lhs > rhs),
                    B::GreaterEqual => i64::from(lhs >= rhs),
                    _ => unreachable!("assignments handled above"),
                };
                if assign {
                    self.set(lvalue(left)?, value.to_string());
                }
                value
            }
        })
    }

    pub fn condition(&mut self, condition: &CshAstCondition<'_>, io: &Io) -> Result<bool> {
        use CshAstConditionKind as C;
        use CshAstTestBinary as B;
        use CshAstTestUnary as U;
        Ok(match &condition.kind {
            C::Word(w) => !self.scalar(w, io)?.is_empty(),
            C::Not(c) => !self.condition(c, io)?,
            C::And(a, b) => self.condition(a, io)? && self.condition(b, io)?,
            C::Or(a, b) => self.condition(a, io)? || self.condition(b, io)?,
            C::Unary { operator, operand } => {
                let value = self.scalar(operand, io)?;
                match operator {
                    U::EmptyString => value.is_empty(),
                    U::NonemptyString => !value.is_empty(),
                    U::VariableSet => self.variables.contains_key(&value),
                    U::OptionEnabled => match value.as_str() {
                        "errexit" => self.errexit,
                        "nounset" => self.nounset,
                        "noglob" => self.noglob,
                        "pipefail" => self.pipefail,
                        _ => false,
                    },
                    U::NameReference => bail!("Unsupported test: name reference"),
                    op => {
                        let flag = match op {
                            U::Exists => "-e",
                            U::Block => "-b",
                            U::Character => "-c",
                            U::Directory => "-d",
                            U::Regular => "-f",
                            U::SetGroupId => "-g",
                            U::Symlink => "-L",
                            U::Sticky => "-k",
                            U::Fifo => "-p",
                            U::Readable => "-r",
                            U::NonemptyFile => "-s",
                            U::Terminal => "-t",
                            U::SetUserId => "-u",
                            U::Writable => "-w",
                            U::Executable => "-x",
                            U::OwnedByUser => "-O",
                            U::OwnedByGroup => "-G",
                            U::ModifiedSinceRead => "-N",
                            U::Socket => "-S",
                            _ => unreachable!(),
                        };
                        self.external(&["test".into(), flag.into(), value], io)? == 0
                    }
                }
            }
            C::Binary {
                left,
                operator,
                right,
            } => {
                let lhs = self.scalar(left, io)?;
                match operator {
                    B::PatternEqual | B::PatternNotEqual => {
                        let pattern = self.pattern(right, io)?;
                        crate::expand::matches(&pattern, &lhs)? == (*operator == B::PatternEqual)
                    }
                    B::Regex => regex::Regex::new(&self.regex_pattern(right, io)?)?.is_match(&lhs),
                    B::StringLess => lhs < self.scalar(right, io)?,
                    B::StringGreater => lhs > self.scalar(right, io)?,
                    B::Newer | B::Older | B::SameFile => {
                        let rhs = self.scalar(right, io)?;
                        let flag = match operator {
                            B::Newer => "-nt",
                            B::Older => "-ot",
                            _ => "-ef",
                        };
                        self.external(&["test".into(), lhs, flag.into(), rhs], io)? == 0
                    }
                    _ => {
                        let rhs = self.scalar(right, io)?;
                        let lhs = self.arithmetic_text(&lhs, io, 0)?;
                        let rhs = self.arithmetic_text(&rhs, io, 0)?;
                        match operator {
                            B::Equal => lhs == rhs,
                            B::NotEqual => lhs != rhs,
                            B::Less => lhs < rhs,
                            B::LessEqual => lhs <= rhs,
                            B::Greater => lhs > rhs,
                            B::GreaterEqual => lhs >= rhs,
                            _ => unreachable!(),
                        }
                    }
                }
            }
        })
    }

    fn regex_pattern(&mut self, word: &CshAstWord<'_>, io: &Io) -> Result<String> {
        match word {
            CshAstWord::SingleQuoted(_)
            | CshAstWord::DoubleQuoted(_)
            | CshAstWord::Escaped(_)
            | CshAstWord::AnsiCQuoted(_)
            | CshAstWord::LocaleQuoted(_) => Ok(regex::escape(&self.scalar(word, io)?)),
            CshAstWord::Concat(words) => {
                let mut s = String::new();
                for w in words {
                    s.push_str(&self.regex_pattern(w, io)?);
                }
                Ok(s)
            }
            _ => self.scalar(word, io),
        }
    }
}

fn lvalue<'a>(a: &'a CshAstArithmetic<'_>) -> Result<&'a str> {
    match &a.kind {
        CshAstArithmeticKind::Variable(n) => Ok(n),
        CshAstArithmeticKind::Group(a) => lvalue(a),
        _ => bail!("Invalid arithmetic assignment target"),
    }
}
