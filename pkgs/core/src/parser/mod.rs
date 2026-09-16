use crate::prelude::internal::*;

mod error;
pub use error::*;
mod cursor;

pub struct CshParser;

impl CshParser {
    /// Parses Cross Shell source into an owned, arena-backed AST.
    ///
    /// Expression IDs index `CshAst::nodes`; the AST does not borrow the source.
    /// Diagnostics borrow the source and use UTF-8 byte offsets. Recursive
    /// command-list nesting is limited to 128 levels; operator chains are iterative.
    pub fn parse<'source_code>(
        source_code: &'source_code str,
    ) -> Result<CshAst, CshParserError<'source_code>> {
        cursor::Cursor::parse(source_code).map_err(|error| CshParserError {
            errors: vec![error],
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
    fn arena_links_survive_growth_and_source_drop() {
        let ast = {
            let source = (0..256)
                .map(|i| format!("echo {i}"))
                .collect::<Vec<_>>()
                .join(" | ");
            CshParser::parse(&source).unwrap()
        };
        assert_eq!(ast.nodes.len(), 511);
        let mut id = ast.commands[0];
        for expected in (1..256).rev() {
            let CshAstExpression::Binary {
                left,
                operator,
                right,
            } = &ast[id]
            else {
                panic!("expected a pipeline node");
            };
            assert_eq!(*operator, CshAstOperator::Pipe);
            let CshAstExpression::Command(command) = &ast[*right] else {
                panic!("expected a command");
            };
            assert_eq!(command.args, [expected.to_string()]);
            id = *left;
        }
        let CshAstExpression::Command(command) = &ast[id] else {
            panic!("expected the first command");
        };
        assert_eq!(command.args, ["0"]);
    }

    #[test]
    fn validates_substitutions_and_reclaims_temporary_nodes() {
        let ast = CshParser::parse("echo $(printf '%s' $(pwd)) <(cat file) && done_cmd").unwrap();
        assert_eq!(ast.nodes.len(), 3);
        assert_eq!(ast.commands, [CshAstNodeId(2)]);
        let CshAstExpression::Command(command) = &ast[CshAstNodeId(0)] else {
            panic!("expected echo");
        };
        assert_eq!(command.args, ["$(printf '%s' $(pwd))", "<(cat file)"]);
        for source in [
            "echo $(echo |)",
            "echo <(if true; then fi)",
            "echo ${x)}",
            "echo $((1 + (2))",
            "echo @(a|b",
        ] {
            assert!(CshParser::parse(source).is_err(), "accepted {source:?}");
        }
    }

    #[test]
    fn bounds_recursive_nesting_but_handles_long_operator_chains() {
        let nested = format!("{}echo{}", "(".repeat(128), ")".repeat(128));
        let error = CshParser::parse(&nested).unwrap_err();
        assert!(matches!(
            error.errors[0],
            CshError::Unexpected {
                expected: "at most 128 nested command lists",
                ..
            }
        ));

        let source = std::iter::repeat_n("true", 10_000)
            .collect::<Vec<_>>()
            .join(" && ");
        let ast = CshParser::parse(&source).unwrap();
        assert_eq!(ast.nodes.len(), 19_999);
        drop(ast); // Arena destruction must not recurse through the operator chain.
    }

    #[test]
    fn keeps_keywords_descriptors_and_word_fragments_distinct() {
        let ast =
            CshParser::parse("ifconfig 'if' a#b 2file 2>out <(pwd) pre\"héllo\"'🌍'").unwrap();
        let CshAstExpression::Redirected {
            expression,
            redirects,
        } = &ast[ast.commands[0]]
        else {
            panic!("expected redirect");
        };
        assert_eq!(redirects[0].descriptor.as_deref(), Some("2"));
        assert_eq!(redirects[0].target, "out");
        let CshAstExpression::Command(command) = &ast[*expression] else {
            panic!("expected command");
        };
        assert_eq!(command.name, "ifconfig");
        assert_eq!(command.args, ["if", "a#b", "2file", "<(pwd)", "prehéllo🌍"]);
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
        assert_debug_snapshot!(error, @r#"
        CshParserError {
            errors: [
                Unexpected {
                    found: "|",
                    span: 13..14,
                    expected: "a command",
                },
            ],
        }
        "#);
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
