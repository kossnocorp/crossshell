use super::*;
use std::borrow::Cow;

fn assert_source_slice(source: &str, text: &str) {
    assert!(!text.is_empty());
    let start = text.as_ptr() as usize - source.as_ptr() as usize;
    assert_eq!(source.get(start..start + text.len()), Some(text));
}

#[test]
fn words_borrow_source_across_quotes_escapes_and_unicode() {
    let source = String::from(r#"NAME=hello echo plain 'héllo' $'raw\n' \🌍 $NAME "\q""#);
    let ast = CshParser::parse(&source).unwrap();
    let CshAstExpression::Command(command) = &ast[ast.commands[0]] else {
        panic!("expected command");
    };
    assert_source_slice(&source, command.assignments[0].name);
    assert!(matches!(
        &command.assignments[0].value,
        CshAstWord::Literal(Cow::Borrowed("hello"))
    ));
    assert!(matches!(
        &command.name,
        Some(CshAstWord::Literal(Cow::Borrowed("echo")))
    ));
    for word in &command.args {
        let text = match word {
            CshAstWord::Literal(Cow::Borrowed(text))
            | CshAstWord::SingleQuoted(text)
            | CshAstWord::AnsiCQuoted(text)
            | CshAstWord::Escaped(text)
            | CshAstWord::Variable(text) => *text,
            CshAstWord::DoubleQuoted(word) => {
                let CshAstWord::Escaped(text) = &**word else {
                    panic!("expected escape")
                };
                assert_eq!(*text, r"\q");
                text
            }
            _ => panic!("expected borrowed text: {word:?}"),
        };
        assert_source_slice(&source, text);
    }
    // Moving/cloning the arena keeps borrowed fragments valid.
    let cloned = ast.clone();
    drop(ast);
    assert_eq!(cloned.source.as_ptr(), source.as_ptr());
    let CshAstExpression::Command(command) = &cloned[cloned.commands[0]] else {
        panic!("expected command");
    };
    assert_source_slice(&source, command.assignments[0].name);
}

#[test]
fn heredocs_borrow_raw_text_and_own_normalized_text() {
    let source = String::from("cat <<'EOF' <<-E'ND'\nhéllo $name\nEOF\n\tfirst\n\tsecond\nEND\n");
    let ast = CshParser::parse(&source).unwrap();
    let raw = &ast.here_documents[0];
    assert!(matches!(raw.delimiter, Cow::Borrowed("EOF")));
    assert!(matches!(raw.body, Cow::Borrowed("héllo $name\n")));
    assert_source_slice(&source, &raw.delimiter);
    assert_source_slice(&source, &raw.body);
    assert!(matches!(raw.content, CshAstWord::Literal(Cow::Borrowed(_))));

    let normalized = &ast.here_documents[1];
    assert!(matches!(normalized.delimiter, Cow::Owned(_)));
    assert_eq!(normalized.delimiter, "END");
    assert!(matches!(normalized.body, Cow::Owned(_)));
    assert_eq!(normalized.body, "first\nsecond\n");
    assert_eq!(
        normalized.content,
        CshAstWord::Literal("first\nsecond\n".into())
    );
}
