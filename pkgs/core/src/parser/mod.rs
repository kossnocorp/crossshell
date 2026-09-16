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
    fn structures_associative_arrays_and_append_assignments() {
        let ast = CshParser::parse(
            r#"declare -A VULKAN_DRIVERS=(
  [Intel]=vulkan-intel
  [AMD]=vulkan-radeon
  [Apple]=vulkan-asahi
)
PACKAGES=()
PACKAGES+=("${VULKAN_DRIVERS[$vendor]}")"#,
        )
        .unwrap();
        assert_debug_snapshot!(ast, @r#"
        CshAst {
            commands: [
                Command(
                    CshAstCommand {
                        assignments: [],
                        name: Some(
                            Literal(
                                "declare",
                            ),
                        ),
                        args: [
                            Literal(
                                "-A",
                            ),
                            Assignment(
                                CshAstAssignment {
                                    name: "VULKAN_DRIVERS",
                                    operator: Set,
                                    value: Array(
                                        [
                                            KeyedElement {
                                                key: Literal(
                                                    "Intel",
                                                ),
                                                operator: Set,
                                                value: Literal(
                                                    "vulkan-intel",
                                                ),
                                            },
                                            KeyedElement {
                                                key: Literal(
                                                    "AMD",
                                                ),
                                                operator: Set,
                                                value: Literal(
                                                    "vulkan-radeon",
                                                ),
                                            },
                                            KeyedElement {
                                                key: Literal(
                                                    "Apple",
                                                ),
                                                operator: Set,
                                                value: Literal(
                                                    "vulkan-asahi",
                                                ),
                                            },
                                        ],
                                    ),
                                },
                            ),
                        ],
                    },
                ),
                Command(
                    CshAstCommand {
                        assignments: [
                            CshAstAssignment {
                                name: "PACKAGES",
                                operator: Set,
                                value: Array(
                                    [],
                                ),
                            },
                        ],
                        name: None,
                        args: [],
                    },
                ),
                Command(
                    CshAstCommand {
                        assignments: [
                            CshAstAssignment {
                                name: "PACKAGES",
                                operator: Append,
                                value: Array(
                                    [
                                        DoubleQuoted(
                                            Parameter {
                                                prefix: "",
                                                name: "VULKAN_DRIVERS",
                                                suffix: Concat(
                                                    [
                                                        Literal(
                                                            "[",
                                                        ),
                                                        Variable(
                                                            "vendor",
                                                        ),
                                                        Literal(
                                                            "]",
                                                        ),
                                                    ],
                                                ),
                                            },
                                        ),
                                    ],
                                ),
                            },
                        ],
                        name: None,
                        args: [],
                    },
                ),
            ],
        }
        "#);
    }

    #[test]
    fn distinguishes_array_keys_from_globs_and_ordinary_arguments() {
        let ast = CshParser::parse(
            r#"a=(["a*b"]=one [$key]+=two [$(printf ']')]=three [0]= [abc] *.sh)
echo [Intel]=vulkan-intel PACKAGES+=x
local scalar+=value"#,
        )
        .unwrap();
        let CshAstExpression::Command(command) = &ast[ast.commands[0]] else {
            panic!("expected assignment");
        };
        let CshAstWord::Array(elements) = &command.assignments[0].value else {
            panic!("expected array");
        };
        assert!(
            matches!(&elements[0], CshAstWord::KeyedElement { key, .. } if matches!(&**key, CshAstWord::DoubleQuoted(_)))
        );
        assert!(
            matches!(&elements[1], CshAstWord::KeyedElement { key, operator: CshAstAssignmentOperator::Append, .. } if **key == CshAstWord::Variable("key".into()))
        );
        assert!(
            matches!(&elements[2], CshAstWord::KeyedElement { key, .. } if matches!(&**key, CshAstWord::CommandSubstitution { .. }))
        );
        assert!(
            matches!(&elements[3], CshAstWord::KeyedElement { value, .. } if **value == CshAstWord::Literal(String::new()))
        );
        assert!(matches!(
            &elements[4],
            CshAstWord::Glob(CshAstGlob::CharacterClass { .. })
        ));
        let CshAstExpression::Command(echo) = &ast[ast.commands[1]] else {
            panic!("expected echo");
        };
        assert!(
            matches!(&echo.args[0], CshAstWord::Concat(parts) if matches!(&parts[0], CshAstWord::Glob(_)))
        );
        assert_eq!(echo.args[1], CshAstWord::Literal("PACKAGES+=x".into()));
        let CshAstExpression::Command(local) = &ast[ast.commands[2]] else {
            panic!("expected local");
        };
        assert!(
            matches!(&local.args[0], CshAstWord::Assignment(a) if a.operator == CshAstAssignmentOperator::Append)
        );
    }

    #[test]
    fn distinguishes_globstar_from_star_and_quoted_stars() {
        let ast = CshParser::parse(
            r#"TESTENV=prod tape ./test/**/*.test.js '**' "**" \*\* *""* **(foo)"#,
        )
        .unwrap();
        let CshAstExpression::Command(command) = &ast[ast.commands[0]] else {
            panic!("expected command");
        };
        assert_eq!(
            command.args[0],
            CshAstWord::Concat(vec![
                CshAstWord::Literal("./test/".into()),
                CshAstWord::Glob(CshAstGlob::GlobStar),
                CshAstWord::Literal("/".into()),
                CshAstWord::Glob(CshAstGlob::Star),
                CshAstWord::Literal(".test.js".into()),
            ])
        );
        assert_debug_snapshot!(command.args, @r#"
        [
            Concat(
                [
                    Literal(
                        "./test/",
                    ),
                    Glob(
                        GlobStar,
                    ),
                    Literal(
                        "/",
                    ),
                    Glob(
                        Star,
                    ),
                    Literal(
                        ".test.js",
                    ),
                ],
            ),
            SingleQuoted(
                "**",
            ),
            DoubleQuoted(
                Literal(
                    "**",
                ),
            ),
            Concat(
                [
                    Escaped(
                        "*",
                    ),
                    Escaped(
                        "*",
                    ),
                ],
            ),
            Concat(
                [
                    Glob(
                        Star,
                    ),
                    DoubleQuoted(
                        Literal(
                            "",
                        ),
                    ),
                    Glob(
                        Star,
                    ),
                ],
            ),
            Concat(
                [
                    Glob(
                        Star,
                    ),
                    ExtendedGlob {
                        operator: '*',
                        alternatives: [
                            Literal(
                                "foo",
                            ),
                        ],
                    },
                ],
            ),
        ]
        "#);
    }

    #[test]
    fn structures_glob_tokens_and_classes() {
        let ast = CshParser::parse(
            r"echo src/*.? [!a-z0-9] [[:alpha:][:digit:]_] []-] [é-ü] [a\-z] [unfinished",
        )
        .unwrap();
        let CshAstExpression::Command(command) = &ast[ast.commands[0]] else {
            panic!("expected command");
        };
        assert_debug_snapshot!(command.args, @r#"
        [
            Concat(
                [
                    Literal(
                        "src/",
                    ),
                    Glob(
                        Star,
                    ),
                    Literal(
                        ".",
                    ),
                    Glob(
                        QuestionMark,
                    ),
                ],
            ),
            Glob(
                CharacterClass {
                    negated: true,
                    items: [
                        Range {
                            start: 'a',
                            end: 'z',
                        },
                        Range {
                            start: '0',
                            end: '9',
                        },
                    ],
                },
            ),
            Glob(
                CharacterClass {
                    negated: false,
                    items: [
                        NamedClass(
                            "alpha",
                        ),
                        NamedClass(
                            "digit",
                        ),
                        Character(
                            '_',
                        ),
                    ],
                },
            ),
            Glob(
                CharacterClass {
                    negated: false,
                    items: [
                        Character(
                            ']',
                        ),
                        Character(
                            '-',
                        ),
                    ],
                },
            ),
            Glob(
                CharacterClass {
                    negated: false,
                    items: [
                        Range {
                            start: 'é',
                            end: 'ü',
                        },
                    ],
                },
            ),
            Glob(
                CharacterClass {
                    negated: false,
                    items: [
                        Character(
                            'a',
                        ),
                        Character(
                            '-',
                        ),
                        Character(
                            'z',
                        ),
                    ],
                },
            ),
            Concat(
                [
                    Literal(
                        "[",
                    ),
                    Literal(
                        "unfinished",
                    ),
                ],
            ),
        ]
        "#);
    }

    #[test]
    fn structures_nested_extended_glob_alternatives() {
        let ast =
            CshParser::parse(r#"echo @(foo*|+(bar|baz?)|"literal*"|$(printf x)) !(a|) ?(b) *(c)"#)
                .unwrap();
        assert_debug_snapshot!(ast, @r#"
        CshAst {
            commands: [
                Command(
                    CshAstCommand {
                        assignments: [],
                        name: Some(
                            Literal(
                                "echo",
                            ),
                        ),
                        args: [
                            ExtendedGlob {
                                operator: '@',
                                alternatives: [
                                    Concat(
                                        [
                                            Literal(
                                                "foo",
                                            ),
                                            Glob(
                                                Star,
                                            ),
                                        ],
                                    ),
                                    ExtendedGlob {
                                        operator: '+',
                                        alternatives: [
                                            Literal(
                                                "bar",
                                            ),
                                            Concat(
                                                [
                                                    Literal(
                                                        "baz",
                                                    ),
                                                    Glob(
                                                        QuestionMark,
                                                    ),
                                                ],
                                            ),
                                        ],
                                    },
                                    DoubleQuoted(
                                        Literal(
                                            "literal*",
                                        ),
                                    ),
                                    CommandSubstitution {
                                        commands: [
                                            Command(
                                                CshAstCommand {
                                                    assignments: [],
                                                    name: Some(
                                                        Literal(
                                                            "printf",
                                                        ),
                                                    ),
                                                    args: [
                                                        Literal(
                                                            "x",
                                                        ),
                                                    ],
                                                },
                                            ),
                                        ],
                                        backticks: false,
                                    },
                                ],
                            },
                            ExtendedGlob {
                                operator: '!',
                                alternatives: [
                                    Literal(
                                        "a",
                                    ),
                                    Literal(
                                        "",
                                    ),
                                ],
                            },
                            ExtendedGlob {
                                operator: '?',
                                alternatives: [
                                    Literal(
                                        "b",
                                    ),
                                ],
                            },
                            ExtendedGlob {
                                operator: '*',
                                alternatives: [
                                    Literal(
                                        "c",
                                    ),
                                ],
                            },
                        ],
                    },
                ),
            ],
        }
        "#);
        let nested = format!("echo {}x{}", "@(".repeat(200), ")".repeat(200));
        assert!(CshParser::parse(&nested).is_err());
    }

    #[test]
    fn distinguishes_globs_from_quoted_text_and_regex() {
        let ast = CshParser::parse(
            r#"echo '*.?' "[a-z]*" \* \? \[ "$ROOT"/*.sh
[[ $x =~ ^[a-z]*$ && $y == *-t2? && $z == "*" ]]"#,
        )
        .unwrap();
        let CshAstExpression::Command(command) = &ast[ast.commands[0]] else {
            panic!("expected command");
        };
        assert_eq!(
            &command.args[..5],
            &[
                CshAstWord::SingleQuoted("*.?".into()),
                CshAstWord::DoubleQuoted(Box::new(CshAstWord::Literal("[a-z]*".into()))),
                CshAstWord::Escaped("*".into()),
                CshAstWord::Escaped("?".into()),
                CshAstWord::Escaped("[".into()),
            ]
        );
        let CshAstExpression::Test(CshAstWord::Concat(parts)) = &ast[ast.commands[1]] else {
            panic!("expected test");
        };
        assert_eq!(
            parts
                .iter()
                .filter(|w| matches!(w, CshAstWord::Glob(_)))
                .count(),
            2
        );
        assert!(parts.contains(&CshAstWord::Literal(" =~ ^[a-z]*".into())));
        assert!(
            parts.contains(&CshAstWord::DoubleQuoted(Box::new(CshAstWord::Literal(
                "*".into()
            ))))
        );
    }

    #[test]
    fn structures_parameter_expansions_in_tests() {
        let ast = CshParser::parse("[[ ${running_kernel,,} == *-t2* ]]").unwrap();
        assert_debug_snapshot!(ast, @r#"
        CshAst {
            commands: [
                Test(
                    Concat(
                        [
                            Literal(
                                " ",
                            ),
                            Parameter {
                                prefix: "",
                                name: "running_kernel",
                                suffix: Literal(
                                    ",,",
                                ),
                            },
                            Literal(
                                " == ",
                            ),
                            Glob(
                                Star,
                            ),
                            Literal(
                                "-t2",
                            ),
                            Glob(
                                Star,
                            ),
                            Literal(
                                " ",
                            ),
                        ],
                    ),
                ),
            ],
        }
        "#);
    }

    #[test]
    fn retains_substitutions_and_quote_boundaries_in_tests() {
        let ast = CshParser::parse(
            r#"[[ "$(printf '%s' "$ROOT")" == '${literal}' && $name =~ ^(a|b)$ ]]"#,
        )
        .unwrap();
        let CshAstExpression::Test(CshAstWord::Concat(parts)) = &ast[ast.commands[0]] else {
            panic!("expected test");
        };
        let CshAstWord::DoubleQuoted(word) = &parts[1] else {
            panic!("expected quoted operand");
        };
        let CshAstWord::CommandSubstitution { commands, .. } = &**word else {
            panic!("expected substitution");
        };
        let CshAstExpression::Command(command) = &ast[commands[0]] else {
            panic!("expected printf");
        };
        assert_eq!(
            command.args[1],
            CshAstWord::DoubleQuoted(Box::new(CshAstWord::Variable("ROOT".into())))
        );
        assert!(parts.contains(&CshAstWord::SingleQuoted("${literal}".into())));
        assert!(parts.contains(&CshAstWord::Variable("name".into())));
        for source in [
            "[[ $(echo |) == x ]]",
            "[[ ${kernel,,} == x",
            "[[ \"$ROOT == x ]]",
        ] {
            assert!(CshParser::parse(source).is_err(), "accepted {source:?}");
        }
    }

    #[test]
    fn structures_migration_assignment() {
        let ast = {
            let source = String::from(
                r#"migration=$(grep -l "dell-xps13-sidecar-amps" "$ROOT"/migrations/*.sh | head -1)"#,
            );
            CshParser::parse(&source).unwrap()
        };
        let CshAstExpression::Command(assignment) = &ast[ast.commands[0]] else {
            panic!("expected assignment");
        };
        assert!(assignment.name.is_none());
        let CshAstWord::CommandSubstitution { commands, .. } = &assignment.assignments[0].value
        else {
            panic!("expected substitution");
        };
        let CshAstExpression::Binary {
            left,
            operator,
            right,
        } = &ast[commands[0]]
        else {
            panic!("expected pipeline");
        };
        assert_eq!(*operator, CshAstOperator::Pipe);
        let CshAstExpression::Command(grep) = &ast[*left] else {
            panic!("expected grep");
        };
        assert_eq!(
            grep.args[2],
            CshAstWord::Concat(vec![
                CshAstWord::DoubleQuoted(Box::new(CshAstWord::Variable("ROOT".into()))),
                CshAstWord::Literal("/migrations/".into()),
                CshAstWord::Glob(CshAstGlob::Star),
                CshAstWord::Literal(".sh".into()),
            ])
        );
        let CshAstExpression::Command(head) = &ast[*right] else {
            panic!("expected head");
        };
        assert_eq!(head.name, Some(CshAstWord::Literal("head".into())));
        assert_debug_snapshot!(ast, @r#"
        CshAst {
            commands: [
                Command(
                    CshAstCommand {
                        assignments: [
                            CshAstAssignment {
                                name: "migration",
                                operator: Set,
                                value: CommandSubstitution {
                                    commands: [
                                        Binary {
                                            left: Command(
                                                CshAstCommand {
                                                    assignments: [],
                                                    name: Some(
                                                        Literal(
                                                            "grep",
                                                        ),
                                                    ),
                                                    args: [
                                                        Literal(
                                                            "-l",
                                                        ),
                                                        DoubleQuoted(
                                                            Literal(
                                                                "dell-xps13-sidecar-amps",
                                                            ),
                                                        ),
                                                        Concat(
                                                            [
                                                                DoubleQuoted(
                                                                    Variable(
                                                                        "ROOT",
                                                                    ),
                                                                ),
                                                                Literal(
                                                                    "/migrations/",
                                                                ),
                                                                Glob(
                                                                    Star,
                                                                ),
                                                                Literal(
                                                                    ".sh",
                                                                ),
                                                            ],
                                                        ),
                                                    ],
                                                },
                                            ),
                                            operator: Pipe,
                                            right: Command(
                                                CshAstCommand {
                                                    assignments: [],
                                                    name: Some(
                                                        Literal(
                                                            "head",
                                                        ),
                                                    ),
                                                    args: [
                                                        Literal(
                                                            "-1",
                                                        ),
                                                    ],
                                                },
                                            ),
                                        },
                                    ],
                                    backticks: false,
                                },
                            },
                        ],
                        name: None,
                        args: [],
                    },
                ),
            ],
        }
        "#);
    }

    #[test]
    fn structures_quoted_words_and_expansion_operands() {
        let ast = CshParser::parse(r#""$RUN" '$ROOT' "$ROOT" \$ROOT "" $1 $10 $@ $? $$ ${x:-$(printf '%s' "$HOME")} $((1 + $N)) $'a\'b\n' $"hello $USER" >"$OUT"/log"#).unwrap();
        assert_debug_snapshot!(ast, @r#"
        CshAst {
            commands: [
                Redirected {
                    expression: Command(
                        CshAstCommand {
                            assignments: [],
                            name: Some(
                                DoubleQuoted(
                                    Variable(
                                        "RUN",
                                    ),
                                ),
                            ),
                            args: [
                                SingleQuoted(
                                    "$ROOT",
                                ),
                                DoubleQuoted(
                                    Variable(
                                        "ROOT",
                                    ),
                                ),
                                Concat(
                                    [
                                        Escaped(
                                            "$",
                                        ),
                                        Literal(
                                            "ROOT",
                                        ),
                                    ],
                                ),
                                DoubleQuoted(
                                    Literal(
                                        "",
                                    ),
                                ),
                                Variable(
                                    "1",
                                ),
                                Concat(
                                    [
                                        Variable(
                                            "1",
                                        ),
                                        Literal(
                                            "0",
                                        ),
                                    ],
                                ),
                                Variable(
                                    "@",
                                ),
                                Variable(
                                    "?",
                                ),
                                Variable(
                                    "$",
                                ),
                                Parameter {
                                    prefix: "",
                                    name: "x",
                                    suffix: Concat(
                                        [
                                            Literal(
                                                ":-",
                                            ),
                                            CommandSubstitution {
                                                commands: [
                                                    Command(
                                                        CshAstCommand {
                                                            assignments: [],
                                                            name: Some(
                                                                Literal(
                                                                    "printf",
                                                                ),
                                                            ),
                                                            args: [
                                                                SingleQuoted(
                                                                    "%s",
                                                                ),
                                                                DoubleQuoted(
                                                                    Variable(
                                                                        "HOME",
                                                                    ),
                                                                ),
                                                            ],
                                                        },
                                                    ),
                                                ],
                                                backticks: false,
                                            },
                                        ],
                                    ),
                                },
                                ArithmeticExpansion(
                                    Concat(
                                        [
                                            Literal(
                                                "1 + ",
                                            ),
                                            Variable(
                                                "N",
                                            ),
                                        ],
                                    ),
                                ),
                                AnsiCQuoted(
                                    "a\\'b\\n",
                                ),
                                LocaleQuoted(
                                    Concat(
                                        [
                                            Literal(
                                                "hello ",
                                            ),
                                            Variable(
                                                "USER",
                                            ),
                                        ],
                                    ),
                                ),
                            ],
                        },
                    ),
                    redirects: [
                        CshAstRedirect {
                            descriptor: None,
                            operator: ">",
                            target: Concat(
                                [
                                    DoubleQuoted(
                                        Variable(
                                            "OUT",
                                        ),
                                    ),
                                    Literal(
                                        "/log",
                                    ),
                                ],
                            ),
                        },
                    ],
                },
            ],
        }
        "#);
    }

    #[test]
    fn bounds_nested_word_expansions() {
        for source in [
            format!("echo {}x{}", "${x:-".repeat(200), "}".repeat(200)),
            format!("echo {}pwd{}", "$(echo ".repeat(200), ")".repeat(200)),
            format!("a={}x{}", "(a=".repeat(200), ")".repeat(200)),
        ] {
            assert!(CshParser::parse(&source).is_err());
        }
    }

    #[test]
    fn keeps_assignment_word_boundaries() {
        let ast = CshParser::parse("X=#tag Y= Z='' echo \"\" '' # comment").unwrap();
        let CshAstExpression::Command(command) = &ast[ast.commands[0]] else {
            panic!("expected command");
        };
        assert_eq!(
            command
                .assignments
                .iter()
                .map(|a| &a.value)
                .collect::<Vec<_>>(),
            vec![
                &CshAstWord::Literal("#tag".into()),
                &CshAstWord::Literal(String::new()),
                &CshAstWord::SingleQuoted(String::new()),
            ]
        );
        assert_eq!(
            command.args,
            vec![
                CshAstWord::DoubleQuoted(Box::new(CshAstWord::Literal(String::new()))),
                CshAstWord::SingleQuoted(String::new()),
            ]
        );
    }

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
                                    name: Some(
                                        Literal(
                                            "a",
                                        ),
                                    ),
                                    args: [],
                                },
                            ),
                            operator: Or,
                            right: Command(
                                CshAstCommand {
                                    assignments: [],
                                    name: Some(
                                        Literal(
                                            "b",
                                        ),
                                    ),
                                    args: [],
                                },
                            ),
                        },
                        operator: And,
                        right: Binary {
                            left: Command(
                                CshAstCommand {
                                    assignments: [],
                                    name: Some(
                                        Literal(
                                            "c",
                                        ),
                                    ),
                                    args: [],
                                },
                            ),
                            operator: Pipe,
                            right: Command(
                                CshAstCommand {
                                    assignments: [],
                                    name: Some(
                                        Literal(
                                            "d",
                                        ),
                                    ),
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
                            name: Some(
                                Literal(
                                    "e",
                                ),
                            ),
                            args: [],
                        },
                    ),
                    operator: PipeWithStderr,
                    right: Command(
                        CshAstCommand {
                            assignments: [],
                            name: Some(
                                Literal(
                                    "f",
                                ),
                            ),
                            args: [],
                        },
                    ),
                },
            ],
        }
        "#);
    }

    #[test]
    fn keeps_variable_descriptors_out_of_command_arguments() {
        let ast = CshParser::parse(
            "exec {fd_1}>out {input}<&0 {fd_1}>&-; {input}<&-; echo {fd} '{fd}' \\{fd\\} {fd} >out; { echo ok; } {log}>>out",
        ).unwrap();
        let CshAstExpression::Redirected {
            expression,
            redirects,
        } = &ast[ast.commands[0]]
        else {
            panic!("expected redirected exec");
        };
        let CshAstExpression::Command(command) = &ast[*expression] else {
            panic!("expected exec command");
        };
        assert!(command.args.is_empty());
        assert_eq!(
            redirects
                .iter()
                .map(|r| (r.descriptor.as_deref(), r.operator.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (Some("{fd_1}"), ">"),
                (Some("{input}"), "<&"),
                (Some("{fd_1}"), ">&")
            ]
        );
        let CshAstExpression::Redirected {
            expression,
            redirects,
        } = &ast[ast.commands[1]]
        else {
            panic!("expected redirect-only command");
        };
        assert_eq!(redirects[0].descriptor.as_deref(), Some("{input}"));
        assert!(matches!(&ast[*expression], CshAstExpression::Command(c) if c.name.is_none()));
        let CshAstExpression::Redirected {
            expression,
            redirects,
        } = &ast[ast.commands[2]]
        else {
            panic!("expected redirected echo");
        };
        assert_eq!(redirects[0].descriptor, None);
        assert!(matches!(&ast[*expression], CshAstExpression::Command(c) if c.args.len() == 4));
        let CshAstExpression::Redirected {
            expression,
            redirects,
        } = &ast[ast.commands[3]]
        else {
            panic!("expected redirected group");
        };
        assert!(matches!(&ast[*expression], CshAstExpression::Group(_)));
        assert_eq!(redirects[0].descriptor.as_deref(), Some("{log}"));
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
                                    operator: Set,
                                    value: Literal(
                                        "test",
                                    ),
                                },
                            ],
                            name: Some(
                                Literal(
                                    "cmd",
                                ),
                            ),
                            args: [
                                Literal(
                                    "arg",
                                ),
                            ],
                        },
                    ),
                    redirects: [
                        CshAstRedirect {
                            descriptor: None,
                            operator: ">",
                            target: Literal(
                                "out",
                            ),
                        },
                        CshAstRedirect {
                            descriptor: Some(
                                "2",
                            ),
                            operator: ">&",
                            target: Literal(
                                "1",
                            ),
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
                                        name: Some(
                                            Literal(
                                                "echo",
                                            ),
                                        ),
                                        args: [
                                            Literal(
                                                "hi",
                                            ),
                                        ],
                                    },
                                ),
                                Redirected {
                                    expression: Command(
                                        CshAstCommand {
                                            assignments: [],
                                            name: Some(
                                                Literal(
                                                    "cat",
                                                ),
                                            ),
                                            args: [],
                                        },
                                    ),
                                    redirects: [
                                        CshAstRedirect {
                                            descriptor: None,
                                            operator: "<",
                                            target: Literal(
                                                "out",
                                            ),
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
                            target: Literal(
                                "log",
                            ),
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
                        name: Some(
                            Literal(
                                "echo",
                            ),
                        ),
                        args: [
                            Concat(
                                [
                                    Literal(
                                        "pre",
                                    ),
                                    DoubleQuoted(
                                        Literal(
                                            "fix",
                                        ),
                                    ),
                                ],
                            ),
                            Concat(
                                [
                                    Literal(
                                        "a",
                                    ),
                                    Escaped(
                                        " ",
                                    ),
                                    Literal(
                                        "b",
                                    ),
                                ],
                            ),
                            Concat(
                                [
                                    Escaped(
                                        "\"",
                                    ),
                                    Literal(
                                        "x",
                                    ),
                                    Escaped(
                                        "\"",
                                    ),
                                ],
                            ),
                            SingleQuoted(
                                "\\literal",
                            ),
                            DoubleQuoted(
                                Concat(
                                    [
                                        Escaped(
                                            "\\q",
                                        ),
                                        Escaped(
                                            "\"",
                                        ),
                                    ],
                                ),
                            ),
                            Parameter {
                                prefix: "",
                                name: "v",
                                suffix: Concat(
                                    [
                                        Literal(
                                            ":-",
                                        ),
                                        DoubleQuoted(
                                            Literal(
                                                "a)b",
                                            ),
                                        ),
                                    ],
                                ),
                            },
                            DoubleQuoted(
                                CommandSubstitution {
                                    commands: [
                                        Command(
                                            CshAstCommand {
                                                assignments: [],
                                                name: Some(
                                                    Literal(
                                                        "echo",
                                                    ),
                                                ),
                                                args: [
                                                    DoubleQuoted(
                                                        Literal(
                                                            "nested",
                                                        ),
                                                    ),
                                                ],
                                            },
                                        ),
                                    ],
                                    backticks: false,
                                },
                            ),
                            ArithmeticExpansion(
                                Literal(
                                    "1 + (2)",
                                ),
                            ),
                            CommandSubstitution {
                                commands: [
                                    Command(
                                        CshAstCommand {
                                            assignments: [],
                                            name: Some(
                                                Literal(
                                                    "pwd",
                                                ),
                                            ),
                                            args: [],
                                        },
                                    ),
                                ],
                                backticks: true,
                            },
                            ProcessSubstitution {
                                operator: "<",
                                commands: [
                                    Command(
                                        CshAstCommand {
                                            assignments: [],
                                            name: Some(
                                                Literal(
                                                    "cat",
                                                ),
                                            ),
                                            args: [
                                                Literal(
                                                    "x",
                                                ),
                                            ],
                                        },
                                    ),
                                ],
                            },
                            Concat(
                                [
                                    Literal(
                                        "./{cjs,esm}/",
                                    ),
                                    ExtendedGlob {
                                        operator: '!',
                                        alternatives: [
                                            Literal(
                                                "package.json",
                                            ),
                                        ],
                                    },
                                ],
                            ),
                            Literal(
                                "foo#bar",
                            ),
                            DoubleQuoted(
                                Parameter {
                                    prefix: "",
                                    name: "x",
                                    suffix: Literal(
                                        "",
                                    ),
                                },
                            ),
                        ],
                    },
                ),
                Command(
                    CshAstCommand {
                        assignments: [],
                        name: Some(
                            Literal(
                                "echo",
                            ),
                        ),
                        args: [
                            Concat(
                                [
                                    Literal(
                                        "a",
                                    ),
                                    Escaped(
                                        "",
                                    ),
                                    Literal(
                                        "b",
                                    ),
                                ],
                            ),
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
                            Concat(
                                [
                                    Glob(
                                        Star,
                                    ),
                                    Literal(
                                        ".js",
                                    ),
                                ],
                            ),
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
                                                        name: Some(
                                                            Literal(
                                                                "test",
                                                            ),
                                                        ),
                                                        args: [
                                                            Literal(
                                                                "-f",
                                                            ),
                                                            DoubleQuoted(
                                                                Variable(
                                                                    "f",
                                                                ),
                                                            ),
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
                                                        name: Some(
                                                            Literal(
                                                                "echo",
                                                            ),
                                                        ),
                                                        args: [
                                                            DoubleQuoted(
                                                                Variable(
                                                                    "f",
                                                                ),
                                                            ),
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
                                                        name: Some(
                                                            Literal(
                                                                "false",
                                                            ),
                                                        ),
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
                                                        name: Some(
                                                            Literal(
                                                                "break",
                                                            ),
                                                        ),
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
                                                    name: Some(
                                                        Literal(
                                                            "continue",
                                                        ),
                                                    ),
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
                                    name: Some(
                                        Literal(
                                            "ready",
                                        ),
                                    ),
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
                                    name: Some(
                                        Literal(
                                            "sleep",
                                        ),
                                    ),
                                    args: [
                                        Literal(
                                            "1",
                                        ),
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
                                        name: Some(
                                            Literal(
                                                "ready",
                                            ),
                                        ),
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
                                    name: Some(
                                        Literal(
                                            "sleep",
                                        ),
                                    ),
                                    args: [
                                        Literal(
                                            "1",
                                        ),
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
                    word: DoubleQuoted(
                        Variable(
                            "choice",
                        ),
                    ),
                    arms: [
                        CshAstCaseArm {
                            patterns: [
                                Literal(
                                    "y",
                                ),
                                Literal(
                                    "Y",
                                ),
                            ],
                            body: CshAst {
                                commands: [
                                    Group(
                                        CshAst {
                                            commands: [
                                                Command(
                                                    CshAstCommand {
                                                        assignments: [],
                                                        name: Some(
                                                            Literal(
                                                                "build",
                                                            ),
                                                        ),
                                                        args: [],
                                                    },
                                                ),
                                                Command(
                                                    CshAstCommand {
                                                        assignments: [],
                                                        name: Some(
                                                            Literal(
                                                                "test",
                                                            ),
                                                        ),
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
                                Glob(
                                    Star,
                                ),
                            ],
                            body: CshAst {
                                commands: [
                                    Command(
                                        CshAstCommand {
                                            assignments: [],
                                            name: Some(
                                                Literal(
                                                    "echo",
                                                ),
                                            ),
                                            args: [
                                                Literal(
                                                    "abort",
                                                ),
                                            ],
                                        },
                                    ),
                                    Command(
                                        CshAstCommand {
                                            assignments: [],
                                            name: Some(
                                                Literal(
                                                    "exit",
                                                ),
                                            ),
                                            args: [
                                                Literal(
                                                    "1",
                                                ),
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
            assert_eq!(command.args, [CshAstWord::Literal(expected.to_string())]);
            id = *left;
        }
        let CshAstExpression::Command(command) = &ast[id] else {
            panic!("expected the first command");
        };
        assert_eq!(command.args, [CshAstWord::Literal("0".into())]);
    }

    #[test]
    fn validates_and_retains_substitution_nodes() {
        let ast = CshParser::parse("echo $(printf '%s' $(pwd)) <(cat file) && done_cmd").unwrap();
        assert_eq!(ast.nodes.len(), 6);
        assert_eq!(ast.commands, [CshAstNodeId(5)]);
        let CshAstExpression::Command(command) = &ast[CshAstNodeId(3)] else {
            panic!("expected echo");
        };
        assert!(
            matches!(&command.args[0], CshAstWord::CommandSubstitution { commands, .. } if commands == &[CshAstNodeId(1)])
        );
        assert!(
            matches!(&command.args[1], CshAstWord::ProcessSubstitution { commands, .. } if commands == &[CshAstNodeId(2)])
        );
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
        let nested = format!("{}echo{}", "( ".repeat(128), " )".repeat(128));
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
        assert_eq!(redirects[0].target, CshAstWord::Literal("out".into()));
        let CshAstExpression::Command(command) = &ast[*expression] else {
            panic!("expected command");
        };
        assert_eq!(command.name, Some(CshAstWord::Literal("ifconfig".into())));
        assert_eq!(
            &command.args[..3],
            &[
                CshAstWord::SingleQuoted("if".into()),
                CshAstWord::Literal("a#b".into()),
                CshAstWord::Literal("2file".into())
            ]
        );
        assert!(matches!(
            &command.args[3],
            CshAstWord::ProcessSubstitution { .. }
        ));
        assert_eq!(
            command.args[4],
            CshAstWord::Concat(vec![
                CshAstWord::Literal("pre".into()),
                CshAstWord::DoubleQuoted(Box::new(CshAstWord::Literal("héllo".into()))),
                CshAstWord::SingleQuoted("🌍".into())
            ])
        );
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
                        name: Some(
                            Literal(
                                "echo",
                            ),
                        ),
                        args: [
                            DoubleQuoted(
                                Literal(
                                    "Hello, cruel world!",
                                ),
                            ),
                        ],
                    },
                ),
            ],
        }
        "#);
    }

    #[test]
    fn parses_functions_and_arithmetic_loops() {
        let ast = CshParser::parse(
            "function bump { (( count += 1 )); }\nfor ((i=0; i<2; i++)); do bump; done",
        )
        .unwrap();
        assert_debug_snapshot!(ast, @r#"
        CshAst {
            commands: [
                Function {
                    name: "bump",
                    body: Group(
                        CshAst {
                            commands: [
                                Arithmetic(
                                    " count += 1 ",
                                ),
                            ],
                        },
                    ),
                },
                ArithmeticFor {
                    clauses: "i=0; i<2; i++",
                    body: CshAst {
                        commands: [
                            Command(
                                CshAstCommand {
                                    assignments: [],
                                    name: Some(
                                        Literal(
                                            "bump",
                                        ),
                                    ),
                                    args: [],
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
    fn parses_ordered_heredocs_without_parsing_their_contents() {
        let ast = CshParser::parse(
            "cat <<'EOF' 3<<-END\n$(not shell |) ' \"\nEOF\n\tvalue\n\tEND\necho done",
        )
        .unwrap();
        assert_debug_snapshot!(ast, @r#"
        CshAst {
            commands: [
                Redirected {
                    expression: Command(
                        CshAstCommand {
                            assignments: [],
                            name: Some(
                                Literal(
                                    "cat",
                                ),
                            ),
                            args: [],
                        },
                    ),
                    redirects: [
                        CshAstRedirect {
                            descriptor: None,
                            operator: "<<",
                            target: Literal(
                                "EOF",
                            ),
                            here_document: 0,
                        },
                        CshAstRedirect {
                            descriptor: Some(
                                "3",
                            ),
                            operator: "<<-",
                            target: Literal(
                                "END",
                            ),
                            here_document: 1,
                        },
                    ],
                },
                Command(
                    CshAstCommand {
                        assignments: [],
                        name: Some(
                            Literal(
                                "echo",
                            ),
                        ),
                        args: [
                            Literal(
                                "done",
                            ),
                        ],
                    },
                ),
            ],
            here_documents: [
                CshAstHereDocument {
                    delimiter: "EOF",
                    quoted: true,
                    strip_tabs: false,
                    body: "$(not shell |) ' \"\n",
                },
                CshAstHereDocument {
                    delimiter: "END",
                    quoted: false,
                    strip_tabs: true,
                    body: "value\n",
                },
            ],
        }
        "#);
    }

    #[test]
    fn scopes_heredocs_inside_substitutions() {
        let source = "cat <<OUT \"$(cat <<'IN'\ninner\nIN\n)\"\nouter\nOUT\n";
        let ast = CshParser::parse(source).unwrap();
        assert_eq!(ast.here_documents.len(), 2);
        assert_eq!(ast.here_documents[0].delimiter, "OUT");
        assert_eq!(ast.here_documents[0].body, "outer\n");
        assert_eq!(ast.here_documents[1].body, "inner\n");
        assert_eq!(ast.nodes.len(), 4);
    }

    #[test]
    fn preserves_array_and_test_expression_syntax() {
        let ast = CshParser::parse("items=('one two' $(pwd)\n# comment\nthree)\n[[ ${items[0]} =~ ^(one|two)$ && -n \"$HOME\" ]]").unwrap();
        let CshAstExpression::Command(command) = &ast[ast.commands[0]] else {
            panic!("expected assignment");
        };
        let CshAstWord::Array(elements) = &command.assignments[0].value else {
            panic!("expected array");
        };
        assert_eq!(elements.len(), 3);
        assert_eq!(elements[0], CshAstWord::SingleQuoted("one two".into()));
        assert!(matches!(
            &elements[1],
            CshAstWord::CommandSubstitution { .. }
        ));
        assert_eq!(elements[2], CshAstWord::Literal("three".into()));
        let CshAstExpression::Test(CshAstWord::Concat(parts)) = &ast[ast.commands[1]] else {
            panic!("expected structured test");
        };
        assert!(matches!(&parts[1], CshAstWord::Parameter { name, .. } if name == "items"));
        assert!(
            parts.contains(&CshAstWord::DoubleQuoted(Box::new(CshAstWord::Variable(
                "HOME".into()
            ))))
        );
        for source in [
            "f()",
            "function",
            "function f",
            "a=(one",
            "[[ -f x",
            "cat <<EOF",
            "cat <<EOF\nnot closed\n",
            "cat <<'EOF'\n EOF\n",
        ] {
            // `function` alone is reserved by Bash, but is not a complete definition.
            assert!(CshParser::parse(source).is_err(), "accepted {source:?}");
        }
    }

    #[test]
    fn parses_omarchy_corpus() {
        let vendor = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor");
        let pattern = vendor.join("@omacom/omarchy/**/*.sh");
        let mut paths = glob::glob(pattern.to_str().unwrap())
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        paths.sort();
        assert!(
            !paths.is_empty(),
            "No shell scripts found at {}",
            pattern.display()
        );
        let mut parsed = Vec::new();
        let mut failures = Vec::new();
        let mut names = std::collections::BTreeSet::new();
        for path in &paths {
            let relative = path.strip_prefix(&vendor).unwrap().to_str().unwrap();
            let name = format!(
                "omarchy_corpus__{}",
                relative.trim_start_matches('@').replace(['/', '.'], "__")
            );
            assert!(
                names.insert(name.clone()),
                "Snapshot name collision: {relative}"
            );
            let source = std::fs::read_to_string(path).unwrap();
            match CshParser::parse(&source) {
                Ok(ast) => parsed.push((name, relative, ast)),
                Err(error) => {
                    let mut report = Vec::new();
                    CshErrorReport::new(&source, relative, &error.errors, false)
                        .write(&mut report)
                        .unwrap();
                    failures.push(String::from_utf8(report).unwrap());
                }
            }
        }
        assert!(
            failures.is_empty(),
            "{} of {} corpus files failed:\n{}",
            failures.len(),
            paths.len(),
            failures.join("\n")
        );
        for (name, relative, ast) in parsed {
            insta::with_settings!({ description => relative }, {
                assert_debug_snapshot!(name, ast);
            });
        }
    }

    #[test]
    fn parses_npm_scripts_corpus() {
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
        assert_debug_snapshot!("npm_scripts_corpus_asts", asts);
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
        assert_snapshot!("npm_scripts_corpus_rejections", reports);
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
                        name: Some(
                            Literal(
                                "echo",
                            ),
                        ),
                        args: [
                            SingleQuoted(
                                "",
                            ),
                            Concat(
                                [
                                    Literal(
                                        "pre",
                                    ),
                                    DoubleQuoted(
                                        Literal(
                                            "fix",
                                        ),
                                    ),
                                ],
                            ),
                            SingleQuoted(
                                "a b",
                            ),
                        ],
                    },
                ),
                Command(
                    CshAstCommand {
                        assignments: [],
                        name: Some(
                            Literal(
                                "pwd",
                            ),
                        ),
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
                        name: Some(
                            SingleQuoted(
                                "if",
                            ),
                        ),
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
                        name: Some(
                            Literal(
                                "echo",
                            ),
                        ),
                        args: [
                            SingleQuoted(
                                "héllo\n🌍",
                            ),
                        ],
                    },
                ),
            ],
        }
        "#);
    }
}
