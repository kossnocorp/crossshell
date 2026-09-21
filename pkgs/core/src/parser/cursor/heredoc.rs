use super::*;

impl<'a> Cursor<'a> {
    pub(super) fn heredoc_word(
        &mut self,
        end: usize,
        strip_tabs: bool,
    ) -> Parsed<'a, CshAstWord<'a>> {
        let mut parts = Vec::new();
        let mut line_start = true;
        while self.pos < end {
            if strip_tabs && line_start {
                while self.pos < end && self.byte() == Some(b'\t') {
                    self.pos += 1;
                }
                if self.pos == end {
                    break;
                }
            }
            line_start = false;
            match self.byte().unwrap() {
                b'$' | b'`' => parts.push(self.word_part(true, end)?),
                b'\\' if matches!(self.peek(1), Some(b'$' | b'`' | b'\\' | b'\n')) => {
                    self.pos += 1;
                    let b = self.byte().unwrap();
                    self.pos += 1;
                    if b == b'\n' {
                        line_start = true;
                    } else {
                        parts.push(CshAstWord::Escaped(&self.source[self.pos - 1..self.pos]));
                    }
                }
                _ => {
                    let c = self.rest().chars().next().unwrap();
                    self.pos += c.len_utf8();
                    line_start = c == '\n';
                    match parts.last_mut() {
                        Some(CshAstWord::Literal(text)) => text.to_mut().push(c),
                        _ => parts.push(CshAstWord::Literal(
                            self.source[self.pos - c.len_utf8()..self.pos].into(),
                        )),
                    }
                }
            }
            if self.pos > end {
                return Err(self.expected("an expansion within the here-document body"));
            }
        }
        Ok(CshAstWord::concat(parts))
    }
}
