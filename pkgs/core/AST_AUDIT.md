# Corpus AST audit — 2026-09-16

## Method and sample

This section records the initial findings. The implementation follow-up below
closes the listed AST representation gaps.

Selected eight files from the 486 sorted paths matching
`vendor/@omacom/omarchy/**/*.sh`, using Python
`random.Random(20260916).sample(paths, 8)`. Parsed each using `cssh ast`, checked
syntax with `bash -n`, and inspected AST output against source constructs.
All eight passed both parsers. Scripts were not executed. This is a qualitative
sample, not a claim of semantic equivalence or an accuracy percentage.

Paths below are relative to `vendor/@omacom/omarchy/`:

| File                                                     | Findings                                                                                                                                                                                                                                                              |
| -------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `test/shell.d/keybindings-menu-test.sh`                  | Function/group bodies, nested substitutions, quote concatenation, arrays and redirects retain their structure. Unquoted `BINDS` here-documents contain command substitutions only as raw text. Arithmetic also hides nested pipelines in text.                        |
| `install/hardware/asus/fix-z13-touchpad.sh`              | `&&` in the condition, body commands, redirect order, and quoted here-document body are represented correctly. The udev text remains literal.                                                                                                                         |
| `test/shell.d/legacy-power-udev-rules-migration-test.sh` | Process substitution, prefix environment assignments, quoted globs, and here-document quoting are retained. `${#legacy_rule_migrations[@]}` and array subscripts still need typed parameter operations; arithmetic commands are raw strings.                          |
| `migrations/1789130779.sh`                               | Assignment, quoted path fragments, if body, stderr redirect and `                                                                                                                                                                                                     |     | true`look correct.`[[ ! -f ... ]]` exposes words but not an actual conditional expression tree. |
| `test/shell.d/windows-vm-compose-test.sh`                | Found incorrect variable-descriptor redirects (fixed). Background subshells, `$!`, nested substitutions and ordered numeric redirects are retained. Arithmetic-for clauses remain raw text; `${!#}` preserves syntax but needs explicit indirect-parameter semantics. |
| `migrations/1780294774.sh`                               | Multiline single-quoted jq source remains one opaque argument. `jq ... >file && mv ...                                                                                                                                                                                |     | rm ...` is correctly left-associated, with the redirect attached only to jq.                    |
| `test/shell.d/tailscale-receive-test.sh`                 | `{1..50}` is just a `Literal`; arithmetic hides a nested `wc` substitution. Expandable here-documents need their own expansion grammar. `$(<file)` is represented as a substitution containing a redirect-only command and needs Bash's file-read semantics.          |
| `test/shell.d/hyprland-session-locked-test.sh`           | Declaration assignments, line continuations, positional parameters and environment prefixes look correct. `${4:-0}` retains `:-0` as a suffix rather than a typed default-value operation.                                                                            |

## Fixed: variable descriptors were ordinary arguments

At `windows-vm-compose-test.sh:335–336`, the source is:

```sh
exec {alias_storage_fd}<&-
exec {alias_shared_fd}<&-
```

Previously these produced an argument such as `Literal("{alias_storage_fd}")`
and a redirect with `descriptor: None`, incorrectly selecting stdin. The parser
now recognizes adjacent, unquoted `{identifier}` redirect prefixes and stores
them in `descriptor`, including the braces. This also handles descriptor
allocation (`exec {fd}>file`), duplication (`{fd}<&0`), redirect-only commands,
and redirects following compound commands. Quoted, escaped or space-separated
braces remain ordinary words.

The full corpus snapshot refresh found this same issue in four files:
`windows-vm-compose-test.sh`, `migrate-notify-test.sh`,
`monitor-modeless-test.sh`, and `shell/plugins/image-picker/list.sh`.
Regression assertions cover variable descriptors, argument boundaries and
compound-command attachment.

## Original follow-up list

1. **Here-document expansion bodies.** Keep quoted bodies literal; parse unquoted
   bodies into text, escaped characters, parameters, arithmetic and arena-linked
   command substitutions. Here-documents have their own quote/backslash rules:
   ordinary quote characters in their text are not shell word delimiters. For
   example, the Taildrop fixture mixes `$downloads` with `\$DECOY`; these must
   expand at different times. Retain delimiter and tab-stripping metadata.
2. **Arithmetic expressions.** Replace raw `Arithmetic(String)` and
   `ArithmeticFor.clauses` with operators, operands, assignments, increments,
   subscripts, and nested expansions. Split for-loop init/test/update explicitly.
   `(( $(wc -l <file) >= expected ))` should expose the `wc` command in the arena,
   just as a normal command substitution does. Define integer width, overflow
   and numeric-literal rules once for all platforms.
3. **Conditional expressions.** Replace `Test(Word)` with unary/binary tests,
   logical operators, negation and grouping. Preserve operand quoting and the
   distinct literal/glob/regex contexts. This is necessary for short-circuiting
   and precedence without reparsing strings at evaluation time.
4. **Parameter operations.** Represent subscripts, length, indirection, defaults,
   substring slicing, replacement and case conversion explicitly. Preserve
   unset-versus-empty behavior and scalar-versus-list expansion (`"$@"`,
   `"${array[@]}"`). Current prefix/name/suffix fields retain much of the syntax,
   but still require another parser before evaluation.
5. **Brace and tilde expansions.** `{1..50}` needs a sequence node, not an ordinary
   literal; add alternatives, nesting and quote-sensitive eligibility too.
   Tilde expansion was not established by this sample and needs a targeted
   follow-up. Expansion order must be explicit and platform-independent.
6. **Typed redirect operations and source spans.** Replace string descriptors and
   operators with enums (default/number/variable and file/duplicate/close/etc.),
   retaining left-to-right application. Add source locations to executable AST
   nodes so evaluation failures can identify the original construct.

## Implementation follow-up

All six representation tasks above are implemented:

| Area                  | Current representation                                                                                                                                                                                                                                 |
| --------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Here-documents        | `content: CshAstWord`, literal for quoted delimiters and structured expansions for unquoted delimiters; nested substitutions share the expression arena. Continuations participate in delimiter matching.                                              |
| Arithmetic            | `CshAstArithmetic`, radix-tagged literals, variables/subscripts, explicit grouping, unary/binary operators, ternaries, assignment/increment operators and structured expansion operands. For loops have separate optional init/condition/update trees. |
| Conditions            | `CshAstCondition`, typed unary/binary tests, negation and precedence-aware logical trees, with quote-aware glob/regex operands.                                                                                                                        |
| Parameters            | `CshAstParameter`, explicit mode/subscript and typed default, slice, trim, replacement, case and transform operations. Default-value and pattern/replacement quoting contexts are distinguished.                                                       |
| Brace/tilde expansion | Lazy alternative/sequence nodes, zero-padding/step metadata, and typed tilde prefixes. Tilde eligibility is applied after brace expansion.                                                                                                             |
| Redirects/spans       | Typed descriptor and operation enums; expression-arena span table plus spans on redirects, here-document bodies, arithmetic and conditions.                                                                                                            |

Regression tests in `src/parser/semantic_tests.rs` assert semantic tree shape,
arena links, source ranges, expansion-context distinctions and malformed-input
rejection. The complete shell and npm corpus snapshots now show these structures.
Arithmetic and conditional nesting/height are bounded as well as command lists
and words. See the README's interpreter contract for expansion order, integer
semantics and runtime-dependent operations.

## Cross-platform interpretation

The audited executable sub-languages now have structured syntax. Runtime data can
still require interpretation: an arithmetic expansion may inject operators, an
array's type determines index versus key semantics, and a redirect operand can
expand into a descriptor close/move. These are evaluation decisions rather than
opaque source fragments in the AST.

Choose a single shell dialect and expansion/options contract across Windows,
macOS and Linux. Use a logical descriptor table that can be backed by OS handles;
variable descriptors must not become Windows command-line arguments. Implement
substitution, splitting, globbing and redirects in the interpreter rather than
delegating their meaning to each host shell. Specifically retain Bash's special
`$(<file)` behavior if that dialect is supported.

These Omarchy scripts also depend on Linux-specific commands and paths. Accurate
ASTs establish language semantics; equivalent availability and behavior of
external programs is a separate runtime concern. Future audits should include
small differential expansion/argument tests and malformed-input tests alongside
the positive corpus snapshots.
