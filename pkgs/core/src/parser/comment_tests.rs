use super::*;

fn check(source: &str, expected: &[&str]) -> CshAst {
    let mut ast = CshParser::parse_with_options(
        source,
        CshParserOptions {
            keep_comments: true,
        },
    )
    .unwrap();
    assert_eq!(
        ast.comments
            .iter()
            .map(|comment| comment.text.as_str())
            .collect::<Vec<_>>(),
        expected,
    );
    let mut end = 0;
    for comment in &ast.comments {
        assert!(
            comment.span.start >= end,
            "comments must be in source order"
        );
        assert_eq!(&source[comment.span.clone()], format!("#{}", comment.text));
        end = comment.span.end;
    }
    let comments = std::mem::take(&mut ast.comments);
    assert_eq!(ast, CshParser::parse(source).unwrap());
    ast.comments = comments;
    ast
}

#[test]
fn preserves_comment_text_and_byte_spans() {
    check("", &[]);
    check("#", &[""]);
    check(
        "#!/bin/sh\n  # leading é 🐚  \necho café # inline\n#\n# final",
        &["!/bin/sh", " leading é 🐚  ", " inline", "", " final"],
    );
    check(
        "# backslash \\\necho ok\n# crlf\r\n",
        &[" backslash \\", " crlf\r"],
    );
}

#[test]
fn preserves_nested_and_dangling_comments() {
    check(
        "# file\nf() {\n# body\nif true; then # then\necho $(\n# substitution\necho hi # inner\n) | # pipe\ncat\n# before else\nelse\necho no\nfi\n# before close\n}\n# eof",
        &[
            " file",
            " body",
            " then",
            " substitution",
            " inner",
            " pipe",
            " before else",
            " before close",
            " eof",
        ],
    );
    check(
        "a=(one # array\ntwo)\necho `\n# backtick\necho hi\n`\ncat <(\n# process\necho hi\n)",
        &[" array", " backtick", " process"],
    );
}

#[test]
fn hashes_in_words_and_heredocs_are_not_comments() {
    check(
        r##"echo foo#bar '#single' "#double" \#escaped $x#suffix ${x#prefix} ${x:-#default}
cat <<'EOF' # quoted doc
# literal body
EOF
cat <<EOF # expanded doc
# still literal
$(echo hi # nested comment
)
EOF
"##,
        &[" quoted doc", " expanded doc", " nested comment"],
    );
}

#[test]
fn speculative_arithmetic_does_not_duplicate_comments() {
    check("((echo $(echo hi # once\n)) || other)", &[" once"]);
}

#[test]
fn retained_comments_are_owned_and_visible_in_debug_output() {
    let ast = {
        let source = String::from("# owned");
        check(&source, &[" owned"])
    };
    assert_eq!(ast.comments[0].text, " owned");
    assert!(format!("{ast:?}").contains("comments"));
}
