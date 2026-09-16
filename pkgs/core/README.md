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
through its expression links. Recursive command-list and word-expansion nesting
is limited to 128 levels. Balanced parameter, arithmetic, and extended-glob delimiters are scanned
with an explicit stack.

Debug formatting resolves node IDs to show the syntax tree. This keeps AST
snapshots readable and independent of arena allocation order, including commands
nested inside words.

## Structured words

Command names, arguments, assignment values, redirect targets, `for` words, and
`case` subjects/patterns use `CshAstWord`. A command without a name (for example,
an assignment-only command) has `name: None`.

Words distinguish literals, single/double quotes, escaped characters, variables,
braced parameters, command/process substitutions, arithmetic expansions, glob
patterns, and arrays. `Concat` joins fragments into **one word**, preserving quote
boundaries needed for field splitting and pathname expansion. ANSI-C quotes retain
their escape syntax; locale quotes retain their expandable contents.

For example, `"$ROOT"/migrations/*.sh` becomes:

```text
Concat([
    DoubleQuoted(Variable("ROOT")),
    Pattern("/migrations/*.sh"),
])
```

`CommandSubstitution { commands, backticks }` and
`ProcessSubstitution { operator, commands }` retain command-list IDs in the same
expression arena as the outer command. Follow them with `&ast[id]`, just like
top-level expressions. Nested substitutions and their here-documents remain owned
by the AST after the original source is dropped.

Braced parameters retain their prefix and name separately from their suffix;
the suffix contains subscript/operator text and structured expansion operands.
Arithmetic expansions likewise retain expression text with structured nested
expansions. These are syntax representations, not evaluated values.

Functions have arena-linked bodies. Bash `[[ ... ]]` conditions contain structured
word fragments, exposing variables, parameter expansions, quotes, and substitution
commands. Conditional operators and regex text are literal fragments. For example,
`${running_kernel,,}` exposes the parameter name `running_kernel` and suffix `,,`
(Bash's lowercase-all operation). Arithmetic commands retain their source text
for evaluation. Array literals contain word
elements in assignment values and declaration arguments. Here-document redirects
reference `CshAst::here_documents`, which records the delimiter, quoting and
tab-stripping flags, and body. Bodies are read in declaration order at the next
newline, and command substitutions have independent pending here-documents.

## Tests and performance

```sh
cargo test --workspace
cargo test parses_npm_scripts_corpus
cargo test parses_omarchy_corpus
cargo bench -p crossshell --bench parser --profile test
cargo bench -p crossshell --bench parser
```

`parses_npm_scripts_corpus` parses every npm JSONL corpus entry, checks explicitly
annotated rejections, and compares sorted `npm_scripts_corpus_asts` and
`npm_scripts_corpus_rejections` snapshots.

`parses_omarchy_corpus` globs `vendor/@omacom/omarchy/**/*.sh`, sorts the paths,
and requires every file to parse. Each file gets its own AST snapshot. Names are
derived from the vendor-relative path by removing the leading `@` and replacing
`/` and `.` with `__`, prefixed with `omarchy_corpus__`. For example:

```text
@omacom/omarchy/install/hardware/framework/qmk-hid.sh
→ crossshell__parser__tests__omarchy_corpus__omacom__omarchy__install__hardware__framework__qmk-hid__sh.snap
```

Snapshots live in `src/parser/snapshots`. Refresh them with
`INSTA_UPDATE=always cargo test parses_omarchy_corpus` and review the diff. The
test fails if the glob is empty, a path cannot be read, a snapshot name collides,
or any script fails to parse. Vendored scripts are parsed as data, never executed.

The benchmark loads and decodes the corpus before timing. It measures parsing,
result validation, and AST destruction over seven runs and reports the median,
throughput, and time per script. It excludes snapshot formatting and comparison.
Use `--profile test` to measure the same unoptimized parser used by `cargo test`;
the default benchmark profile is optimized.
