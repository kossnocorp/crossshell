use super::*;

impl<'a> Cursor<'a> {
    pub(super) fn extended_glob(&mut self) -> Parsed<'a, CshAstWord<'a>> {
        let operator = self.byte().unwrap() as char;
        self.pos += 2;
        let mut alternatives = Vec::new();
        let mut parts = Vec::new();
        loop {
            match self.byte() {
                None => return Err(self.expected("a closing extended-glob delimiter")),
                Some(b'|' | b')') => {
                    alternatives.push(CshAstWord::concat(std::mem::take(&mut parts)));
                    let close = self.eat(b')');
                    if close {
                        break;
                    }
                    self.pos += 1;
                }
                Some(b'$' | b'`' | b'\'' | b'"' | b'\\' | b'*' | b'?' | b'[') => {
                    parts.push(self.word_part(false, self.source.len())?);
                }
                Some(b'+' | b'@' | b'!') if self.peek(1) == Some(b'(') => {
                    parts.push(self.word_part(false, self.source.len())?);
                }
                _ => {
                    let start = self.pos;
                    loop {
                        self.pos += self.rest().chars().next().unwrap().len_utf8();
                        if matches!(
                            self.byte(),
                            None | Some(
                                b'|' | b')'
                                    | b'$'
                                    | b'`'
                                    | b'\''
                                    | b'"'
                                    | b'\\'
                                    | b'*'
                                    | b'?'
                                    | b'['
                            )
                        ) || (matches!(self.byte(), Some(b'+' | b'@' | b'!'))
                            && self.peek(1) == Some(b'('))
                        {
                            break;
                        }
                    }
                    parts.push(CshAstWord::Literal(self.source[start..self.pos].into()));
                }
            }
        }
        Ok(CshAstWord::ExtendedGlob {
            operator,
            alternatives,
        })
    }

    pub(super) fn glob_class(&mut self, end: usize) -> Parsed<'a, CshAstWord<'a>> {
        use CshAstGlobClassItem as I;
        let opening = self.pos;
        self.pos += 1;
        let negated = self.eat(b'!') || self.eat(b'^');
        let mut items = Vec::new();
        let mut hyphens = Vec::new();
        while self.pos < end {
            if self.byte() == Some(b']') && !items.is_empty() {
                self.pos += 1;
                let mut ranges = Vec::new();
                let mut i = 0;
                while i < items.len() {
                    if i + 2 < items.len()
                        && hyphens[i + 1]
                        && let (I::Character(start), I::Character(end)) = (&items[i], &items[i + 2])
                    {
                        ranges.push(I::Range {
                            start: *start,
                            end: *end,
                        });
                        i += 3;
                    } else {
                        ranges.push(items[i].clone());
                        i += 1;
                    }
                }
                return Ok(CshAstWord::Glob(CshAstGlob::CharacterClass {
                    negated,
                    items: ranges,
                }));
            }
            if self.byte() == Some(b'[') && matches!(self.peek(1), Some(b':' | b'.' | b'=')) {
                let marker = self.peek(1).unwrap();
                let start = self.pos + 2;
                let closing = self.source.as_bytes()[start..end]
                    .windows(2)
                    .position(|w| w == [marker, b']']);
                if let Some(len) = closing {
                    let name = &self.source[start..start + len];
                    items.push(match marker {
                        b':' => I::NamedClass(name),
                        b'.' => I::CollatingSymbol(name),
                        _ => I::EquivalenceClass(name),
                    });
                    hyphens.push(false);
                    self.pos = start + len + 2;
                    continue;
                }
            }
            let escaped = self.eat(b'\\');
            let Some(c) = self.rest().chars().next() else {
                break;
            };
            if !escaped && (c.is_whitespace() || "\"'$`;|&()<>".contains(c)) {
                break;
            }
            self.pos += c.len_utf8();
            items.push(I::Character(c));
            hyphens.push(c == '-' && !escaped);
        }
        // An unmatched opening bracket is an ordinary shell character.
        self.pos = opening + 1;
        Ok(CshAstWord::Literal("[".into()))
    }
}
