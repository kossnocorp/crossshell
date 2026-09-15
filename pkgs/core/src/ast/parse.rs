use crate::prelude::internal::*;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("Parsing failed with {} error(s)", .errors.len())]
pub struct CshAstParseError {
    pub errors: Vec<Rich<'static, char>>,
}

impl CshAst {
    /// Parses Cross Shell source code into AST.
    pub fn parse(source_code: &str) -> Result<Self, CshAstParseError> {
        let space = one_of::<_, _, extra::Err<Rich<'_, char>>>(" \t\r").repeated();

        let quoted = |quote| {
            none_of(quote)
                .repeated()
                .collect::<String>()
                .delimited_by(just(quote), just(quote))
        };

        let bare = any()
            .filter(|c: &char| !c.is_whitespace() && !"\"'\n;#|&<>()$`\\".contains(*c))
            .repeated()
            .at_least(1)
            .collect::<String>();

        let word = choice((quoted('"'), quoted('\''), bare))
            .repeated()
            .at_least(1)
            .collect::<Vec<_>>()
            .map(|parts| parts.concat());

        let command = word
            .then(space.at_least(1).ignore_then(word).repeated().collect())
            .map(|(name, args)| CshAstCommand { name, args });

        let comment = just('#').then(none_of('\n').repeated()).ignored();

        let line = space
            .ignore_then(command.or_not())
            .then_ignore(space)
            .then_ignore(comment.or_not());

        let parser = line
            .separated_by(one_of("\n;"))
            .collect::<Vec<_>>()
            .map(|lines| Self {
                commands: lines.into_iter().flatten().collect(),
            });

        parser
            .parse(source_code)
            .into_result()
            .map_err(|errors: Vec<Rich<'_, char>>| CshAstParseError {
                errors: errors.into_iter().map(Rich::into_owned).collect(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_example() {
        let ast = CshAst::parse(include_str!("../../../../examples/00-basic.sh")).unwrap();
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
        let ast = CshAst::parse("# comment\necho '' pre\"fix\" 'a b'  ; pwd # end\n").unwrap();
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
        let ast = CshAst::parse("if;").unwrap();
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
        let error = CshAst::parse("echo \"unterminated").unwrap_err();
        assert_debug_snapshot!(error, @r#"
        CshAstParseError {
            errors: [
                found end of input at 18..18 expected something else, or ''"'',
            ],
        }
        "#);
    }
}
