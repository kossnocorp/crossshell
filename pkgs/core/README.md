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
is limited to 128 levels. Arithmetic and conditional trees also have bounded
height, including left-associated operator chains.

Debug formatting resolves node IDs to show the syntax tree. This keeps AST
snapshots readable and independent of arena allocation order, including commands
nested inside words.

Inspect a script without executing it with the hidden CLI command:

```sh
cargo run -p cssh -- ast path/to/script.sh
```

The pretty-printed AST goes to stdout; read/parse errors go to stderr with a
nonzero exit status. See [the corpus AST audit](AST_AUDIT.md) for concrete
accuracy findings and the next structures needed by the interpreter.

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
    Literal("/migrations/"),
    Glob(Star),
    Literal(".sh"),
])
```

Unquoted globs have explicit `Star` (`*`), `GlobStar` (`**`), `QuestionMark` (`?`), and
`CharacterClass` nodes. Classes preserve negation, characters, ranges, POSIX named
classes, collating symbols, and equivalence classes. Extended globs (`@(...)`,
`?(...)`, `*(...)`, `+(...)`, `!(...)`) contain structured alternatives and may
nest. Quoted/escaped wildcard characters remain literal. `[[ ... ]]` uses these
glob nodes as well; the regex operand of `=~` preserves regex syntax as text.

These nodes preserve syntax rather than implying unrestricted string matching.
In pathname matching, `*` and `?` do not match `/`; leading-dot matching follows
the evaluator's rules/options. `**` enables recursive directory matching where
the chosen glob dialect, options, and pattern position allow it (Bash requires
`globstar`). String patterns in `case` and `[[ ... == ... ]]` have different
matching rules. For example, `./test/**/*.test.js` contains `Literal("./test/")`,
`Glob(GlobStar)`, `Literal("/")`, `Glob(Star)`, and `Literal(".test.js")`.

`CommandSubstitution { commands, backticks }` and
`ProcessSubstitution { operator, commands }` retain command-list IDs in the same
expression arena as the outer command. Follow them with `&ast[id]`, just like
top-level expressions. Nested substitutions and their here-documents remain owned
by the AST after the original source is dropped.

Braced parameters have explicit selection modes (value, length, indirection,
names, indices) and typed operations: defaults, assignment/error/alternate values,
arithmetic slices, prefix/suffix removal, pattern replacement, case conversion,
and transformations. Operands retain quotes, escapes and nested substitutions.
For example, `${running_kernel,,}` has a lowercase-all `Case` operation.
Subscript words retain `@` versus `*`; whether an ordinary subscript is an
arithmetic index or an associative key depends on the variable's runtime type.

Arithmetic commands, expansions and for-loop clauses use precedence-aware trees
with radix-tagged integer literals, variables, subscripts, unary/binary operators,
assignments and ternaries. Explicit grouping is retained. Nested shell expansions
are structured words with arena-linked commands. Expansion results may inject
arithmetic tokens; an evaluator must expand those before arithmetic evaluation,
rather than treating every substitution as an atomic number.

Functions have arena-linked bodies. `[[ ... ]]` has its own tree for unary/binary
tests, negation, `&&` and `||`. Operands retain quote boundaries and distinguish
glob patterns from regex text. These operators are separate from command-list
operators and preserve the short-circuit evaluation structure.

Brace alternatives and numeric/alphabetic sequences are explicit nodes. Sequences
retain endpoints, step and zero-padding without eagerly generating words. Tilde
prefixes distinguish home/user directories, current/previous directories and
directory-stack entries. Check their eligibility on each resulting word after
brace expansion (for example, `{,prefix}~`), before parameter expansion.

Array literals contain word elements and explicit `KeyedElement`
nodes for `[key]=value` or `[key]+=value`; keys retain expansions and quotes and
are not parsed as glob classes. Assignments record `Set` (`=`) or `Append` (`+=`).
Declaration builtins (`declare`, `typeset`, `local`, `export`, `readonly`) retain
assignment arguments as `Assignment` nodes in argument order. For example,
`declare -A MAP=([Intel]=driver)` has a `MAP` assignment containing an array with
`KeyedElement { key: Literal("Intel"), operator: Set, value: Literal("driver") }`.
Here-document redirects reference `CshAst::here_documents`, which records the
delimiter, quoting and tab-stripping flags, original/tab-stripped body, and
structured `content`. Quoted bodies are literal. Unquoted bodies have their own
expansion grammar: ordinary quote characters are text, escaped dollars remain
literal, and command/arithmetic substitutions are parsed. Continuations are
handled before delimiter matching. Bodies are read in declaration order at the
next newline, and command substitutions have independent pending here-documents.

Redirects use typed default/number/variable descriptors and operation enums,
including explicit descriptor closing. Dynamic duplication targets still require
classification after expansion. Redirect order is retained. `ast.spans[id.0]`
gives an expression's UTF-8 byte range in the original source; redirects,
here-document bodies, arithmetic and conditional nodes have their own spans.
Debug output resolves arena IDs and omits spans for readability.

## Interpreter contract

The AST targets the Bash-style syntax used by the corpus, with one language
contract across operating systems. Evaluation should perform brace expansion,
tilde expansion, parameter/command/arithmetic expansion, field splitting,
pathname expansion, then quote removal, respecting each word's context.
Here-documents and `[[ ... ]]` do not use ordinary argument splitting/globbing.
Logical operators, ternaries and parameter defaults must remain lazy.

Use signed 64-bit, two's-complement arithmetic on every platform; arithmetic
overflow wraps, division truncates toward zero, and division by zero is an error.
Literal radices are parsed independently of host integer types. Shell options
(including globstar) and locale must be explicit runtime state, not host-shell
defaults. Descriptors should use a logical table backed by OS handles. Bash's
`$(<file)` remains recognizable as a substitution containing one input-only,
redirect-only command and needs its file-read semantics at evaluation time.

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
