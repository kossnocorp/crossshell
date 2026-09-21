use super::*;

impl<'a> Cursor<'a> {
    pub(super) fn brace_word(&mut self) -> Parsed<'a, CshAstWord<'a>> {
        let start = self.pos;
        self.pos += 1;
        let mut alternatives = Vec::new();
        let mut parts = Vec::new();
        let mut closed = false;
        while let Some(b) = self.byte() {
            if matches!(
                b,
                b' ' | b'\t' | b'\r' | b'\n' | b';' | b'|' | b'&' | b'<' | b'>' | b'(' | b')'
            ) {
                break;
            }
            if b == b'}' {
                self.pos += 1;
                closed = true;
                break;
            }
            if b == b',' {
                self.pos += 1;
                alternatives.push(CshAstWord::concat(std::mem::take(&mut parts)));
            } else {
                if let Some(tilde) = self.tilde(Some(b',')) {
                    parts.push(tilde);
                    continue;
                }
                parts.push(self.word_part(false, self.source.len())?);
            }
        }
        alternatives.push(CshAstWord::concat(parts));
        if closed && alternatives.len() > 1 {
            return Ok(CshAstWord::BraceAlternatives(alternatives));
        }
        if closed && let Some(sequence) = brace_sequence(&self.source[start + 1..self.pos - 1]) {
            return Ok(CshAstWord::BraceSequence(sequence));
        }
        let mut literal = vec![CshAstWord::Literal("{".into())];
        for (i, word) in alternatives.into_iter().enumerate() {
            if i > 0 {
                literal.push(CshAstWord::Literal(",".into()));
            }
            literal.push(word);
        }
        if closed {
            literal.push(CshAstWord::Literal("}".into()));
        }
        Ok(CshAstWord::concat(literal))
    }

    pub(super) fn tilde(&mut self, stop: Option<u8>) -> Option<CshAstWord<'a>> {
        if self.byte() != Some(b'~') {
            return None;
        }
        let from = self.pos + 1;
        let mut end = from;
        for c in self.source[from..].chars() {
            if c == '/'
                || c == ':'
                || c.is_ascii_whitespace()
                || ";|&<>()".contains(c)
                || stop.is_some_and(|stop| c == stop as char || c == '}')
            {
                break;
            }
            if "\"'\\$`{}*?[".contains(c) {
                return None;
            }
            end += c.len_utf8();
        }
        let user = &self.source[from..end];
        use CshAstTilde as T;
        let kind = match user {
            "" => T::Home,
            "+" => T::WorkingDirectory,
            "-" => T::PreviousDirectory,
            _ if user
                .trim_start_matches(['+', '-'])
                .bytes()
                .all(|b| b.is_ascii_digit()) =>
            {
                T::DirectoryStack {
                    index: user.trim_start_matches(['+', '-']),
                    reverse: user.starts_with('-'),
                    explicit_sign: user.starts_with(['+', '-']),
                }
            }
            _ => T::User(user),
        };
        self.pos = end;
        Some(CshAstWord::Tilde(kind))
    }
}

fn brace_sequence(text: &str) -> Option<CshAstBraceSequence<'_>> {
    let pieces: Vec<_> = text.split("..").collect();
    if !(2..=3).contains(&pieces.len()) {
        return None;
    }
    let integer = |s: &str| {
        !s.trim_start_matches('-').is_empty()
            && s.trim_start_matches('-')
                .bytes()
                .all(|b| b.is_ascii_digit())
    };
    let alphabetic = pieces[0].len() == 1
        && pieces[1].len() == 1
        && pieces[..2]
            .iter()
            .all(|s| s.as_bytes()[0].is_ascii_alphabetic());
    if !alphabetic && !pieces[..2].iter().all(|s| integer(s)) {
        return None;
    }
    if pieces.len() == 3 && !integer(pieces[2]) {
        return None;
    }
    let padding = if !alphabetic
        && pieces[..2].iter().any(|s| {
            s.trim_start_matches('-').len() > 1 && s.trim_start_matches('-').starts_with('0')
        }) {
        pieces[..2].iter().map(|s| s.len()).max().unwrap()
    } else {
        0
    };
    Some(CshAstBraceSequence {
        start: pieces[0],
        end: pieces[1],
        step: pieces.get(2).copied(),
        alphabetic,
        padding,
    })
}
