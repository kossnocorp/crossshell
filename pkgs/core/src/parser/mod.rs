use crate::prelude::internal::*;

mod error;
pub use error::*;

pub struct CshParser;

impl CshParser {
    /// Parses Cross Shell source code into AST.
    pub fn parse<'source_code>(
        source_code: &'source_code str,
    ) -> Result<CshAst, CshParserError<'source_code>> {
        let horizontal_space =
            one_of::<_, _, extra::Err<CshError<'_>>>(" \t\r").labelled("whitespace");
        let space = horizontal_space.repeated();

        let quoted = |quote, label| {
            just(quote)
                .map_with(|_, extra| -> SimpleSpan { extra.span() })
                .then(none_of(quote).repeated().collect::<String>())
                .then(just(quote).or_not())
                .try_map(move |((opening, text), closing), span: SimpleSpan| {
                    if closing.is_some() {
                        Ok(text)
                    } else {
                        Err(CshError::UnclosedQuote {
                            quote,
                            opening_span: opening.into_range(),
                            end_span: span.end..span.end,
                        })
                    }
                })
                .labelled(label)
                .as_context()
        };

        let bare = any()
            .filter(|c: &char| !c.is_whitespace() && !"\"'\n;#|&<>()$`\\".contains(*c))
            .repeated()
            .at_least(1)
            .collect::<String>()
            .labelled("unquoted word");

        let unsupported_escape = just('\\').try_map(|_, span: SimpleSpan| {
            Err::<String, _>(CshError::UnsupportedEscape {
                span: span.into_range(),
            })
        });

        let word = choice((
            quoted('"', "double-quoted string"),
            quoted('\'', "single-quoted string"),
            bare,
            unsupported_escape,
        ))
        .repeated()
        .at_least(1)
        .collect::<Vec<_>>()
        .map(|parts| parts.concat())
        .labelled("word")
        .as_context();

        let command = word
            .labelled("command name")
            .then(
                horizontal_space
                    .repeated()
                    .at_least(1)
                    .ignore_then(word.labelled("argument").as_context())
                    .repeated()
                    .collect(),
            )
            .map(|(name, args)| CshAstCommand { name, args })
            .labelled("command")
            .as_context();

        let comment = just('#')
            .then(none_of('\n').repeated())
            .ignored()
            .labelled("comment");

        let line = space
            .ignore_then(command.or_not())
            .then_ignore(space)
            .then_ignore(comment.or_not());

        let parser = line
            .separated_by(one_of("\n;").labelled("command separator"))
            .collect::<Vec<_>>()
            .map(|lines| CshAst {
                commands: lines.into_iter().flatten().collect(),
            });

        parser
            .parse(source_code)
            .into_result()
            .map_err(|diagnostics| CshParserError {
                errors: diagnostics,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_example() {
        let ast = CshParser::parse(include_str!("../../../../examples/00-basic.sh")).unwrap();
        assert_debug_snapshot!(ast, @r#"
        CshAst {
            commands: [
                CshAstCommand {
                    name: "echo",
                    args: [
                        "Hello, cruel world!",
                    ],
                },
            ],
        }
        "#);
    }

    #[test]
    fn parses_comments_separators_and_quotes() {
        let ast = CshParser::parse("# comment\necho '' pre\"fix\" 'a b'  ; pwd # end\n").unwrap();
        assert_debug_snapshot!(ast, @r#"
        CshAst {
            commands: [
                CshAstCommand {
                    name: "echo",
                    args: [
                        "",
                        "prefix",
                        "a b",
                    ],
                },
                CshAstCommand {
                    name: "pwd",
                    args: [],
                },
            ],
        }
        "#);
    }

    #[test]
    fn parses_keyword_as_command() {
        let ast = CshParser::parse("if;").unwrap();
        assert_debug_snapshot!(ast, @r#"
        CshAst {
            commands: [
                CshAstCommand {
                    name: "if",
                    args: [],
                },
            ],
        }
        "#);
    }

    #[test]
    fn rejects_unterminated_quote() {
        let error = CshParser::parse("echo \"unterminated").unwrap_err();
        assert_debug_snapshot!(error, @r#"
        CshParserError {
            errors: [
                UnclosedQuote {
                    quote: '"',
                    opening_span: 5..6,
                    end_span: 18..18,
                },
            ],
        }
        "#);
    }

    #[test]
    fn preserves_byte_spans_for_unclosed_unicode_quote() {
        let error = CshParser::parse("echo héllo '🌍\r\n").unwrap_err();
        assert_debug_snapshot!(error, @r"
        CshParserError {
            errors: [
                UnclosedQuote {
                    quote: '\'',
                    opening_span: 12..13,
                    end_span: 19..19,
                },
            ],
        }
        ");
    }

    #[test]
    fn rejects_unsupported_escape() {
        let error = CshParser::parse("echo \\\"unterminated").unwrap_err();
        assert_debug_snapshot!(error, @"
        CshParserError {
            errors: [
                UnsupportedEscape {
                    span: 5..6,
                },
            ],
        }
        ");
    }

    #[test]
    fn labels_unexpected_operator() {
        let error = CshParser::parse("echo hello | cat").unwrap_err();
        assert_debug_snapshot!(error, @"
        CshParserError {
            errors: [
                Unexpected(
                    found ''|'' at 11..12 expected whitespace, argument, comment, command separator, or end of input in command at 0..11,
                ),
            ],
        }
        ");
    }

    #[test]
    fn parses_multiline_quoted_string() {
        let ast = CshParser::parse("echo 'héllo\n🌍'").unwrap();
        assert_debug_snapshot!(ast, @r#"
        CshAst {
            commands: [
                CshAstCommand {
                    name: "echo",
                    args: [
                        "héllo\n🌍",
                    ],
                },
            ],
        }
        "#);
    }
}
