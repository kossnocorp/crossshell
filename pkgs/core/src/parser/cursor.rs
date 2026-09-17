use crate::prelude::internal::*;
mod arithmetic;
mod brace;
mod condition;
mod glob;
mod heredoc;
mod parameter;
mod word;

type Parsed<'a, T> = Result<T, CshError<'a>>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Keyword {
    If,
    Then,
    Elif,
    Else,
    Fi,
    For,
    In,
    Do,
    Done,
    While,
    Until,
    Case,
    Esac,
    OpenBrace,
    CloseBrace,
    Bang,
}

impl Keyword {
    fn text(self) -> &'static str {
        match self {
            Self::If => "if",
            Self::Then => "then",
            Self::Elif => "elif",
            Self::Else => "else",
            Self::Fi => "fi",
            Self::For => "for",
            Self::In => "in",
            Self::Do => "do",
            Self::Done => "done",
            Self::While => "while",
            Self::Until => "until",
            Self::Case => "case",
            Self::Esac => "esac",
            Self::OpenBrace => "{",
            Self::CloseBrace => "}",
            Self::Bang => "!",
        }
    }
}

/// The grammar determines the terminators of a command list. No string search
/// or keyword classification is needed for top-level and parenthesized lists.
#[derive(Clone, Copy)]
enum Stop {
    Eof,
    Backtick,
    Subshell,
    Group,
    Then,
    Branch,
    Fi,
    Do,
    Done,
    Case,
}

// Classify an unquoted byte with one lookup instead of searching several sets
// for every character. High bytes take the UTF-8 path so Unicode whitespace
// retains exactly the same word-boundary semantics.
const BARE: u8 = 0;
const SPECIAL: u8 = 1;
const GLOB: u8 = 2;
const UNICODE: u8 = 3;
const WORD_CLASS: [u8; 256] = {
    let mut table = [BARE; 256];
    let special = b" \t\r\n\x0b\x0c\"';|&<>()$`\\";
    let mut i = 0;
    while i < special.len() {
        table[special[i] as usize] = SPECIAL;
        i += 1;
    }
    let glob = b"?*+@!";
    i = 0;
    while i < glob.len() {
        table[glob[i] as usize] = GLOB;
        i += 1;
    }
    i = 128;
    while i < table.len() {
        table[i] = UNICODE;
        i += 1;
    }
    table
};

/// Single-pass parser. ASCII syntax is dispatched directly from the source;
/// UTF-8 is decoded only for escapes, non-ASCII word characters, and diagnostics.
pub(super) struct Cursor<'a> {
    source: &'a str,
    pos: usize,
    nodes: Vec<CshAstExpression>,
    spans: Vec<Range<usize>>,
    depth: usize,
    here_documents: Vec<CshAstHereDocument>,
    pending_here_documents: Vec<usize>,
    backtick: bool,
}

impl<'a> Cursor<'a> {
    pub(super) fn parse(source: &'a str) -> Parsed<'a, CshAst> {
        let mut cursor = Self {
            source,
            pos: 0,
            nodes: Vec::new(),
            spans: Vec::new(),
            depth: 0,
            here_documents: Vec::new(),
            pending_here_documents: Vec::new(),
            backtick: false,
        };
        let commands = cursor.list(Stop::Eof, false)?;
        if !cursor.pending_here_documents.is_empty() {
            return Err(cursor.expected("a here-document body"));
        }
        if cursor.pos != source.len() {
            return Err(cursor.expected("end of file"));
        }
        Ok(CshAst {
            commands,
            nodes: cursor.nodes,
            spans: cursor.spans,
            here_documents: cursor.here_documents,
        })
    }

    fn rest(&self) -> &'a str {
        &self.source[self.pos..]
    }

    fn byte(&self) -> Option<u8> {
        self.source.as_bytes().get(self.pos).copied()
    }

    fn peek(&self, offset: usize) -> Option<u8> {
        self.source.as_bytes().get(self.pos + offset).copied()
    }

    fn eat(&mut self, byte: u8) -> bool {
        if self.byte() == Some(byte) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expected(&self, expected: &'static str) -> CshError<'a> {
        let end = self.pos + self.rest().chars().next().map_or(0, char::len_utf8);
        CshError::Unexpected {
            found: &self.source[self.pos..end],
            span: self.pos..end,
            expected,
        }
    }

    fn require_close_paren(&mut self) -> Parsed<'a, ()> {
        if self.eat(b')') {
            Ok(())
        } else {
            Err(self.expected(")"))
        }
    }

    fn keyword(&self) -> Option<Keyword> {
        use Keyword::*;
        let (keyword, rest) = match &self.source.as_bytes()[self.pos..] {
            [b'i', b'f', rest @ ..] => (If, rest),
            [b'i', b'n', rest @ ..] => (In, rest),
            [b't', b'h', b'e', b'n', rest @ ..] => (Then, rest),
            [b'e', b'l', b'i', b'f', rest @ ..] => (Elif, rest),
            [b'e', b'l', b's', b'e', rest @ ..] => (Else, rest),
            [b'e', b's', b'a', b'c', rest @ ..] => (Esac, rest),
            [b'f', b'i', rest @ ..] => (Fi, rest),
            [b'f', b'o', b'r', rest @ ..] => (For, rest),
            [b'd', b'o', b'n', b'e', rest @ ..] => (Done, rest),
            [b'd', b'o', rest @ ..] => (Do, rest),
            [b'w', b'h', b'i', b'l', b'e', rest @ ..] => (While, rest),
            [b'u', b'n', b't', b'i', b'l', rest @ ..] => (Until, rest),
            [b'c', b'a', b's', b'e', rest @ ..] => (Case, rest),
            [b'{', rest @ ..] => (OpenBrace, rest),
            [b'}', rest @ ..] => (CloseBrace, rest),
            [b'!', rest @ ..] => (Bang, rest),
            _ => return None,
        };
        let boundary = match rest.first() {
            None
            | Some(
                b' ' | b'\t' | b'\r' | b'\n' | 0x0b | 0x0c | b';' | b'|' | b'&' | b'(' | b')'
                | b'<' | b'>',
            ) => true,
            Some(128..=255) => self.source[self.source.len() - rest.len()..]
                .chars()
                .next()
                .unwrap()
                .is_whitespace(),
            _ => false,
        };
        boundary.then_some(keyword)
    }

    fn eat_keyword(&mut self, keyword: Keyword) -> bool {
        if self.keyword() == Some(keyword) {
            self.pos += keyword.text().len();
            true
        } else {
            false
        }
    }

    fn require_keyword(&mut self, keyword: Keyword) -> Parsed<'a, ()> {
        if self.eat_keyword(keyword) {
            Ok(())
        } else {
            Err(self.expected(keyword.text()))
        }
    }

    fn space(&mut self) {
        loop {
            match self.byte() {
                Some(b' ' | b'\t' | b'\r') => self.pos += 1,
                Some(b'\\') if self.peek(1) == Some(b'\n') => self.pos += 2,
                _ => break,
            }
        }
    }

    fn comment(&mut self) {
        if self.byte() == Some(b'#') {
            self.pos += self.rest().find('\n').unwrap_or(self.rest().len());
        }
    }

    fn continuation(&mut self) -> Parsed<'a, ()> {
        loop {
            self.space();
            self.comment();
            if !self.newline()? {
                break;
            }
        }
        Ok(())
    }

    fn newline(&mut self) -> Parsed<'a, bool> {
        if !self.eat(b'\n') {
            return Ok(false);
        }
        for id in std::mem::take(&mut self.pending_here_documents) {
            let body_start = self.pos;
            let doc = &mut self.here_documents[id];
            let body_end;
            loop {
                let line_start = self.pos;
                let mut raw = String::new();
                let mut logical = String::new();
                loop {
                    let rest = &self.source[self.pos..];
                    if rest.is_empty() {
                        return Err(self.expected("a here-document delimiter"));
                    }
                    let len = rest.find('\n').unwrap_or(rest.len());
                    let mut line = &rest[..len];
                    if doc.strip_tabs {
                        line = line.trim_start_matches('\t');
                    }
                    let newline = len < rest.len();
                    self.pos += len + usize::from(newline);
                    raw.push_str(line);
                    if newline {
                        raw.push('\n');
                    }
                    let continued = !doc.quoted
                        && newline
                        && line.bytes().rev().take_while(|b| *b == b'\\').count() % 2 == 1;
                    logical.push_str(if continued {
                        &line[..line.len() - 1]
                    } else {
                        line
                    });
                    if !continued {
                        break;
                    }
                }
                if logical == doc.delimiter {
                    body_end = line_start;
                    break;
                }
                doc.body.push_str(&raw);
            }
            let after = self.pos;
            let quoted = doc.quoted;
            let strip_tabs = doc.strip_tabs;
            doc.span = body_start..body_end;
            let content = if quoted {
                CshAstWord::Literal(doc.body.clone())
            } else {
                self.pos = body_start;
                self.heredoc_word(body_end, strip_tabs)?
            };
            self.here_documents[id].content = content;
            self.pos = after;
        }
        Ok(true)
    }

    fn alloc(&mut self, expression: CshAstExpression) -> CshAstNodeId {
        let start = match &expression {
            CshAstExpression::Binary { left, .. } => self.spans[left.0].start,
            CshAstExpression::Background(id)
            | CshAstExpression::Negated(id)
            | CshAstExpression::Redirected { expression: id, .. } => self.spans[id.0].start,
            _ => self.pos,
        };
        self.alloc_at(expression, start)
    }

    fn alloc_at(&mut self, expression: CshAstExpression, start: usize) -> CshAstNodeId {
        let id = CshAstNodeId(self.nodes.len());
        self.nodes.push(expression);
        self.spans.push(start..self.pos);
        id
    }

    fn at_stop(&self, stop: Stop) -> bool {
        if self.byte().is_none() {
            return true;
        }
        match stop {
            Stop::Eof => false,
            Stop::Backtick => self.byte() == Some(b'`'),
            Stop::Subshell => self.byte() == Some(b')'),
            Stop::Group => self.keyword() == Some(Keyword::CloseBrace),
            Stop::Then => self.keyword() == Some(Keyword::Then),
            Stop::Branch => matches!(
                self.keyword(),
                Some(Keyword::Elif | Keyword::Else | Keyword::Fi)
            ),
            Stop::Fi => self.keyword() == Some(Keyword::Fi),
            Stop::Do => self.keyword() == Some(Keyword::Do),
            Stop::Done => self.keyword() == Some(Keyword::Done),
            Stop::Case => {
                (self.byte() == Some(b';') && matches!(self.peek(1), Some(b';' | b'&')))
                    || self.keyword() == Some(Keyword::Esac)
            }
        }
    }

    fn list(&mut self, stop: Stop, nonempty: bool) -> Parsed<'a, CshAstList> {
        // Bound recursive syntax, but keep operator chains iterative. The arena
        // also makes dropping a long chain independent of the call stack.
        if self.depth >= 128 {
            return Err(self.expected("at most 128 nested command lists"));
        }
        self.depth += 1;
        let result = self.list_inner(stop, nonempty);
        self.depth -= 1;
        result
    }

    fn list_inner(&mut self, stop: Stop, nonempty: bool) -> Parsed<'a, CshAstList> {
        let mut commands = Vec::new();
        loop {
            self.space();
            self.comment();
            if self.at_stop(stop) {
                break;
            }
            if self.newline()? {
                continue;
            }
            if self.byte() == Some(b';') && !matches!(self.peek(1), Some(b';' | b'&')) {
                self.pos += 1;
                continue;
            }
            let mut expression = self.expression()?;
            self.space();
            self.comment();
            if self.byte() == Some(b'&') && !matches!(self.peek(1), Some(b'&' | b'>')) {
                self.pos += 1;
                expression = self.alloc(CshAstExpression::Background(expression));
                commands.push(expression);
                continue;
            }
            commands.push(expression);
            if self.at_stop(stop) {
                break;
            }
            if self.newline()? {
                continue;
            }
            if self.byte() == Some(b';') && !matches!(self.peek(1), Some(b';' | b'&')) {
                self.pos += 1;
                continue;
            }
            return Err(self.expected("a command separator"));
        }
        if nonempty && commands.is_empty() {
            Err(self.expected("a command"))
        } else {
            Ok(commands)
        }
    }

    fn expression(&mut self) -> Parsed<'a, CshAstNodeId> {
        let mut left = self.pipeline()?;
        loop {
            self.space();
            let operator = match self.byte() {
                Some(b'&') if self.peek(1) == Some(b'&') => CshAstOperator::And,
                Some(b'|') if self.peek(1) == Some(b'|') => CshAstOperator::Or,
                _ => break,
            };
            self.pos += 2;
            self.continuation()?;
            let right = self.pipeline()?;
            left = self.alloc(CshAstExpression::Binary {
                left,
                operator,
                right,
            });
        }
        Ok(left)
    }

    fn pipeline(&mut self) -> Parsed<'a, CshAstNodeId> {
        self.space();
        let start = self.pos;
        let negated = self.byte() == Some(b'!') && self.eat_keyword(Keyword::Bang);
        let mut left = self.command()?;
        loop {
            self.space();
            if self.byte() != Some(b'|') || self.peek(1) == Some(b'|') {
                break;
            }
            self.pos += 1;
            let operator = if self.eat(b'&') {
                CshAstOperator::PipeWithStderr
            } else {
                CshAstOperator::Pipe
            };
            self.continuation()?;
            let right = self.command()?;
            left = self.alloc(CshAstExpression::Binary {
                left,
                operator,
                right,
            });
        }
        Ok(if negated {
            self.alloc_at(CshAstExpression::Negated(left), start)
        } else {
            left
        })
    }

    fn command(&mut self) -> Parsed<'a, CshAstNodeId> {
        self.space();
        let command_start = self.pos;
        let expression = if let Some(name) = self.function_header()? {
            self.continuation()?;
            if !matches!(self.byte(), Some(b'(' | b'{')) {
                return Err(self.expected("a function body"));
            }
            let body = self.command()?;
            CshAstExpression::Function { name, body }
        } else if self.byte() == Some(b'[') && self.peek(1) == Some(b'[') {
            CshAstExpression::Test(self.condition()?)
        } else if self.byte() == Some(b'(') && self.peek(1) == Some(b'(') {
            // Bash also permits adjacent nested subshells: ((cmd) || other).
            // Arithmetic requires a matching, adjacent closing `))`.
            let start = self.pos;
            let nodes = self.nodes.len();
            let documents = self.here_documents.len();
            let pending = self.pending_here_documents.clone();
            let depth = self.depth;
            match self.arithmetic_command() {
                Ok(expression) => CshAstExpression::Arithmetic(expression),
                Err(error) => {
                    self.nodes.truncate(nodes);
                    self.spans.truncate(nodes);
                    self.here_documents.truncate(documents);
                    self.pending_here_documents = pending;
                    self.depth = depth;
                    self.pos = start;
                    // If delimiters unambiguously identify arithmetic, retain
                    // its error instead of accepting it as two subshells.
                    if self.arithmetic_delimiters().is_ok() {
                        return Err(error);
                    }
                    self.pos = start + 1;
                    let body = self.list(Stop::Subshell, true)?;
                    self.require_close_paren()?;
                    CshAstExpression::Subshell(body)
                }
            }
        } else if self.eat(b'(') {
            let body = self.list(Stop::Subshell, true)?;
            self.require_close_paren()?;
            CshAstExpression::Subshell(body)
        } else {
            match self.keyword() {
                Some(Keyword::OpenBrace) => {
                    self.pos += 1;
                    let body = self.list(Stop::Group, true)?;
                    self.require_keyword(Keyword::CloseBrace)?;
                    CshAstExpression::Group(body)
                }
                Some(Keyword::If) => {
                    self.pos += 2;
                    self.conditional()?
                }
                Some(Keyword::For) => {
                    self.pos += 3;
                    self.for_loop()?
                }
                Some(Keyword::Case) => {
                    self.pos += 4;
                    self.case()?
                }
                Some(keyword @ (Keyword::While | Keyword::Until)) => {
                    self.pos += 5;
                    let condition = self.list(Stop::Do, true)?;
                    self.require_keyword(Keyword::Do)?;
                    let body = self.list(Stop::Done, true)?;
                    self.require_keyword(Keyword::Done)?;
                    CshAstExpression::Loop {
                        until: keyword == Keyword::Until,
                        condition,
                        body,
                    }
                }
                None | Some(Keyword::In | Keyword::Bang) => return self.simple(),
                _ => return Err(self.expected("a command")),
            }
        };
        let id = self.alloc_at(expression, command_start);
        let mut redirects = Vec::new();
        loop {
            self.space();
            let Some(redirect) = self.redirect()? else {
                break;
            };
            redirects.push(redirect);
        }
        Ok(self.redirected(id, redirects))
    }

    fn function_header(&mut self) -> Parsed<'a, Option<String>> {
        let explicit = matches!(
            &self.source.as_bytes()[self.pos..],
            [b'f', b'u', b'n', b'c', b't', b'i', b'o', b'n']
                | [
                    b'f',
                    b'u',
                    b'n',
                    b'c',
                    b't',
                    b'i',
                    b'o',
                    b'n',
                    b' ' | b'\t' | b'\n',
                    ..
                ]
        );
        if explicit {
            self.pos += 8;
            self.space();
        }
        let start = self.pos;
        let bytes = self.source.as_bytes();
        if !matches!(self.byte(), Some(b'a'..=b'z' | b'A'..=b'Z' | b'_')) {
            return if explicit {
                Err(self.expected("a function name"))
            } else {
                Ok(None)
            };
        }
        let mut end = start + 1;
        while matches!(
            bytes.get(end),
            Some(b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'-')
        ) {
            end += 1;
        }
        let mut after = end;
        while matches!(bytes.get(after), Some(b' ' | b'\t')) {
            after += 1;
        }
        if bytes.get(after) == Some(&b'(') && bytes.get(after + 1) == Some(&b')') {
            self.pos = after + 2;
        } else if explicit {
            self.pos = after;
        } else {
            return Ok(None);
        }
        Ok(Some(self.source[start..end].to_owned()))
    }

    fn arithmetic_delimiters(&mut self) -> Parsed<'a, ()> {
        self.pos += 2;
        self.balanced(b')')?;
        self.require_close_paren()?;
        Ok(())
    }

    fn conditional(&mut self) -> Parsed<'a, CshAstExpression> {
        let mut branches = Vec::new();
        loop {
            let condition = self.list(Stop::Then, true)?;
            self.require_keyword(Keyword::Then)?;
            let body = self.list(Stop::Branch, true)?;
            branches.push(CshAstBranch { condition, body });
            if !self.eat_keyword(Keyword::Elif) {
                break;
            }
        }
        let otherwise = if self.eat_keyword(Keyword::Else) {
            Some(self.list(Stop::Fi, true)?)
        } else {
            None
        };
        self.require_keyword(Keyword::Fi)?;
        Ok(CshAstExpression::If {
            branches,
            otherwise,
        })
    }

    fn for_loop(&mut self) -> Parsed<'a, CshAstExpression> {
        self.space();
        if self.byte() == Some(b'(') && self.peek(1) == Some(b'(') {
            let clauses = self.arithmetic_for()?;
            self.space();
            self.eat(b';');
            self.continuation()?;
            self.require_keyword(Keyword::Do)?;
            let body = self.list(Stop::Done, true)?;
            self.require_keyword(Keyword::Done)?;
            return Ok(CshAstExpression::ArithmeticFor { clauses, body });
        }
        let variable = self
            .delimiter_word()?
            .ok_or_else(|| self.expected("a loop variable"))?;
        self.space();
        let words = if self.eat_keyword(Keyword::In) {
            let mut words = Vec::new();
            loop {
                self.space();
                let Some(word) = self.word()? else {
                    break;
                };
                words.push(word);
            }
            Some(words)
        } else {
            None
        };
        self.space();
        if !self.eat(b';') && !self.eat(b'\n') {
            return Err(self.expected("`;` or a newline"));
        }
        self.continuation()?;
        self.require_keyword(Keyword::Do)?;
        let body = self.list(Stop::Done, true)?;
        self.require_keyword(Keyword::Done)?;
        Ok(CshAstExpression::For {
            variable,
            words,
            body,
        })
    }

    fn case(&mut self) -> Parsed<'a, CshAstExpression> {
        self.space();
        let word = self.required_word()?;
        self.space();
        self.require_keyword(Keyword::In)?;
        self.continuation()?;
        let mut arms = Vec::new();
        while !self.eat_keyword(Keyword::Esac) {
            self.eat(b'(');
            let mut patterns = Vec::new();
            loop {
                self.space();
                patterns.push(self.required_word()?);
                self.space();
                if !self.eat(b'|') {
                    break;
                }
            }
            self.require_close_paren()?;
            let body = self.list(Stop::Case, false)?;
            let terminator = match (self.byte(), self.peek(1)) {
                (Some(b';'), Some(b';')) if self.peek(2) == Some(b'&') => ";;&",
                (Some(b';'), Some(b';')) => ";;",
                (Some(b';'), Some(b'&')) => ";&",
                _ => "",
            };
            self.pos += terminator.len();
            let terminator = terminator.to_owned();
            arms.push(CshAstCaseArm {
                patterns,
                body,
                terminator,
            });
            self.continuation()?;
        }
        Ok(CshAstExpression::Case { word, arms })
    }

    fn simple(&mut self) -> Parsed<'a, CshAstNodeId> {
        let start = self.pos;
        let mut command = CshAstCommand {
            assignments: Vec::new(),
            name: None,
            args: Vec::new(),
        };
        let mut has_name = false;
        let mut has_item = false;
        let mut redirects = Vec::new();
        loop {
            self.space();
            if let Some(redirect) = self.redirect()? {
                redirects.push(redirect);
                has_item = true;
                continue;
            }
            let declaration = matches!(&command.name, Some(CshAstWord::Literal(name)) if matches!(name.as_str(), "declare" | "typeset" | "local" | "export" | "readonly"));
            if (!has_name || declaration)
                && let Some(assignment) = self.assignment()?
            {
                if has_name {
                    command
                        .args
                        .push(CshAstWord::Assignment(Box::new(assignment)));
                } else {
                    command.assignments.push(assignment);
                }
                has_item = true;
                continue;
            }
            let Some(word) = self.word()? else {
                break;
            };
            has_item = true;
            if !has_name {
                command.name = Some(word);
                has_name = true;
            } else {
                command.args.push(word);
            }
        }
        if !has_item {
            return Err(self.expected("a command"));
        }
        let id = self.alloc_at(CshAstExpression::Command(command), start);
        Ok(self.redirected(id, redirects))
    }

    fn redirected(
        &mut self,
        expression: CshAstNodeId,
        redirects: Vec<CshAstRedirect>,
    ) -> CshAstNodeId {
        if redirects.is_empty() {
            expression
        } else {
            self.alloc(CshAstExpression::Redirected {
                expression,
                redirects,
            })
        }
    }

    fn redirect(&mut self) -> Parsed<'a, Option<CshAstRedirect>> {
        if !matches!(self.byte(), Some(b'0'..=b'9' | b'<' | b'>' | b'&' | b'{')) {
            return Ok(None);
        }
        let start = self.pos;
        let mut prefix = 0;
        if self.byte() == Some(b'{') {
            if !matches!(self.peek(1), Some(b'a'..=b'z' | b'A'..=b'Z' | b'_')) {
                return Ok(None);
            }
            prefix = 2;
            while matches!(
                self.peek(prefix),
                Some(b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_')
            ) {
                prefix += 1;
            }
            if self.peek(prefix) != Some(b'}')
                || !matches!(self.peek(prefix + 1), Some(b'<' | b'>'))
            {
                return Ok(None);
            }
            prefix += 1;
        } else {
            while matches!(self.peek(prefix), Some(b'0'..=b'9')) {
                prefix += 1;
            }
        }
        let operator = match &self.source.as_bytes()[start + prefix..] {
            // These are word fragments, not redirection operators.
            [b'<' | b'>', b'(', ..] => return Ok(None),
            [b'&', b'>', b'>', ..] => "&>>",
            [b'&', b'>', ..] => "&>",
            [b'<', b'<', b'<', ..] => "<<<",
            [b'<', b'<', b'-', ..] => "<<-",
            [b'<', b'<', ..] => "<<",
            [b'<', b'&', ..] => "<&",
            [b'<', b'>', ..] => "<>",
            [b'<', ..] => "<",
            [b'>', b'>', ..] => ">>",
            [b'>', b'&', ..] => ">&",
            [b'>', b'|', ..] => ">|",
            [b'>', ..] => ">",
            _ => return Ok(None),
        };
        self.pos += prefix + operator.len();
        let descriptor = if prefix == 0 {
            CshAstDescriptor::Default
        } else if self.source.as_bytes()[start] == b'{' {
            CshAstDescriptor::Variable(self.source[start + 1..start + prefix - 1].to_owned())
        } else {
            CshAstDescriptor::Number(self.source[start..start + prefix].to_owned())
        };
        self.space();
        let delimiter_start = self.pos;
        let delimiter = if matches!(operator, "<<" | "<<-") {
            Some(
                self.delimiter_word()?
                    .ok_or_else(|| self.expected("a redirection target"))?,
            )
        } else {
            None
        };
        let target = if let Some(delimiter) = &delimiter {
            CshAstWord::Literal(delimiter.clone())
        } else {
            self.word()?
                .ok_or_else(|| self.expected("a redirection target"))?
        };
        let here_document = if matches!(operator, "<<" | "<<-") {
            let id = self.here_documents.len();
            self.here_documents.push(CshAstHereDocument {
                delimiter: delimiter.unwrap(),
                quoted: self.source[delimiter_start..self.pos]
                    .replace("\\\n", "")
                    .contains(['\'', '"', '\\']),
                strip_tabs: operator == "<<-",
                body: String::new(),
                content: CshAstWord::Literal(String::new()),
                span: self.pos..self.pos,
            });
            self.pending_here_documents.push(id);
            Some(id)
        } else {
            None
        };
        use CshAstRedirectOperator as R;
        let operator = match operator {
            "<" => R::Input,
            ">" => R::Output,
            ">>" => R::Append,
            ">|" => R::Clobber,
            "<>" => R::ReadWrite,
            "<&" if target == CshAstWord::Literal("-".into()) => R::CloseInput,
            ">&" if target == CshAstWord::Literal("-".into()) => R::CloseOutput,
            "<&" => R::DuplicateInput,
            ">&" => R::DuplicateOutput,
            "<<" => R::HereDocument,
            "<<-" => R::HereDocumentStripTabs,
            "<<<" => R::HereString,
            "&>" => R::OutputAndError,
            "&>>" => R::AppendAndError,
            _ => unreachable!(),
        };
        Ok(Some(CshAstRedirect {
            descriptor,
            operator,
            target,
            span: start..self.pos,
            here_document,
        }))
    }

    fn required_word(&mut self) -> Parsed<'a, CshAstWord> {
        self.word()?.ok_or_else(|| self.expected("a word"))
    }

    fn delimiter_word(&mut self) -> Parsed<'a, Option<String>> {
        if self.byte() == Some(b'#') {
            return Ok(None);
        }
        let start = self.pos;
        let mut text = String::new();
        loop {
            match self.byte() {
                Some(b'(') if text.ends_with('=') => {
                    let start = self.pos;
                    self.pos += 1;
                    loop {
                        self.continuation()?;
                        if self.eat(b')') {
                            break;
                        }
                        self.required_word()?;
                    }
                    text.push_str(&self.source[start..self.pos]);
                }
                None | Some(b' ' | b'\t' | b'\r' | b'\n' | b';' | b'|' | b'&' | b'(' | b')') => {
                    break;
                }
                Some(b'\'') => self.single(&mut text)?,
                Some(b'"') => self.double(&mut text)?,
                Some(b'\\') => self.escape(&mut text, false)?,
                Some(b'$' | b'`') => self.expansion(&mut text)?,
                Some(b'<' | b'>') => {
                    if self.source.as_bytes().get(self.pos + 1) != Some(&b'(') {
                        break;
                    }
                    self.substitution(&mut text)?;
                }
                Some(b'?' | b'*' | b'+' | b'@' | b'!')
                    if self.source.as_bytes().get(self.pos + 1) == Some(&b'(') =>
                {
                    let start = self.pos;
                    self.pos += 2;
                    self.balanced(b')')?;
                    text.push_str(&self.source[start..self.pos]);
                }
                _ => {
                    let start = self.pos;
                    let bytes = self.source.as_bytes();
                    while let Some(&b) = bytes.get(self.pos) {
                        match WORD_CLASS[b as usize] {
                            BARE => self.pos += 1,
                            GLOB if bytes.get(self.pos + 1) != Some(&b'(') => self.pos += 1,
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
                    if start == self.pos {
                        break;
                    }
                    text.push_str(&self.source[start..self.pos]);
                }
            }
        }
        Ok((self.pos != start).then_some(text))
    }

    fn unclosed(&self, quote: char, opening: usize) -> CshError<'a> {
        CshError::UnclosedQuote {
            quote,
            opening_span: opening..opening + 1,
            end_span: self.pos..self.pos,
        }
    }

    fn single(&mut self, text: &mut String) -> Parsed<'a, ()> {
        let opening = self.pos;
        self.pos += 1;
        if let Some(len) = self.rest().find('\'') {
            text.push_str(&self.source[self.pos..self.pos + len]);
            self.pos += len + 1;
            Ok(())
        } else {
            self.pos = self.source.len();
            Err(self.unclosed('\'', opening))
        }
    }

    fn double(&mut self, text: &mut String) -> Parsed<'a, ()> {
        let opening = self.pos;
        self.pos += 1;
        loop {
            match self.byte() {
                None => return Err(self.unclosed('"', opening)),
                Some(b'"') => {
                    self.pos += 1;
                    return Ok(());
                }
                Some(b'\\') => self.escape(text, true)?,
                Some(b'$' | b'`') => self.expansion(text)?,
                _ => {
                    let start = self.pos;
                    while let Some(b) = self.byte() {
                        if matches!(b, b'"' | b'\\' | b'$' | b'`') {
                            break;
                        }
                        self.pos += 1;
                    }
                    text.push_str(&self.source[start..self.pos]);
                }
            }
        }
    }

    fn escape(&mut self, text: &mut String, double: bool) -> Parsed<'a, ()> {
        let start = self.pos;
        self.pos += 1;
        let Some(c) = self.rest().chars().next() else {
            return Err(CshError::IncompleteEscape {
                span: start..self.pos,
            });
        };
        self.pos += c.len_utf8();
        if c != '\n' {
            if double && !matches!(c, '$' | '`' | '"' | '\\') {
                text.push('\\');
            }
            text.push(c);
        }
        Ok(())
    }

    fn expansion(&mut self, text: &mut String) -> Parsed<'a, ()> {
        let start = self.pos;
        match (self.byte(), self.peek(1)) {
            (Some(b'$'), Some(b'(')) if self.peek(2) == Some(b'(') => {
                self.pos += 3;
                self.balanced(b')')?;
                self.require_close_paren()?;
            }
            (Some(b'$'), Some(b'(')) => return self.substitution(text),
            (Some(b'$'), Some(b'{')) => {
                self.pos += 2;
                self.balanced(b'}')?;
            }
            (Some(b'`'), _) => {
                self.pos += 1;
                loop {
                    match self.byte() {
                        None => return Err(self.expected("a closing backtick")),
                        Some(b'`') => {
                            self.pos += 1;
                            break;
                        }
                        Some(b'\\') => self.skip_escape()?,
                        _ => self.pos += 1,
                    }
                }
            }
            _ => self.pos += 1, // A bare dollar sign, including $name.
        }
        text.push_str(&self.source[start..self.pos]);
        Ok(())
    }

    fn substitution(&mut self, text: &mut String) -> Parsed<'a, ()> {
        let start = self.pos;
        self.pos += 2;
        // Validate nested shell syntax with the same parser. Expansion text is
        // retained for execution; temporary nodes are reclaimed in one truncate.
        let checkpoint = self.nodes.len();
        let document_checkpoint = self.here_documents.len();
        let outer_pending = std::mem::take(&mut self.pending_here_documents);
        self.list(Stop::Subshell, false)?;
        self.require_close_paren()?;
        if !self.pending_here_documents.is_empty() {
            return Err(self.expected("a here-document body"));
        }
        self.nodes.truncate(checkpoint);
        self.spans.truncate(checkpoint);
        self.here_documents.truncate(document_checkpoint);
        self.pending_here_documents = outer_pending;
        text.push_str(&self.source[start..self.pos]);
        Ok(())
    }

    fn skip_escape(&mut self) -> Parsed<'a, ()> {
        let start = self.pos;
        self.pos += 1;
        let Some(c) = self.rest().chars().next() else {
            return Err(CshError::IncompleteEscape {
                span: start..self.pos,
            });
        };
        self.pos += c.len_utf8();
        Ok(())
    }

    /// Match expansion delimiters iteratively, retaining the original text.
    fn balanced(&mut self, closing: u8) -> Parsed<'a, ()> {
        let mut closings = vec![closing];
        while let Some(b) = self.byte() {
            match b {
                b'\\' => self.skip_escape()?,
                b'\'' | b'"' => {
                    let opening = self.pos;
                    self.pos += 1;
                    loop {
                        match self.byte() {
                            None => return Err(self.unclosed(b as char, opening)),
                            Some(c) if c == b => {
                                self.pos += 1;
                                break;
                            }
                            Some(b'\\') if b == b'"' => self.skip_escape()?,
                            _ => self.pos += 1,
                        }
                    }
                }
                b'(' | b'{' => {
                    closings.push(if b == b'(' { b')' } else { b'}' });
                    self.pos += 1;
                }
                b')' | b'}' => {
                    if closings.last() != Some(&b) {
                        return Err(self.expected("a matching expansion delimiter"));
                    }
                    self.pos += 1;
                    closings.pop();
                    if closings.is_empty() {
                        return Ok(());
                    }
                }
                _ => self.pos += 1,
            }
        }
        Err(self.expected("a closing expansion delimiter"))
    }
}
