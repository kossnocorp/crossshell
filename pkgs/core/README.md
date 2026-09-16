# Cross Shell core

`CshParser::parse` uses a byte-oriented recursive-descent parser. It returns an
owned `CshAst`; errors borrow the source and carry UTF-8 byte spans for Ariadne
reporting through `CshErrorReport`.

## Arena-backed AST

Expression nodes are allocated in `CshAst::nodes`, a contiguous `Vec` arena.
`CshAstNodeId` values are stable indices, including when the arena grows. Root
commands, binary operands, and nested command lists all refer to this same arena.
IDs belong to their owning AST and must not be used to index another tree.

```rust
use crossshell::{CshAstExpression, CshParser};

let ast = CshParser::parse("echo hello | cat").unwrap();
let root = &ast[ast.commands[0]];
if let CshAstExpression::Binary { left, right, .. } = root {
    let first = &ast[*left];
    let second = &ast[*right];
}
```

Nodes and word strings are owned, so the input can be dropped after parsing.
Operator chains are parsed iteratively and destroying an AST does not recurse
through its expression links. Recursive command-list nesting is limited to 128
levels. Balanced parameter, arithmetic, and extended-glob delimiters are scanned
with an explicit stack.

Debug formatting resolves node IDs to show the syntax tree. This keeps AST
snapshots readable and independent of arena allocation order. Expansion syntax
is retained as text; command and process substitutions are syntax-checked before
their temporary arena nodes are reclaimed.

## Tests and performance

```sh
cargo test --workspace
cargo bench -p crossshell --bench parser --profile test
cargo bench -p crossshell --bench parser
```

The smoke test parses every JSONL corpus entry, checks explicitly annotated
rejections, and compares sorted script-to-AST and script-to-diagnostic snapshots.

The benchmark loads and decodes the corpus before timing. It measures parsing,
result validation, and AST destruction over seven runs and reports the median,
throughput, and time per script. It excludes snapshot formatting and comparison.
Use `--profile test` to measure the same unoptimized parser used by `cargo test`;
the default benchmark profile is optimized.
