use crate::{
    eval::Evaluator,
    io::{self, Io},
};
use anyhow::{Result, bail};
use crossshell::*;
use std::io::Read;

#[derive(Clone, Default)]
struct Field {
    parts: Vec<Part>,
    keep_empty: bool,
}
#[derive(Clone)]
struct Part {
    text: String,
    split: bool,
    glob: bool,
}
struct Raw {
    fields: Vec<Field>,
    alternatives: bool,
}

impl Field {
    fn text(text: String, split: bool, glob: bool, keep_empty: bool) -> Self {
        Self {
            parts: vec![Part { text, split, glob }],
            keep_empty,
        }
    }
    fn append(&mut self, other: &Field) {
        self.parts.extend(other.parts.clone());
        self.keep_empty |= other.keep_empty;
    }
    fn string(&self) -> String {
        self.parts.iter().map(|p| p.text.as_str()).collect()
    }
    fn pattern(&self) -> String {
        self.parts
            .iter()
            .map(|p| {
                if p.glob {
                    p.text.clone()
                } else {
                    glob::Pattern::escape(&p.text)
                }
            })
            .collect()
    }
}

impl Raw {
    fn one(field: Field) -> Self {
        Self {
            fields: vec![field],
            alternatives: false,
        }
    }
}

impl Evaluator<'_, '_, '_> {
    pub fn words(&mut self, words: &[CshAstWord<'_>], io: &Io) -> Result<Vec<String>> {
        let mut result = Vec::new();
        for word in words {
            let raw = self.expand(word, false, io)?;
            for field in raw.fields {
                for field in self.split(field)? {
                    let text = field.string();
                    let pattern = field.pattern();
                    if !self.noglob && pattern != glob::Pattern::escape(&text) {
                        let absolute = self.path(&pattern);
                        let mut matches = glob::glob_with(
                            absolute
                                .to_str()
                                .ok_or_else(|| anyhow::anyhow!("Non-Unicode glob path"))?,
                            glob::MatchOptions {
                                case_sensitive: true,
                                require_literal_separator: true,
                                require_literal_leading_dot: true,
                            },
                        )?
                        .map(|p| {
                            p.map(|p| {
                                if std::path::Path::new(&text).is_absolute() {
                                    p
                                } else {
                                    p.strip_prefix(&self.cwd).unwrap_or(&p).to_path_buf()
                                }
                            })
                        })
                        .collect::<std::result::Result<Vec<_>, _>>()?;
                        matches.sort();
                        if !matches.is_empty() {
                            for path in matches {
                                result.push(
                                    path.into_os_string()
                                        .into_string()
                                        .map_err(|_| anyhow::anyhow!("Non-Unicode glob result"))?,
                                );
                            }
                            continue;
                        }
                    }
                    result.push(text);
                }
            }
        }
        Ok(result)
    }

    pub fn scalar(&mut self, word: &CshAstWord<'_>, io: &Io) -> Result<String> {
        let separator = self.ifs_separator()?;
        Ok(self
            .expand(word, true, io)?
            .fields
            .iter()
            .map(Field::string)
            .collect::<Vec<_>>()
            .join(&separator))
    }

    pub fn pattern(&mut self, word: &CshAstWord<'_>, io: &Io) -> Result<String> {
        Ok(self
            .expand(word, false, io)?
            .fields
            .iter()
            .map(Field::pattern)
            .collect::<Vec<_>>()
            .join(" "))
    }

    pub fn assignment(&mut self, a: &CshAstAssignment<'_>, io: &Io) -> Result<String> {
        let mut value = if a.operator == CshAstAssignmentOperator::Append {
            self.value(a.name)?.unwrap_or_default()
        } else {
            String::new()
        };
        value.push_str(&self.scalar(&a.value, io)?);
        Ok(value)
    }

    fn expand(&mut self, word: &CshAstWord<'_>, quoted: bool, io: &Io) -> Result<Raw> {
        use CshAstWord as W;
        let literal = |text: String| Raw::one(Field::text(text, false, !quoted, quoted));
        let expansion = |text: String| Raw::one(Field::text(text, !quoted, !quoted, quoted));
        Ok(match word {
            W::Literal(s) => literal(s.to_string()),
            W::SingleQuoted(s) | W::Escaped(s) => {
                Raw::one(Field::text((*s).into(), false, false, true))
            }
            W::AnsiCQuoted(s) => Raw::one(Field::text(ansi(s)?, false, false, true)),
            W::DoubleQuoted(w) | W::LocaleQuoted(w) => self.expand(w, true, io)?,
            W::Variable(name) => {
                if *name == "@" {
                    Raw {
                        fields: self
                            .args
                            .iter()
                            .map(|a| Field::text(a.clone(), !quoted, !quoted, quoted))
                            .collect(),
                        alternatives: false,
                    }
                } else {
                    let value = self.value(name)?;
                    if value.is_none() && self.nounset && !matches!(*name, "*" | "!") {
                        bail!("{name}: unbound variable");
                    }
                    expansion(value.unwrap_or_default())
                }
            }
            W::Parameter(p) => {
                if p.subscript.is_none()
                    && p.mode == CshAstParameterMode::Value
                    && let CshAstParameterOperation::Default {
                        operator,
                        test_empty,
                        word,
                    } = &p.operation
                {
                    let value = self.value(p.name)?;
                    let absent = value.is_none() || (*test_empty && value.as_deref() == Some(""));
                    if (absent
                        && matches!(
                            operator,
                            CshAstDefaultOperator::Use | CshAstDefaultOperator::Assign
                        ))
                        || (!absent && *operator == CshAstDefaultOperator::Alternate)
                    {
                        let raw = self.expand(word, quoted, io)?;
                        if *operator == CshAstDefaultOperator::Assign {
                            if !crate::builtins::valid_name(p.name) {
                                bail!("{}: invalid assignment target", p.name);
                            }
                            let value = raw
                                .fields
                                .iter()
                                .map(Field::string)
                                .collect::<Vec<_>>()
                                .join(&self.ifs_separator()?);
                            self.set(p.name, value);
                        }
                        return Ok(raw);
                    }
                }
                if p.name == "@"
                    && p.subscript.is_none()
                    && p.mode == CshAstParameterMode::Value
                    && matches!(p.operation, CshAstParameterOperation::None)
                {
                    Raw {
                        fields: self
                            .args
                            .iter()
                            .map(|a| Field::text(a.clone(), !quoted, !quoted, quoted))
                            .collect(),
                        alternatives: false,
                    }
                } else {
                    expansion(self.parameter(p, io)?)
                }
            }
            W::CommandSubstitution { commands, .. } => {
                let mut state = self.state.fork();
                // Substitutions have a distinct shell environment and inherit I/O
                // except stdout. Read concurrently so large outputs cannot deadlock.
                state.errexit = false;
                let mut child = crate::eval::Evaluator::new(&mut state, self.ast, self.scope);
                child.functions = self.functions.clone();
                let (reader, writer) = io::pipe()?;
                let handle = self.scope.spawn(move || {
                    let mut bytes = Vec::new();
                    (&*reader).read_to_end(&mut bytes)?;
                    Ok(bytes)
                });
                let mut output = io.clone();
                output.0.insert(1, writer);
                let result = child.list(commands, &output, false);
                let jobs = child.wait_all();
                drop(output);
                let bytes = io::join(handle)?;
                let result = result?;
                jobs?;
                self.substitution_status = Some(result.status);
                let text = String::from_utf8(bytes)?.replace('\0', "");
                expansion(text.trim_end_matches('\n').to_owned())
            }
            W::ArithmeticExpansion(a) => expansion(self.arithmetic(a, io)?.to_string()),
            W::Concat(words) => {
                let mut groups = vec![Vec::<Field>::new()];
                for word in words {
                    let raw = self.expand(word, quoted, io)?;
                    if raw.alternatives {
                        let mut next = Vec::new();
                        for group in groups {
                            for field in &raw.fields {
                                let mut group = group.clone();
                                append_fields(&mut group, std::slice::from_ref(field));
                                next.push(group);
                            }
                        }
                        groups = next;
                    } else {
                        for group in &mut groups {
                            append_fields(group, &raw.fields);
                        }
                    }
                    if groups.len() > 100_000 {
                        bail!("Brace expansion exceeds 100000 words");
                    }
                }
                let alternatives = groups.len() > 1;
                Raw {
                    fields: groups.into_iter().flatten().collect(),
                    alternatives,
                }
            }
            W::BraceAlternatives(words) => {
                let mut fields = Vec::new();
                for word in words {
                    fields.extend(self.expand(word, quoted, io)?.fields);
                }
                Raw {
                    fields,
                    alternatives: true,
                }
            }
            W::BraceSequence(s) => {
                let (start, end) = if s.alphabetic {
                    (
                        s.start.chars().next().unwrap_or_default() as i64,
                        s.end.chars().next().unwrap_or_default() as i64,
                    )
                } else {
                    (s.start.parse::<i64>()?, s.end.parse::<i64>()?)
                };
                let magnitude = s
                    .step
                    .unwrap_or("1")
                    .parse::<i64>()?
                    .checked_abs()
                    .filter(|n| *n != 0)
                    .unwrap_or(1);
                let step = if start <= end { magnitude } else { -magnitude };
                let mut fields = Vec::new();
                let mut n = start;
                while if step > 0 { n <= end } else { n >= end } {
                    if fields.len() >= 100_000 {
                        bail!("Brace expansion exceeds 100000 words");
                    }
                    let value = if s.alphabetic {
                        char::from_u32(n as u32).unwrap_or_default().to_string()
                    } else {
                        format!("{n:0width$}", width = s.padding)
                    };
                    fields.push(Field::text(value, false, !quoted, quoted));
                    let Some(next) = n.checked_add(step) else {
                        break;
                    };
                    n = next;
                }
                Raw {
                    fields,
                    alternatives: true,
                }
            }
            W::Tilde(t) => {
                let value = match t {
                    CshAstTilde::Home => self.value("HOME")?.unwrap_or_else(|| "~".into()),
                    CshAstTilde::WorkingDirectory => self.cwd.to_string_lossy().into_owned(),
                    CshAstTilde::PreviousDirectory => {
                        self.value("OLDPWD")?.unwrap_or_else(|| "~-".into())
                    }
                    CshAstTilde::User(_) | CshAstTilde::DirectoryStack { .. } => {
                        bail!("Unsupported expansion: named-user or directory-stack tilde")
                    }
                };
                Raw::one(Field::text(value, false, false, true))
            }
            W::Assignment(a) => Raw::one(Field::text(
                format!("{}={}", a.name, self.assignment(a, io)?),
                false,
                false,
                true,
            )),
            W::Glob(g) => literal(glob_text(g)?),
            W::Array(_) | W::KeyedElement { .. } => {
                bail!("Unsupported expansion: array assignment")
            }
            W::ExtendedGlob { .. } => bail!("Unsupported expansion: extended glob"),
            W::ProcessSubstitution { .. } => bail!("Unsupported expansion: process substitution"),
        })
    }

    fn split(&self, field: Field) -> Result<Vec<Field>> {
        let ifs = self.value("IFS")?.unwrap_or_else(|| " \t\n".into());
        if ifs.is_empty() {
            return Ok(if field.string().is_empty() && !field.keep_empty {
                Vec::new()
            } else {
                vec![field]
            });
        }
        let mut fields = Vec::new();
        let mut current = Field {
            keep_empty: field.keep_empty,
            ..Field::default()
        };
        let mut pending_space = false;
        for part in field.parts {
            for c in part.text.chars() {
                if part.split && ifs.contains(c) {
                    if " \t\n".contains(c) {
                        if !current.parts.is_empty() {
                            pending_space = true;
                        }
                    } else {
                        fields.push(std::mem::take(&mut current));
                        pending_space = false;
                    }
                    continue;
                }
                if pending_space && !current.parts.is_empty() {
                    fields.push(std::mem::take(&mut current));
                }
                pending_space = false;
                current.parts.push(Part {
                    text: c.to_string(),
                    split: false,
                    glob: part.glob,
                });
            }
        }
        if !current.parts.is_empty() || current.keep_empty {
            fields.push(current);
        }
        // A trailing IFS non-whitespace separator terminates a field; it does
        // not create another empty field, unlike two adjacent separators.
        Ok(fields)
    }

    fn parameter(&mut self, p: &CshAstParameter<'_>, io: &Io) -> Result<String> {
        use CshAstParameterOperation as O;
        if p.subscript.is_some() {
            bail!("Unsupported expansion: array subscript");
        }
        let mut name = p.name.to_owned();
        match p.mode {
            CshAstParameterMode::Indirect => {
                name = self.value(&name)?.unwrap_or_default();
            }
            CshAstParameterMode::Names { .. } => {
                return Ok(self
                    .variables
                    .keys()
                    .filter(|n| n.starts_with(&name))
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" "));
            }
            CshAstParameterMode::Indices { .. } => bail!("Unsupported expansion: array indices"),
            _ => {}
        }
        let value = self.value(&name)?;
        if p.mode == CshAstParameterMode::Length {
            return Ok(if matches!(p.name, "@" | "*") {
                self.args.len()
            } else {
                value.as_deref().unwrap_or_default().chars().count()
            }
            .to_string());
        }
        Ok(match &p.operation {
            O::None => {
                if value.is_none() && self.nounset {
                    bail!("{name}: unbound variable");
                }
                value.unwrap_or_default()
            }
            O::Default {
                operator,
                test_empty,
                word,
            } => {
                let absent = value.is_none() || (*test_empty && value.as_deref() == Some(""));
                use CshAstDefaultOperator as D;
                match operator {
                    D::Use if absent => self.scalar(word, io)?,
                    D::Assign if absent => {
                        let value = self.scalar(word, io)?;
                        if !crate::builtins::valid_name(&name) {
                            bail!("{name}: invalid assignment target");
                        }
                        self.set(&name, value.clone());
                        value
                    }
                    D::Error if absent => bail!("{name}: {}", self.scalar(word, io)?),
                    D::Alternate if !absent => self.scalar(word, io)?,
                    D::Alternate => String::new(),
                    _ => value.unwrap_or_default(),
                }
            }
            O::Slice { offset, length } => {
                let chars: Vec<_> = value.unwrap_or_default().chars().collect();
                let offset = self.arithmetic(offset, io)?;
                let start = if offset < 0 {
                    (chars.len() as i64 + offset).max(0)
                } else {
                    offset
                } as usize;
                let end = if let Some(length) = length {
                    let length = self.arithmetic(length, io)?;
                    if length < 0 {
                        (chars.len() as i64 + length).max(0) as usize
                    } else {
                        start.saturating_add(length as usize)
                    }
                } else {
                    chars.len()
                };
                if end < start {
                    bail!("Substring length is negative");
                }
                chars.into_iter().skip(start).take(end - start).collect()
            }
            O::Trim {
                suffix,
                longest,
                pattern,
            } => {
                let value = value.unwrap_or_default();
                let pattern = self.pattern(pattern, io)?;
                let mut boundaries: Vec<_> = value
                    .char_indices()
                    .map(|(i, _)| i)
                    .chain(std::iter::once(value.len()))
                    .collect();
                if *longest != *suffix {
                    boundaries.reverse();
                }
                let mut result = value.clone();
                for i in boundaries {
                    let (candidate, remaining) = if *suffix {
                        (&value[i..], &value[..i])
                    } else {
                        (&value[..i], &value[i..])
                    };
                    if matches(&pattern, candidate)? {
                        result = remaining.into();
                        break;
                    }
                }
                result
            }
            O::Replace {
                anchor,
                pattern,
                replacement,
            } => {
                let value = value.unwrap_or_default();
                let pattern = self.pattern(pattern, io)?;
                let replacement = self.scalar(replacement, io)?;
                replace(&value, &pattern, &replacement, *anchor)?
            }
            O::Case {
                upper,
                all,
                pattern,
            } => {
                let value = value.unwrap_or_default();
                let pattern = self.pattern(pattern, io)?;
                let mut changed = false;
                let mut out = String::new();
                for c in value.chars() {
                    if (*all || !changed)
                        && (pattern.is_empty() || matches(&pattern, &c.to_string())?)
                    {
                        if *upper {
                            out.extend(c.to_uppercase());
                        } else {
                            out.extend(c.to_lowercase());
                        }
                        changed = true;
                    } else {
                        out.push(c);
                    }
                }
                out
            }
            O::Transform(t) => {
                let value = value.unwrap_or_default();
                match t {
                    CshAstParameterTransform::Upper => value.to_uppercase(),
                    CshAstParameterTransform::Lower => value.to_lowercase(),
                    CshAstParameterTransform::UpperFirst => {
                        let mut chars = value.chars();
                        chars
                            .next()
                            .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
                            .unwrap_or_default()
                    }
                    CshAstParameterTransform::Quote => {
                        format!("'{}'", value.replace('\'', "'\\''"))
                    }
                    CshAstParameterTransform::Escape => ansi(&value)?,
                    _ => bail!("Unsupported parameter transform: {t:?}"),
                }
            }
        })
    }
}

fn append_fields(target: &mut Vec<Field>, source: &[Field]) {
    if let Some((first, rest)) = source.split_first() {
        if let Some(last) = target.last_mut() {
            last.append(first);
        } else {
            target.push(first.clone());
        }
        target.extend_from_slice(rest);
    }
}

fn glob_text(g: &CshAstGlob<'_>) -> Result<String> {
    Ok(match g {
        CshAstGlob::Star | CshAstGlob::GlobStar => "*".into(),
        CshAstGlob::QuestionMark => "?".into(),
        CshAstGlob::CharacterClass { negated, items } => {
            let mut text = if *negated {
                "[!".to_owned()
            } else {
                "[".to_owned()
            };
            for item in items {
                match item {
                    CshAstGlobClassItem::Character(c) => text.push(*c),
                    CshAstGlobClassItem::Range { start, end } => {
                        text.push(*start);
                        text.push('-');
                        text.push(*end);
                    }
                    CshAstGlobClassItem::NamedClass(n) => text.push_str(match *n {
                        "alnum" => "a-zA-Z0-9",
                        "alpha" => "a-zA-Z",
                        "digit" => "0-9",
                        "lower" => "a-z",
                        "upper" => "A-Z",
                        "xdigit" => "a-fA-F0-9",
                        "space" => " \t\r\n\u{b}\u{c}",
                        "blank" => " \t",
                        _ => bail!("Unsupported glob character class: {n}"),
                    }),
                    _ => bail!("Unsupported glob collation"),
                }
            }
            text.push(']');
            text
        }
    })
}

pub(crate) fn matches(pattern: &str, value: &str) -> Result<bool> {
    Ok(glob::Pattern::new(pattern)?.matches_with(
        value,
        glob::MatchOptions {
            case_sensitive: true,
            require_literal_separator: false,
            require_literal_leading_dot: false,
        },
    ))
}

fn replace(
    value: &str,
    pattern: &str,
    replacement: &str,
    anchor: CshAstReplaceAnchor,
) -> Result<String> {
    let boundaries: Vec<_> = value
        .char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(value.len()))
        .collect();
    let mut output = String::new();
    let mut copied = 0;
    let mut matched = false;
    for (index, start) in boundaries.iter().copied().enumerate() {
        if start < copied
            || (matched && anchor != CshAstReplaceAnchor::All)
            || (anchor == CshAstReplaceAnchor::Prefix && start != 0)
        {
            continue;
        }
        for end in boundaries[index..].iter().copied().rev() {
            if anchor == CshAstReplaceAnchor::Suffix && end != value.len() {
                continue;
            }
            if matches(pattern, &value[start..end])? {
                output.push_str(&value[copied..start]);
                output.push_str(replacement);
                copied = end;
                matched = true;
                break;
            }
        }
    }
    output.push_str(&value[copied..]);
    Ok(output)
}

fn ansi(text: &str) -> Result<String> {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        let Some(c) = chars.next() else {
            out.push('\\');
            break;
        };
        match c {
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            'a' => out.push('\x07'),
            'b' => out.push('\x08'),
            'e' | 'E' => out.push('\x1b'),
            'f' => out.push('\x0c'),
            'v' => out.push('\x0b'),
            '\\' | '\'' | '"' => out.push(c),
            'x' | 'u' | 'U' | '0'..='7' => {
                let octal = c.is_ascii_digit();
                let mut digits = if octal { c.to_string() } else { String::new() };
                let count = match c {
                    'x' => 2,
                    'u' => 4,
                    'U' => 8,
                    _ => 2,
                };
                let radix = if octal { 8 } else { 16 };
                for _ in 0..count {
                    if chars.peek().is_some_and(|c| c.is_digit(radix)) {
                        digits.push(chars.next().unwrap());
                    } else {
                        break;
                    }
                }
                let n = u32::from_str_radix(&digits, radix)?;
                if n == 0 {
                    break;
                }
                out.push(
                    char::from_u32(n).ok_or_else(|| anyhow::anyhow!("Invalid Unicode escape"))?,
                );
            }
            _ => {
                out.push('\\');
                out.push(c);
            }
        }
    }
    Ok(out)
}
