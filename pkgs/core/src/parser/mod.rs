use crate::prelude::internal::*;

mod error;
pub use error::*;

pub struct CshParser;

impl CshParser {
    /// Parses Cross Shell source code into AST.
    pub fn parse<'source_code>(
        source_code: &'source_code str,
    ) -> Result<CshAst, CshParserError<'source_code>> {
        let parser = recursive(|script| {
            let nonempty = script
                .clone()
                .filter(|ast: &CshAst| !ast.commands.is_empty());
            let space = one_of::<_, _, extra::Err<CshError<'_>>>(" \t\r")
                .ignored()
                .or(just("\\\n").ignored())
                .repeated()
                .labelled("whitespace");
            let continuation = one_of(" \t\r\n").repeated();
            let keyword = |name: &'static str| {
                just(name).then_ignore(
                    any()
                        .filter(|c: &char| !c.is_whitespace() && !";|&()<>".contains(*c))
                        .not(),
                )
            };
            let escape_char =
                just('\\')
                    .ignore_then(any().or_not())
                    .validate(|c, extra, emitter| {
                        c.unwrap_or_else(|| {
                            let span: SimpleSpan = extra.span();
                            emitter.emit(CshError::IncompleteEscape {
                                span: span.into_range(),
                            });
                            '\0'
                        })
                    });
            let escaped = escape_char.map(|c: char| {
                if c == '\n' {
                    String::new()
                } else {
                    c.to_string()
                }
            });
            let single = just('\'')
                .map_with(|_, e| -> SimpleSpan { e.span() })
                .then(none_of('\'').repeated().collect::<String>())
                .then(just('\'').or_not())
                .try_map(|((opening, text), closing), span: SimpleSpan| {
                    closing.map(|_| text).ok_or(CshError::UnclosedQuote {
                        quote: '\'',
                        opening_span: opening.into_range(),
                        end_span: span.end..span.end,
                    })
                });
            // Balanced expansion text is retained verbatim; expansion happens at execution time.
            let balanced = recursive(|inner| {
                choice((
                    just('\\').then(any()).ignored(),
                    single.ignored(),
                    inner
                        .clone()
                        .repeated()
                        .delimited_by(just('('), just(')'))
                        .ignored(),
                    inner
                        .clone()
                        .repeated()
                        .delimited_by(just('{'), just('}'))
                        .ignored(),
                    just('"')
                        .ignore_then(
                            choice((just('\\').then(any()).ignored(), none_of("\"\\").ignored()))
                                .repeated(),
                        )
                        .then_ignore(just('"'))
                        .ignored(),
                    none_of("\\'\"(){}").ignored(),
                ))
            });
            let arithmetic = just("$((")
                .ignore_then(balanced.clone().repeated())
                .then_ignore(just("))"))
                .to_slice();
            let substitution = just("$(")
                .ignore_then(script.clone())
                .then_ignore(just(')'))
                .to_slice();
            let process_substitution = one_of("<>")
                .then(just('('))
                .ignore_then(script.clone())
                .then_ignore(just(')'))
                .to_slice();
            let parameter = just("${")
                .ignore_then(balanced.clone().repeated())
                .then_ignore(just('}'))
                .to_slice();
            let backtick = just('`')
                .ignore_then(
                    choice((just('\\').then(any()).ignored(), none_of('`').ignored())).repeated(),
                )
                .then_ignore(just('`'))
                .to_slice();
            let expansion = choice((arithmetic, substitution, parameter, backtick))
                .map(str::to_owned)
                .boxed();
            let dollar = just('$').then_ignore(one_of("({").not()).to("$".to_owned());
            let double_escape = escape_char.map(|c| match c {
                '\n' => String::new(),
                '$' | '`' | '"' | '\\' => c.to_string(),
                _ => format!("\\{c}"),
            });
            let double = just('"')
                .map_with(|_, e| -> SimpleSpan { e.span() })
                .then(
                    choice((
                        expansion.clone(),
                        double_escape,
                        dollar.clone(),
                        none_of("\"\\$`").map(|c: char| c.to_string()),
                    ))
                    .repeated()
                    .collect::<Vec<_>>(),
                )
                .then(just('"').or_not())
                .try_map(|((opening, parts), closing), span: SimpleSpan| {
                    closing
                        .map(|_| parts.concat())
                        .ok_or(CshError::UnclosedQuote {
                            quote: '"',
                            opening_span: opening.into_range(),
                            end_span: span.end..span.end,
                        })
                });
            let glob = one_of("?*+@!")
                .then(just('('))
                .ignore_then(balanced.repeated())
                .then_ignore(just(')'))
                .to_slice()
                .map(str::to_owned);
            let bare = one_of("?*+@!")
                .then(just('('))
                .not()
                .ignore_then(
                    any().filter(|c: &char| !c.is_whitespace() && !"\"';|&<>()$`\\".contains(*c)),
                )
                .repeated()
                .at_least(1)
                .collect::<String>();
            let word = just('#')
                .not()
                .ignore_then(
                    choice((
                        single,
                        double,
                        expansion,
                        process_substitution.map(str::to_owned),
                        escaped,
                        glob,
                        bare,
                        dollar,
                    ))
                    .repeated()
                    .at_least(1)
                    .collect::<Vec<_>>(),
                )
                .map(|parts| parts.concat())
                .labelled("word")
                .as_context()
                .boxed();
            let redirect = one_of("0123456789")
                .repeated()
                .at_least(1)
                .collect::<String>()
                .or_not()
                .then(choice((
                    just("&>>"),
                    just("<<<"),
                    just(">>"),
                    just("<<-"),
                    just("<<"),
                    just("<&"),
                    just(">&"),
                    just("&>"),
                    just(">|"),
                    just("<>"),
                    just(">"),
                    just("<"),
                )))
                .then_ignore(space)
                .then(word.clone())
                .map(|((descriptor, operator), target)| CshAstRedirect {
                    descriptor,
                    operator: operator.to_owned(),
                    target,
                })
                .labelled("redirection");
            let item = redirect
                .clone()
                .map(Err)
                .or(word
                    .clone()
                    .map_with(|word, extra| Ok((word, extra.slice()))))
                .padded_by(space);
            let reserved = choice((
                keyword("if"),
                keyword("then"),
                keyword("elif"),
                keyword("else"),
                keyword("fi"),
                keyword("for"),
                keyword("do"),
                keyword("done"),
                keyword("while"),
                keyword("until"),
                keyword("case"),
                keyword("esac"),
                keyword("{"),
                keyword("}"),
            ));
            let simple = reserved
                .not()
                .ignore_then(item.repeated().at_least(1).collect::<Vec<_>>())
                .map(|items| {
                    let mut words = Vec::new();
                    let mut assignments = Vec::new();
                    let mut redirects = Vec::new();
                    for item in items {
                        match item {
                            Ok((word, raw)) => {
                                let assignment = raw.split_once('=').filter(|(name, _)| {
                                    !name.is_empty()
                                        && name.chars().enumerate().all(|(i, c)| {
                                            c == '_'
                                                || c.is_ascii_alphabetic()
                                                || (i > 0 && c.is_ascii_digit())
                                        })
                                });
                                if words.is_empty()
                                    && let Some((name, _)) = assignment
                                {
                                    assignments.push(CshAstAssignment {
                                        name: name.to_owned(),
                                        value: word[name.len() + 1..].to_owned(),
                                    });
                                } else {
                                    words.push(word);
                                }
                            }
                            Err(redirect) => redirects.push(redirect),
                        }
                    }
                    let mut words = words.into_iter();
                    let expression = CshAstExpression::Command(CshAstCommand {
                        assignments,
                        name: words.next().unwrap_or_default(),
                        args: words.collect(),
                    });
                    if redirects.is_empty() {
                        expression
                    } else {
                        CshAstExpression::Redirected {
                            expression: Box::new(expression),
                            redirects,
                        }
                    }
                });
            let branch = nonempty
                .clone()
                .then_ignore(keyword("then").padded_by(continuation))
                .then(nonempty.clone())
                .map(|(condition, body)| CshAstBranch { condition, body });
            let conditional = keyword("if")
                .then_ignore(space)
                .ignore_then(branch.clone())
                .then(
                    keyword("elif")
                        .then_ignore(space)
                        .ignore_then(branch)
                        .repeated()
                        .collect::<Vec<_>>(),
                )
                .then(
                    keyword("else")
                        .padded_by(continuation)
                        .ignore_then(nonempty.clone())
                        .or_not(),
                )
                .then_ignore(keyword("fi"))
                .map(|((first, rest), otherwise)| CshAstExpression::If {
                    branches: std::iter::once(first).chain(rest).collect(),
                    otherwise,
                });
            let for_loop = keyword("for")
                .then_ignore(space)
                .ignore_then(word.clone())
                .then(
                    keyword("in")
                        .padded_by(space)
                        .ignore_then(word.clone().padded_by(space).repeated().collect::<Vec<_>>())
                        .or_not(),
                )
                .then_ignore(space)
                .then_ignore(one_of(";\n"))
                .then_ignore(continuation)
                .then_ignore(keyword("do"))
                .then_ignore(continuation)
                .then(nonempty.clone())
                .then_ignore(keyword("done"))
                .map(|((variable, words), body)| CshAstExpression::For {
                    variable,
                    words,
                    body,
                });
            let condition_loop = choice((keyword("while").to(false), keyword("until").to(true)))
                .then_ignore(space)
                .then(nonempty.clone())
                .then_ignore(keyword("do").padded_by(continuation))
                .then(nonempty.clone())
                .then_ignore(keyword("done"))
                .map(|((until, condition), body)| CshAstExpression::Loop {
                    until,
                    condition,
                    body,
                });
            let case_arm = keyword("esac")
                .not()
                .ignore_then(just('(').or_not())
                .ignore_then(
                    word.clone()
                        .padded_by(space)
                        .separated_by(just('|'))
                        .at_least(1)
                        .collect::<Vec<_>>(),
                )
                .then_ignore(just(')'))
                .then(script.clone())
                .then(choice((just(";;&"), just(";;"), just(";&"))).or_not())
                .then_ignore(continuation)
                .map(|((patterns, body), terminator)| CshAstCaseArm {
                    patterns,
                    body,
                    terminator: terminator.unwrap_or("").to_owned(),
                });
            let case = keyword("case")
                .then_ignore(space)
                .ignore_then(word.clone())
                .then_ignore(space)
                .then_ignore(keyword("in"))
                .then_ignore(continuation)
                .then(case_arm.repeated().collect::<Vec<_>>())
                .then_ignore(keyword("esac"))
                .map(|(word, arms)| CshAstExpression::Case { word, arms });
            let compound = choice((
                nonempty
                    .clone()
                    .delimited_by(just('('), just(')'))
                    .map(CshAstExpression::Subshell),
                nonempty
                    .clone()
                    .delimited_by(keyword("{"), keyword("}"))
                    .map(CshAstExpression::Group),
                conditional,
                for_loop,
                condition_loop,
                case,
            ))
            .then(redirect.padded_by(space).repeated().collect::<Vec<_>>())
            .map(|(expression, redirects)| {
                if redirects.is_empty() {
                    expression
                } else {
                    CshAstExpression::Redirected {
                        expression: Box::new(expression),
                        redirects,
                    }
                }
            });
            let command = compound
                .or(simple)
                .padded_by(space)
                .labelled("command")
                .as_context();
            let binary = |left, (operator, right)| CshAstExpression::Binary {
                left: Box::new(left),
                operator,
                right: Box::new(right),
            };
            let pipeline = command.clone().foldl(
                choice((
                    just("|&").to(CshAstOperator::PipeWithStderr),
                    just('|')
                        .then_ignore(just('|').not())
                        .to(CshAstOperator::Pipe),
                ))
                .labelled("pipe operator")
                .then_ignore(continuation)
                .then(command)
                .repeated(),
                binary,
            );
            let pipeline = keyword("!").padded_by(space).or_not().then(pipeline).map(
                |(negated, expression)| {
                    if negated.is_some() {
                        CshAstExpression::Negated(Box::new(expression))
                    } else {
                        expression
                    }
                },
            );
            let expression = pipeline.clone().foldl(
                choice((
                    just("&&").to(CshAstOperator::And),
                    just("||").to(CshAstOperator::Or),
                ))
                .labelled("logical operator")
                .then_ignore(continuation)
                .then(pipeline)
                .repeated(),
                binary,
            );
            let comment = just('#')
                .then(none_of('\n').repeated())
                .ignored()
                .labelled("comment");
            let separator = just(';')
                .then_ignore(one_of(";&").not())
                .or(just('\n'))
                .to(false)
                .or(just('&').then_ignore(just('&').not()).to(true))
                .labelled("command separator");
            let line = expression
                .or_not()
                .then_ignore(space)
                .then_ignore(comment.or_not());
            // Parse each line once. Repeating (line, separator) before a final
            // line reparses the last command whenever the separator is absent.
            space
                .ignore_then(line.clone())
                .then(separator.then(line).repeated().collect::<Vec<_>>())
                .map(|(mut current, following)| {
                    let mut commands = Vec::new();
                    for (background, next) in following {
                        if let Some(expression) = current {
                            commands.push(if background {
                                CshAstExpression::Background(Box::new(expression))
                            } else {
                                expression
                            });
                        }
                        current = next;
                    }
                    commands.extend(current);
                    CshAst { commands }
                })
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
    fn preserves_operator_precedence() {
        let ast = CshParser::parse("a || b && c | d & e |& f").unwrap();
        assert_debug_snapshot!(ast, @r#"
        CshAst {
            commands: [
                Background(
                    Binary {
                        left: Binary {
                            left: Command(
                                CshAstCommand {
                                    assignments: [],
                                    name: "a",
                                    args: [],
                                },
                            ),
                            operator: Or,
                            right: Command(
                                CshAstCommand {
                                    assignments: [],
                                    name: "b",
                                    args: [],
                                },
                            ),
                        },
                        operator: And,
                        right: Binary {
                            left: Command(
                                CshAstCommand {
                                    assignments: [],
                                    name: "c",
                                    args: [],
                                },
                            ),
                            operator: Pipe,
                            right: Command(
                                CshAstCommand {
                                    assignments: [],
                                    name: "d",
                                    args: [],
                                },
                            ),
                        },
                    },
                ),
                Binary {
                    left: Command(
                        CshAstCommand {
                            assignments: [],
                            name: "e",
                            args: [],
                        },
                    ),
                    operator: PipeWithStderr,
                    right: Command(
                        CshAstCommand {
                            assignments: [],
                            name: "f",
                            args: [],
                        },
                    ),
                },
            ],
        }
        "#);
    }

    #[test]
    fn parses_assignments_redirects_and_subshells() {
        let ast = CshParser::parse("MODE=test COUNT=2 (echo hi)");
        assert!(ast.is_err(), "Assignments cannot prefix a subshell");
        let ast =
            CshParser::parse("MODE=test >out cmd arg 2>&1; (echo hi; cat <out) >>log").unwrap();
        assert_debug_snapshot!(ast, @r#"
        CshAst {
            commands: [
                Redirected {
                    expression: Command(
                        CshAstCommand {
                            assignments: [
                                CshAstAssignment {
                                    name: "MODE",
                                    value: "test",
                                },
                            ],
                            name: "cmd",
                            args: [
                                "arg",
                            ],
                        },
                    ),
                    redirects: [
                        CshAstRedirect {
                            descriptor: None,
                            operator: ">",
                            target: "out",
                        },
                        CshAstRedirect {
                            descriptor: Some(
                                "2",
                            ),
                            operator: ">&",
                            target: "1",
                        },
                    ],
                },
                Redirected {
                    expression: Subshell(
                        CshAst {
                            commands: [
                                Command(
                                    CshAstCommand {
                                        assignments: [],
                                        name: "echo",
                                        args: [
                                            "hi",
                                        ],
                                    },
                                ),
                                Redirected {
                                    expression: Command(
                                        CshAstCommand {
                                            assignments: [],
                                            name: "cat",
                                            args: [],
                                        },
                                    ),
                                    redirects: [
                                        CshAstRedirect {
                                            descriptor: None,
                                            operator: "<",
                                            target: "out",
                                        },
                                    ],
                                },
                            ],
                        },
                    ),
                    redirects: [
                        CshAstRedirect {
                            descriptor: None,
                            operator: ">>",
                            target: "log",
                        },
                    ],
                },
            ],
        }
        "#);
    }

    #[test]
    fn parses_word_expansions_and_escapes() {
        let ast = CshParser::parse(r#"echo pre"fix" a\ b \"x\" '\literal' "\q\"" ${v:-"a)b"} "$(echo "nested")" $((1 + (2))) `pwd` <(cat x) ./{cjs,esm}/!(package.json) foo#bar "${x}"
echo a\
b # comment"#).unwrap();
        assert_debug_snapshot!(ast, @r#"
        CshAst {
            commands: [
                Command(
                    CshAstCommand {
                        assignments: [],
                        name: "echo",
                        args: [
                            "prefix",
                            "a b",
                            "\"x\"",
                            "\\literal",
                            "\\q\"",
                            "${v:-\"a)b\"}",
                            "$(echo \"nested\")",
                            "$((1 + (2)))",
                            "`pwd`",
                            "<(cat x)",
                            "./{cjs,esm}/!(package.json)",
                            "foo#bar",
                            "${x}",
                        ],
                    },
                ),
                Command(
                    CshAstCommand {
                        assignments: [],
                        name: "echo",
                        args: [
                            "ab",
                        ],
                    },
                ),
            ],
        }
        "#);
    }

    #[test]
    fn parses_nested_control_flow() {
        let ast = CshParser::parse("for f in *.js; do if test -f \"$f\"; then echo \"$f\"; elif false; then break; else continue; fi; done; until ready; do sleep 1; done; while ! ready; do sleep 1; done").unwrap();
        assert_debug_snapshot!(ast, @r#"
        CshAst {
            commands: [
                For {
                    variable: "f",
                    words: Some(
                        [
                            "*.js",
                        ],
                    ),
                    body: CshAst {
                        commands: [
                            If {
                                branches: [
                                    CshAstBranch {
                                        condition: CshAst {
                                            commands: [
                                                Command(
                                                    CshAstCommand {
                                                        assignments: [],
                                                        name: "test",
                                                        args: [
                                                            "-f",
                                                            "$f",
                                                        ],
                                                    },
                                                ),
                                            ],
                                        },
                                        body: CshAst {
                                            commands: [
                                                Command(
                                                    CshAstCommand {
                                                        assignments: [],
                                                        name: "echo",
                                                        args: [
                                                            "$f",
                                                        ],
                                                    },
                                                ),
                                            ],
                                        },
                                    },
                                    CshAstBranch {
                                        condition: CshAst {
                                            commands: [
                                                Command(
                                                    CshAstCommand {
                                                        assignments: [],
                                                        name: "false",
                                                        args: [],
                                                    },
                                                ),
                                            ],
                                        },
                                        body: CshAst {
                                            commands: [
                                                Command(
                                                    CshAstCommand {
                                                        assignments: [],
                                                        name: "break",
                                                        args: [],
                                                    },
                                                ),
                                            ],
                                        },
                                    },
                                ],
                                otherwise: Some(
                                    CshAst {
                                        commands: [
                                            Command(
                                                CshAstCommand {
                                                    assignments: [],
                                                    name: "continue",
                                                    args: [],
                                                },
                                            ),
                                        ],
                                    },
                                ),
                            },
                        ],
                    },
                },
                Loop {
                    until: true,
                    condition: CshAst {
                        commands: [
                            Command(
                                CshAstCommand {
                                    assignments: [],
                                    name: "ready",
                                    args: [],
                                },
                            ),
                        ],
                    },
                    body: CshAst {
                        commands: [
                            Command(
                                CshAstCommand {
                                    assignments: [],
                                    name: "sleep",
                                    args: [
                                        "1",
                                    ],
                                },
                            ),
                        ],
                    },
                },
                Loop {
                    until: false,
                    condition: CshAst {
                        commands: [
                            Negated(
                                Command(
                                    CshAstCommand {
                                        assignments: [],
                                        name: "ready",
                                        args: [],
                                    },
                                ),
                            ),
                        ],
                    },
                    body: CshAst {
                        commands: [
                            Command(
                                CshAstCommand {
                                    assignments: [],
                                    name: "sleep",
                                    args: [
                                        "1",
                                    ],
                                },
                            ),
                        ],
                    },
                },
            ],
        }
        "#);
    }

    #[test]
    fn parses_case_and_group() {
        let ast = CshParser::parse(
            "case \"$choice\" in y|Y) { build; test; };; *) echo abort; exit 1;; esac",
        )
        .unwrap();
        assert_debug_snapshot!(ast, @r#"
        CshAst {
            commands: [
                Case {
                    word: "$choice",
                    arms: [
                        CshAstCaseArm {
                            patterns: [
                                "y",
                                "Y",
                            ],
                            body: CshAst {
                                commands: [
                                    Group(
                                        CshAst {
                                            commands: [
                                                Command(
                                                    CshAstCommand {
                                                        assignments: [],
                                                        name: "build",
                                                        args: [],
                                                    },
                                                ),
                                                Command(
                                                    CshAstCommand {
                                                        assignments: [],
                                                        name: "test",
                                                        args: [],
                                                    },
                                                ),
                                            ],
                                        },
                                    ),
                                ],
                            },
                            terminator: ";;",
                        },
                        CshAstCaseArm {
                            patterns: [
                                "*",
                            ],
                            body: CshAst {
                                commands: [
                                    Command(
                                        CshAstCommand {
                                            assignments: [],
                                            name: "echo",
                                            args: [
                                                "abort",
                                            ],
                                        },
                                    ),
                                    Command(
                                        CshAstCommand {
                                            assignments: [],
                                            name: "exit",
                                            args: [
                                                "1",
                                            ],
                                        },
                                    ),
                                ],
                            },
                            terminator: ";;",
                        },
                    ],
                },
            ],
        }
        "#);
    }

    #[test]
    fn rejects_incomplete_syntax() {
        for source in [
            "echo &&",
            "echo |",
            "echo ||| cat",
            "echo >",
            "echo $(pwd",
            "echo ${value",
            "echo \"${value\"",
            "echo `pwd",
            "(echo hi",
            "echo hi)",
            "if;",
            "if true; then",
            "if then fi",
            "for x in a; do",
            "for %x in (a b) do echo %x",
            "case x in a) echo a;;",
            "{ echo hi;",
            "()",
            "while; do; done",
        ] {
            assert!(
                CshParser::parse(source).is_err(),
                "Unexpectedly accepted {source:?}"
            );
        }
    }

    #[test]
    fn parses_basic_example() {
        let ast = CshParser::parse(include_str!("../../../../examples/00-basic.sh")).unwrap();
        assert_debug_snapshot!(ast, @r#"
        CshAst {
            commands: [
                Command(
                    CshAstCommand {
                        assignments: [],
                        name: "echo",
                        args: [
                            "Hello, cruel world!",
                        ],
                    },
                ),
            ],
        }
        "#);
    }

    #[test]
    fn parses_smoke_test_data() {
        let scripts = include_str!("../../test/smoke/data/top-npm-scripts.jsonl").lines();
        let mut asts = std::collections::BTreeMap::new();
        let mut rejections = std::collections::BTreeMap::new();
        let mut accepted = 0;
        for (index, record) in scripts.enumerate() {
            let record: serde_json::Value = serde_json::from_str(record).unwrap();
            let script = record["script"].as_str().unwrap();
            let result = CshParser::parse(script);
            if let Some(reason) = record["rejection"].as_str() {
                let error = result.expect_err(&format!(
                    "Expected rejection of script #{index} ({reason}):\n{script}"
                ));
                let mut report = Vec::new();
                CshErrorReport::new(script, "script.sh", &error.errors, false)
                    .write(&mut report)
                    .unwrap();
                let report = String::from_utf8(report)
                    .unwrap()
                    .lines()
                    .map(str::trim_end)
                    .collect::<Vec<_>>()
                    .join("\n");
                assert!(
                    rejections.insert(script.to_owned(), report).is_none(),
                    "Duplicate rejection #{index} ({reason}): {script}"
                );
                continue;
            }
            match result {
                Ok(ast) => {
                    accepted += 1;
                    asts.insert(script.to_owned(), ast);
                }
                Err(error) => panic!("Failed to parse script #{index}:\n{script}\n{error:#?}"),
            }
        }
        assert_debug_snapshot!((accepted, asts.len(), rejections.len()), @"
        (
            32710,
            32710,
            9,
        )
        ");
        // Debug snapshots preserve map iteration order; BTreeMap sorts by script.
        assert_debug_snapshot!("smoke_test_data", asts);
        // Debug/YAML escape embedded newlines by default. Keep Ariadne's layout
        // intact in a text mapping so the reports can be read directly.
        let reports = rejections
            .iter()
            .map(|(script, report)| {
                let report = report
                    .lines()
                    .map(|line| format!("    {line}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                format!("{script:?} =>\n{report}")
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        assert_snapshot!("smoke_test_rejections", reports);
    }

    #[test]
    fn parses_comments_separators_and_quotes() {
        let ast = CshParser::parse("# comment\necho '' pre\"fix\" 'a b'  ; pwd # end\n").unwrap();
        assert_debug_snapshot!(ast, @r#"
        CshAst {
            commands: [
                Command(
                    CshAstCommand {
                        assignments: [],
                        name: "echo",
                        args: [
                            "",
                            "prefix",
                            "a b",
                        ],
                    },
                ),
                Command(
                    CshAstCommand {
                        assignments: [],
                        name: "pwd",
                        args: [],
                    },
                ),
            ],
        }
        "#);
    }

    #[test]
    fn parses_quoted_keyword_as_command() {
        let ast = CshParser::parse("'if';").unwrap();
        assert_debug_snapshot!(ast, @r#"
        CshAst {
            commands: [
                Command(
                    CshAstCommand {
                        assignments: [],
                        name: "if",
                        args: [],
                    },
                ),
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
    fn rejects_incomplete_escape() {
        let error = CshParser::parse("echo \\").unwrap_err();
        assert_debug_snapshot!(error, @"
        CshParserError {
            errors: [
                IncompleteEscape {
                    span: 5..6,
                },
            ],
        }
        ");
    }

    #[test]
    fn labels_unexpected_operator() {
        let error = CshParser::parse("echo hello ||| cat").unwrap_err();
        assert_debug_snapshot!(error, @r"
        CshParserError {
            errors: [
                Unexpected(
                    found ''|'' at 13..14 expected '' '', ''\t'', ''\r'', ''\n'', whitespace, ''!'', or command,
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
                Command(
                    CshAstCommand {
                        assignments: [],
                        name: "echo",
                        args: [
                            "héllo\n🌍",
                        ],
                    },
                ),
            ],
        }
        "#);
    }
}
