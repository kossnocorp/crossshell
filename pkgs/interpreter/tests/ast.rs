use crossshell::{CshAstExpression, CshAstWord, CshParser};
use crossshell_interpreter::CshInterpreter;

#[test]
fn executes_ast_nodes_even_when_source_disagrees() {
    let mut ast = CshParser::parse("true").unwrap();
    let CshAstExpression::Command(command) = &mut ast.nodes[0] else {
        panic!()
    };
    command.name = Some(CshAstWord::Literal("false".into()));
    ast.source = "this is not even valid shell syntax ((";
    let mut interpreter = CshInterpreter::new(std::env::current_exe().unwrap()).unwrap();
    assert_eq!(interpreter.run_ast(&ast).unwrap(), 1);
}

#[test]
fn functions_and_workers_borrow_ast_instead_of_reparsing_source() {
    let mut ast = CshParser::parse("f() { return 23; }; f & wait $!; f | f").unwrap();
    ast.source = "invalid source";
    ast.spans.clear();
    let mut interpreter = CshInterpreter::new(std::env::current_exe().unwrap()).unwrap();
    assert_eq!(interpreter.run_ast(&ast).unwrap(), 23);
}

#[test]
fn state_persists_but_subshell_assignments_are_isolated() {
    let mut interpreter = CshInterpreter::new(std::env::current_exe().unwrap()).unwrap();
    assert_eq!(
        interpreter.run("n=4; (n=90); ((n == 4))", "first").unwrap(),
        0
    );
    assert_eq!(
        interpreter.run("((n += 2)); ((n == 6))", "second").unwrap(),
        0
    );
}

#[test]
fn unsupported_syntax_is_reported_without_shell_fallback() {
    let mut interpreter = CshInterpreter::new(std::env::current_exe().unwrap()).unwrap();
    let error = interpreter.run("x=(one two)", "arrays").unwrap_err();
    assert!(format!("{error:#}").contains("array assignment"));
}
