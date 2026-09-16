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

            CshError::UnsupportedEscape { span } => {
                Report::build(ReportKind::Error, (filename, span.clone()))
                    .with_config(*config)
                    .with_message("Escape sequences are not supported yet")
                    .with_label(
                        Label::new((filename, span.clone()))
                            .with_message("This backslash cannot be used in an unquoted word")
                            .with_color(Color::Red),
                    )
            }

            CshError::Unexpected(error) => {
                // Never send raw control characters from Chumsky's token display
                // to the terminal (notably carriage returns, tabs, and newlines).
                let escaped = error.clone().map_token(|c| c.escape_debug().to_string());

                let found = error.found().map_or_else(
                    || "end of file".to_owned(),
                    |c| format!("`{}`", c.escape_debug()),
                );

                let mut message = format!("Unexpected {found}");
                if let Some((context, _)) = escaped.contexts().next() {
                    message.push_str(&format!(" while parsing {context}"));
                }

                let expected = escaped
                    .expected()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>();

                let label = if expected.is_empty() {
                    "Not valid here".to_owned()
                } else {
                    format!("Expected {}", expected.join(", "))
                };

                Report::build(ReportKind::Error, (filename, error.span().into_range()))
                    .with_config(*config)
                    .with_message(message)
                    .with_label(
                        Label::new((filename, error.span().into_range()))
                            .with_message(label)
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
    fn unsupported_escape() {
        assert_snapshot!(render("echo \\\"unterminated"), @r#"
        Error: Escape sequences are not supported yet
           ╭─[ example.sh:1:6 ]
           │
         1 │ echo \"unterminated
           │      ┬
           │      ╰── This backslash cannot be used in an unquoted word
        ───╯
        "#);
    }

    #[test]
    fn unexpected_token_after_unicode() {
        assert_snapshot!(render("echo \"héllo 🌍\" | cat\n"), @r#"
        Error: Unexpected `|` while parsing command
           ╭─[ example.sh:1:16 ]
           │
         1 │ echo "héllo 🌍" | cat
           │                 ┬
           │                 ╰── Expected whitespace, argument, comment, command separator, end of input
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
        assert_snapshot!(render("echo \u{b}"), @r#"
        Error: Unexpected `\u{b}` while parsing command
           ╭─[ example.sh:1:6 ]
           │
         1 │ echo
           │      ┬
           │      ╰── Expected whitespace, argument, comment, command separator, end of input
        ───╯
        "#);
    }
}
