use crate::prelude::internal::*;
use ariadne::{Color, Config, IndexType, Label, Report, ReportKind, Source};
use std::io;

pub struct CshErrorReport<'a> {
    source_code: &'a str,
    filename: &'a str,
    errors: &'a Vec<CshError<'a>>,
    config: Config,
}

impl<'a> CshErrorReport<'a> {
    pub fn new(
        source_code: &'a str,
        filename: &'a str,
        errors: &'a Vec<CshError<'a>>,
        color: bool,
    ) -> Self {
        let config = Config::default()
            .with_index_type(IndexType::Byte)
            .with_color(color);

        Self {
            source_code,
            filename,
            errors,
            config,
        }
    }

    pub fn write(&self, mut writer: impl io::Write) -> io::Result<()> {
        for error in self.errors {
            let report = error.report(self.filename, &self.config);

            report
                .finish()
                .write((self.filename, Source::from(self.source_code)), &mut writer)?;
        }

        Ok(())
    }
}

impl<'a> CshError<'a> {
    pub fn report(
        &self,
        filename: &'a str,
        config: &Config,
    ) -> ReportBuilder<'a, (&'a str, Range<usize>)> {
        match self {
            CshError::UnclosedQuote {
                quote,
                opening_span,
                end_span,
            } => {
                let kind = if *quote == '"' { "double" } else { "single" };
                Report::build(ReportKind::Error, (filename, opening_span.clone()))
                    .with_config(*config)
                    .with_message(format!("Unclosed {kind}-quoted string"))
                    .with_label(
                        Label::new((filename, opening_span.clone()))
                            .with_message("String starts here")
                            .with_color(Color::Yellow),
                    )
                    .with_label(
                        Label::new((filename, end_span.clone()))
                            .with_message(format!(
                                "Expected a closing `{quote}` before end of file"
                            ))
                            .with_color(Color::Red),
                    )
            }

            CshError::IncompleteEscape { span } => {
                Report::build(ReportKind::Error, (filename, span.clone()))
                    .with_config(*config)
                    .with_message("Incomplete escape sequence")
                    .with_label(
                        Label::new((filename, span.clone()))
                            .with_message("Expected a character after this backslash")
                            .with_color(Color::Red),
                    )
            }

            CshError::Unexpected {
                found,
                span,
                expected,
            } => {
                let found = if found.is_empty() {
                    "end of file".to_owned()
                } else {
                    format!("`{}`", found.escape_debug())
                };
                Report::build(ReportKind::Error, (filename, span.clone()))
                    .with_config(*config)
                    .with_message(format!("Unexpected {found}"))
                    .with_label(
                        Label::new((filename, span.clone()))
                            .with_message(format!("Expected {expected}"))
                            .with_color(Color::Red),
                    )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(source: &str) -> String {
        let error = CshParser::parse(source).unwrap_err();
        let mut output = Vec::new();
        CshErrorReport::new(source, "example.sh", &error.errors, false)
            .write(&mut output)
            .unwrap();
        String::from_utf8(output)
            .unwrap()
            .lines()
            .map(str::trim_end)
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn unclosed_quote() {
        assert_snapshot!(render("echo \"unterminated"), @r#"
        Error: Unclosed double-quoted string
           ╭─[ example.sh:1:6 ]
           │
         1 │ echo "unterminated
           │      ┬            │
           │      ╰────────────── String starts here
           │                   │
           │                   ╰─ Expected a closing `"` before end of file
        ───╯
        "#);
    }

    #[test]
    fn incomplete_escape() {
        assert_snapshot!(render("echo \\"), @r"
        Error: Incomplete escape sequence
           ╭─[ example.sh:1:6 ]
           │
         1 │ echo \
           │      ┬
           │      ╰── Expected a character after this backslash
        ───╯
        ");
    }

    #[test]
    fn unexpected_token_after_unicode() {
        assert_snapshot!(render("echo \"héllo 🌍\" ||| cat\n"), @r#"
        Error: Unexpected `|`
           ╭─[ example.sh:1:18 ]
           │
         1 │ echo "héllo 🌍" ||| cat
           │                   ┬
           │                   ╰── Expected a command
        ───╯
        "#);
    }

    #[test]
    fn unclosed_multiline_single_quote() {
        assert_snapshot!(render("echo 'héllo\n🌍"), @r#"
        Error: Unclosed single-quoted string
           ╭─[ example.sh:1:6 ]
           │
         1 │ echo 'héllo
           │      ┬
           │      ╰── String starts here
         2 │ 🌍
           │   │
           │   ╰─ Expected a closing `'` before end of file
        ───╯
        "#);
    }

    #[test]
    fn escapes_control_characters_in_diagnostics() {
        assert_snapshot!(render("echo \u{b}"), @r"
        Error: Unexpected `\u{b}`
           ╭─[ example.sh:1:6 ]
           │
         1 │ echo
           │      ┬
           │      ╰── Expected a command separator
        ───╯
        ");
    }
}
