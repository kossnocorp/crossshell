use super::*;

fn command(ast: &CshAst, index: usize) -> &CshAstCommand {
    let CshAstExpression::Command(command) = &ast[ast.commands[index]] else {
        panic!("expected command")
    };
    command
}

fn parameter(word: &CshAstWord) -> &CshAstParameter {
    match word {
        CshAstWord::Parameter(p) => p,
        CshAstWord::DoubleQuoted(w) => parameter(w),
        _ => panic!("expected parameter: {word:?}"),
    }
}

#[test]
fn arithmetic_precedence_associativity_and_expansions() {
    use CshAstArithmeticBinary as B;
    use CshAstArithmeticKind as K;
    let source = "(( result = -2**2 + 3 * 4, count += $(printf 5) )); for ((i=0; i<3; ++i)); do :; done; echo $((2**3**2))";
    let ast = CshParser::parse(source).unwrap();
    let CshAstExpression::Arithmetic(expr) = &ast[ast.commands[0]] else {
        panic!()
    };
    let K::Binary {
        left,
        operator: B::Comma,
        right,
    } = &expr.kind
    else {
        panic!()
    };
    let K::Binary {
        operator: B::Assign,
        right: sum,
        ..
    } = &left.kind
    else {
        panic!()
    };
    let K::Binary {
        left: power,
        operator: B::Add,
        right: product,
    } = &sum.kind
    else {
        panic!()
    };
    assert!(matches!(
        product.kind,
        K::Binary {
            operator: B::Multiply,
            ..
        }
    ));
    let K::Binary {
        left: negative,
        operator: B::Power,
        ..
    } = &power.kind
    else {
        panic!()
    };
    assert!(matches!(
        negative.kind,
        K::Unary {
            operator: CshAstArithmeticUnary::Minus,
            ..
        }
    ));
    let K::Binary {
        operator: B::AddAssign,
        right,
        ..
    } = &right.kind
    else {
        panic!()
    };
    let K::Expansion(word) = &right.kind else {
        panic!()
    };
    let CshAstWord::CommandSubstitution { commands, .. } = &**word else {
        panic!()
    };
    assert_eq!(&source[ast.spans[commands[0].0].clone()], "printf 5");
    let CshAstExpression::ArithmeticFor(expression) = &ast[ast.commands[1]] else {
        panic!()
    };
    let clauses = &expression.clauses;
    assert!(matches!(
        clauses.condition.as_ref().unwrap().kind,
        K::Binary {
            operator: B::Less,
            ..
        }
    ));
    assert!(matches!(
        clauses.update.as_ref().unwrap().kind,
        K::Unary {
            operator: CshAstArithmeticUnary::PreIncrement,
            ..
        }
    ));
    let CshAstWord::ArithmeticExpansion(expr) = &command(&ast, 2).args[0] else {
        panic!()
    };
    let K::Binary {
        operator: B::Power,
        right,
        ..
    } = &expr.kind
    else {
        panic!()
    };
    assert!(matches!(
        right.kind,
        K::Binary {
            operator: B::Power,
            ..
        }
    ));
}

#[test]
fn arithmetic_radices_ternary_subscripts_and_empty_clauses() {
    use CshAstArithmeticKind as K;
    let ast = CshParser::parse(
        "((a[1+2] = flag ? 16#ff : 010)); for ((;;)); do break; done; echo $((64#_))",
    )
    .unwrap();
    let CshAstExpression::Arithmetic(expr) = &ast[ast.commands[0]] else {
        panic!()
    };
    let K::Binary { left, right, .. } = &expr.kind else {
        panic!()
    };
    assert!(matches!(left.kind, K::Subscript { .. }));
    let K::Conditional {
        then_value,
        else_value,
        ..
    } = &right.kind
    else {
        panic!()
    };
    assert!(matches!(&then_value.kind, K::Number { radix: 16, digits } if digits == "ff"));
    assert!(matches!(&else_value.kind, K::Number { radix: 8, digits } if digits == "010"));
    let CshAstExpression::ArithmeticFor(expression) = &ast[ast.commands[1]] else {
        panic!()
    };
    let clauses = &expression.clauses;
    assert_eq!(
        clauses,
        &CshAstArithmeticFor {
            init: None,
            condition: None,
            update: None
        }
    );
    let CshAstWord::ArithmeticExpansion(expr) = &command(&ast, 2).args[0] else {
        panic!()
    };
    assert!(matches!(&expr.kind, K::Number { radix: 64, digits } if digits == "_"));
}

#[test]
fn arithmetic_substitutions_use_shell_grammar_and_retain_grouping() {
    let source = "(( $(case x in x) printf 1;; esac) + (2 * 3) )); ((echo hi) || echo bye)";
    let ast = CshParser::parse(source).unwrap();
    assert_eq!(ast.spans.len(), ast.nodes.len());
    let CshAstExpression::Arithmetic(expr) = &ast[ast.commands[0]] else {
        panic!()
    };
    let CshAstArithmeticKind::Binary { left, right, .. } = &expr.kind else {
        panic!()
    };
    assert!(matches!(right.kind, CshAstArithmeticKind::Group(_)));
    let CshAstArithmeticKind::Expansion(word) = &left.kind else {
        panic!()
    };
    let CshAstWord::CommandSubstitution { commands, .. } = &**word else {
        panic!()
    };
    assert!(matches!(ast[commands[0]], CshAstExpression::Case(_)));
    assert!(matches!(
        ast[ast.commands[1]],
        CshAstExpression::Subshell(_)
    ));
}

#[test]
fn conditions_preserve_short_circuiting_and_operand_contexts() {
    use CshAstConditionKind as K;
    let ast = CshParser::parse(
        r#"[[ ! -f "$file" || $name == a* && ( $name =~ ^(a|b)[0-9]+$ || $name == "a*" ) ]]"#,
    )
    .unwrap();
    let CshAstExpression::Test(condition) = &ast[ast.commands[0]] else {
        panic!()
    };
    let K::Or(not, and) = &condition.kind else {
        panic!()
    };
    let K::Not(unary) = &not.kind else { panic!() };
    assert!(matches!(
        &unary.kind,
        K::Unary {
            operator: CshAstTestUnary::Regular,
            operand: CshAstWord::DoubleQuoted(_),
            ..
        }
    ));
    let K::And(pattern, group) = &and.kind else {
        panic!()
    };
    assert!(matches!(
        &pattern.kind,
        K::Binary {
            operator: CshAstTestBinary::PatternEqual,
            right: CshAstWord::Concat(_),
            ..
        }
    ));
    let K::Or(regex, literal) = &group.kind else {
        panic!()
    };
    assert!(
        matches!(&regex.kind, K::Binary { operator: CshAstTestBinary::Regex, right: CshAstWord::Literal(s), .. } if s == "^(a|b)[0-9]+$")
    );
    assert!(matches!(
        &literal.kind,
        K::Binary {
            right: CshAstWord::DoubleQuoted(_),
            ..
        }
    ));
}

#[test]
fn parameter_operators_are_typed_and_quote_sensitive() {
    use CshAstParameterOperation as O;
    let ast = CshParser::parse(r#"echo ${x-default} "${x:-'default'}" ${x:=one} ${x:?bad} ${x:+yes} ${x: -2:1} ${x##*/} "${x//a/'b'}" ${x^^[a-z]} ${x@Q} ${#items[@]} ${!items[@]} ${!prefix*} ${!#} ${x:-$(printf value)} ${x:-~} "${x:-~}""#).unwrap();
    let args = &command(&ast, 0).args;
    assert!(matches!(
        &parameter(&args[0]).operation,
        O::Default {
            test_empty: false,
            ..
        }
    ));
    assert!(
        matches!(&parameter(&args[1]).operation, O::Default { test_empty: true, word: CshAstWord::Literal(s), .. } if s == "'default'")
    );
    assert!(matches!(
        &parameter(&args[2]).operation,
        O::Default {
            operator: CshAstDefaultOperator::Assign,
            ..
        }
    ));
    assert!(matches!(
        &parameter(&args[3]).operation,
        O::Default {
            operator: CshAstDefaultOperator::Error,
            ..
        }
    ));
    assert!(matches!(
        &parameter(&args[4]).operation,
        O::Default {
            operator: CshAstDefaultOperator::Alternate,
            ..
        }
    ));
    assert!(matches!(
        &parameter(&args[5]).operation,
        O::Slice {
            length: Some(_),
            ..
        }
    ));
    assert!(matches!(
        &parameter(&args[6]).operation,
        O::Trim {
            suffix: false,
            longest: true,
            pattern: CshAstWord::Concat(_),
            ..
        }
    ));
    assert!(
        matches!(&parameter(&args[7]).operation, O::Replace { anchor: CshAstReplaceAnchor::All, replacement: CshAstWord::SingleQuoted(s), .. } if s == "b")
    );
    assert!(matches!(
        &parameter(&args[8]).operation,
        O::Case {
            upper: true,
            all: true,
            pattern: CshAstWord::Glob(_),
            ..
        }
    ));
    assert_eq!(
        parameter(&args[9]).operation,
        O::Transform(CshAstParameterTransform::Quote)
    );
    assert_eq!(parameter(&args[10]).mode, CshAstParameterMode::Length);
    assert_eq!(
        parameter(&args[10]).subscript,
        Some(CshAstWord::Literal("@".into()))
    );
    assert_eq!(
        parameter(&args[11]).mode,
        CshAstParameterMode::Indices { separate: true }
    );
    assert_eq!(
        parameter(&args[12]).mode,
        CshAstParameterMode::Names { separate: false }
    );
    assert_eq!(parameter(&args[13]).mode, CshAstParameterMode::Indirect);
    assert_eq!(parameter(&args[13]).name, "#");
    let O::Default {
        word: CshAstWord::CommandSubstitution { commands, .. },
        ..
    } = &parameter(&args[14]).operation
    else {
        panic!()
    };
    assert!(
        matches!(&ast[commands[0]], CshAstExpression::Command(c) if c.name == Some(CshAstWord::Literal("printf".into())))
    );
    assert!(matches!(
        &parameter(&args[15]).operation,
        O::Default {
            word: CshAstWord::Tilde(CshAstTilde::Home),
            ..
        }
    ));
    assert!(
        matches!(&parameter(&args[16]).operation, O::Default { word: CshAstWord::Literal(s), .. } if s == "~")
    );
    let nested = CshParser::parse(r#"echo "${x:-${y:-'hi'}}" "${x#'f'}" "${x/foo/~}""#).unwrap();
    let args = &command(&nested, 0).args;
    let O::Default { word, .. } = &parameter(&args[0]).operation else {
        panic!()
    };
    assert!(
        matches!(&parameter(word).operation, O::Default { word: CshAstWord::Literal(s), .. } if s == "'hi'")
    );
    assert!(
        matches!(&parameter(&args[1]).operation, O::Trim { pattern: CshAstWord::SingleQuoted(s), .. } if s == "f")
    );
    assert!(matches!(
        &parameter(&args[2]).operation,
        O::Replace {
            replacement: CshAstWord::Tilde(_),
            ..
        }
    ));
}

#[test]
fn brace_sequences_alternatives_and_tilde_eligibility() {
    let ast = CshParser::parse(r#"PATH=~/bin:~alice/bin echo pre{a,{b,c},$(printf d)}post {01..05..2} {z..a} '{1..3}' \{1..3\} ~ ~/bin ~+ ~- ~2 ~-3 ~alice ~"alice" x~ ~:$x"#).unwrap();
    let c = command(&ast, 0);
    let CshAstWord::Concat(path) = &c.assignments[0].value else {
        panic!()
    };
    assert_eq!(
        path.iter()
            .filter(|w| matches!(w, CshAstWord::Tilde(_)))
            .count(),
        2
    );
    let CshAstWord::Concat(parts) = &c.args[0] else {
        panic!()
    };
    let CshAstWord::BraceAlternatives(alternatives) = &parts[1] else {
        panic!()
    };
    assert_eq!(alternatives.len(), 3);
    assert!(matches!(alternatives[1], CshAstWord::BraceAlternatives(_)));
    assert!(
        matches!(&c.args[1], CshAstWord::BraceSequence(s) if s.start == "01" && s.end == "05" && s.padding == 2 && s.step.as_deref() == Some("2"))
    );
    assert!(matches!(&c.args[2], CshAstWord::BraceSequence(s) if s.alphabetic));
    assert!(matches!(&c.args[3], CshAstWord::SingleQuoted(_)));
    assert!(matches!(&c.args[4], CshAstWord::Concat(_)));
    assert_eq!(c.args[5], CshAstWord::Tilde(CshAstTilde::Home));
    assert_eq!(c.args[7], CshAstWord::Tilde(CshAstTilde::WorkingDirectory));
    assert_eq!(c.args[8], CshAstWord::Tilde(CshAstTilde::PreviousDirectory));
    assert_eq!(
        c.args[10],
        CshAstWord::Tilde(CshAstTilde::DirectoryStack {
            index: "3".into(),
            reverse: true,
            explicit_sign: true,
        })
    );
    assert_eq!(
        c.args[11],
        CshAstWord::Tilde(CshAstTilde::User("alice".into()))
    );
    assert!(!matches!(c.args[12], CshAstWord::Tilde(_)));
    assert_eq!(c.args[13], CshAstWord::Literal("x~".into()));
    let ast =
        CshParser::parse(r#"PATH=x\:~ echo {~,/bin} prefix{~root,x} {,prefix}~ ~:suffix"#).unwrap();
    let c = command(&ast, 0);
    let CshAstWord::Concat(parts) = &c.assignments[0].value else {
        panic!()
    };
    assert!(!parts.iter().any(|w| matches!(w, CshAstWord::Tilde(_))));
    let CshAstWord::BraceAlternatives(alternatives) = &c.args[0] else {
        panic!()
    };
    assert_eq!(alternatives[0], CshAstWord::Tilde(CshAstTilde::Home));
    let CshAstWord::Concat(parts) = &c.args[2] else {
        panic!()
    };
    assert_eq!(parts[1], CshAstWord::Tilde(CshAstTilde::Home));
    let CshAstWord::Concat(parts) = &c.args[3] else {
        panic!()
    };
    assert_eq!(parts[0], CshAstWord::Tilde(CshAstTilde::Home));
}

#[test]
fn ansi_quotes_in_parameter_defaults_are_active_inside_double_quotes() {
    let ast = CshParser::parse(r#"echo "${x:-$'hi\n'}""#).unwrap();
    assert!(matches!(&parameter(&command(&ast, 0).args[0]).operation,
        CshAstParameterOperation::Default { word: CshAstWord::AnsiCQuoted(s), .. } if s == "hi\\n"));
    let ast = CshParser::parse(r#"echo "${x:-\}}" "${x:-\q}""#).unwrap();
    assert!(matches!(&parameter(&command(&ast, 0).args[0]).operation,
        CshAstParameterOperation::Default { word: CshAstWord::Escaped(s), .. } if s == "}"));
    assert!(matches!(&parameter(&command(&ast, 0).args[1]).operation,
        CshAstParameterOperation::Default { word: CshAstWord::Escaped(s), .. } if s == "\\q"));
}

#[test]
fn heredocs_have_a_distinct_expansion_grammar_and_arena_links() {
    let source = "cat <<A <<'B'\n\"$name\" '$name' \\$name \\";
    let source = format!("{source}\nnext $(printf hi) $((1+2))\nA\n$name $(not parsed |)\nB\n");
    let ast = CshParser::parse(&source).unwrap();
    let CshAstWord::Concat(parts) = &ast.here_documents[0].content else {
        panic!()
    };
    assert_eq!(
        parts
            .iter()
            .filter(|p| matches!(p, CshAstWord::Variable(n) if n == "name"))
            .count(),
        2
    );
    assert!(parts.contains(&CshAstWord::Escaped("$".into())));
    assert!(
        !parts
            .iter()
            .any(|p| matches!(p, CshAstWord::DoubleQuoted(_) | CshAstWord::SingleQuoted(_)))
    );
    let commands = parts
        .iter()
        .find_map(|p| {
            if let CshAstWord::CommandSubstitution { commands, .. } = p {
                Some(commands)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(&source[ast.spans[commands[0].0].clone()], "printf hi");
    assert!(
        parts
            .iter()
            .any(|p| matches!(p, CshAstWord::ArithmeticExpansion(_)))
    );
    assert_eq!(
        ast.here_documents[1].content,
        CshAstWord::Literal("$name $(not parsed |)\n".into())
    );
    assert_eq!(ast.spans.len(), ast.nodes.len());
}

#[test]
fn heredoc_continuations_precede_delimiter_matching() {
    let ast = CshParser::parse("cat <<EOF\none\\\nEOF\nEO\\\nF\necho after\n").unwrap();
    assert_eq!(ast.commands.len(), 2);
    assert_eq!(
        ast.here_documents[0].content,
        CshAstWord::Literal("oneEOF\n".into())
    );
    let ast = CshParser::parse("cat <<-EOF\n\t$name\n\tEOF\n").unwrap();
    assert_eq!(
        ast.here_documents[0].content,
        CshAstWord::Concat(vec![
            CshAstWord::Variable("name".into()),
            CshAstWord::Literal("\n".into())
        ])
    );
    let ast = CshParser::parse("cat <<OUT\n$(cat <<IN\n$inner\nIN\n)\nOUT\n").unwrap();
    assert_eq!(ast.here_documents.len(), 2);
    assert!(matches!(
        ast.here_documents[1].content,
        CshAstWord::Concat(_)
    ));
}

#[test]
fn redirects_are_typed_ordered_and_spanned() {
    let source = "echo λ 2>&1 >out {fd}<&- &>>log";
    let ast = CshParser::parse(source).unwrap();
    let CshAstExpression::Redirected(redirected) = &ast[ast.commands[0]] else {
        panic!()
    };
    let redirects = &redirected.redirects;
    assert_eq!(
        redirects.iter().map(|r| r.operator).collect::<Vec<_>>(),
        vec![
            CshAstRedirectOperator::DuplicateOutput,
            CshAstRedirectOperator::Output,
            CshAstRedirectOperator::CloseInput,
            CshAstRedirectOperator::AppendAndError
        ]
    );
    assert_eq!(
        redirects[0].descriptor,
        CshAstDescriptor::Number("2".into())
    );
    assert_eq!(
        redirects[2].descriptor,
        CshAstDescriptor::Variable("fd".into())
    );
    assert_eq!(&source[redirects[2].span.clone()], "{fd}<&-");
    assert_eq!(&source[ast.spans[ast.commands[0].0].clone()], source);
}

#[test]
fn rejects_malformed_executable_sublanguages() {
    for source in [
        "((x +))",
        "((1=2))",
        "((x ? y))",
        "((2#2))",
        "for ((i=0;i<2)); do :; done",
        "[[ x && ]]",
        "[[ x == ]]",
        "[[ ( x ]]",
        "echo ${x@Z}",
        "echo ${x:}",
        "echo ${x[0}",
        "cat <<EOF\n$(echo |)\nEOF\n",
    ] {
        assert!(CshParser::parse(source).is_err(), "accepted {source:?}");
    }
    for source in [
        format!("(({}1))", "!".repeat(200)),
        format!("[[ {}x ]]", "! ".repeat(200)),
        format!("echo {}a{}", "{a,".repeat(200), "}".repeat(200)),
    ] {
        assert!(CshParser::parse(&source).is_err());
    }
}
