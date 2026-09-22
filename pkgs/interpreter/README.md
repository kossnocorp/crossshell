# Crossshell interpreter

`crossshell_interpreter` evaluates `crossshell::CshAst` directly. The CLI parses
each script once with `CshParser` and passes the resulting AST to `run_ast`.
There is no other shell parser, runtime, or source-to-shell fallback.

```rust,no_run
fn main() -> anyhow::Result<()> {
    // Dispatch bundled utilities before parsing host CLI arguments.
    if let Some(code) = crossshell_interpreter::dispatch_utility() {
        std::process::exit(code);
    }

    let source = "printf 'b\\na\\n' | sort";
    let ast = crossshell::CshParser::parse(source)
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    let mut interpreter =
        crossshell_interpreter::CshInterpreter::new(std::env::current_exe()?)?;
    let status = interpreter.run_ast(&ast)?;
    assert_eq!(status, 0);
    Ok(())
}
```

## AST and source lifetimes

The evaluator borrows the AST and traverses its node IDs. Function definitions
store borrowed names and body IDs. Pipelines, background jobs, and substitutions
share that same AST; they do not clone it or reconstruct source strings.

Workers use `std::thread::scope`. Rust guarantees they finish before `run_ast`
returns, and therefore before its borrowed AST/source can be released. `wait`
joins background jobs earlier; outstanding jobs are joined at the execution
boundary, including on errors or `exit`. `$!` is an interpreter job ID accepted
by `wait`, not an operating-system PID. Detached jobs and terminal job control
are not implemented.

Variables, exported environment, and the working directory persist between
calls. Functions belong to one AST execution. Pipelines, subshells, substitutions,
and jobs copy shell state for isolation, while borrowing the syntax. The host's
environment, directory, and standard descriptors are never mutated.

`run(source, name)` is a convenience wrapper around `CshParser` and `run_ast`.
`run_script(path, args)` reads and parses a file once and sets `$0` and positional
arguments. Use `set_args` before `run_ast` for a caller-parsed script. Methods
return shell exit statuses; runtime/unsupported-feature errors return `Err`.

## Execution

- uutils commands run as real child processes, through executable links to the
  host in a private directory prepended to the inherited `PATH`. `UTILITIES`
  lists the bundled commands. Native pipes stream concurrently without buffering
  entire pipelines. Substitutions collect stdout; here-documents use anonymous
  temporary files so large inputs cannot block before a command starts.
- Other commands use normal `PATH` lookup and `std::process::Command`, with
  native descriptors, exported variables, working directories, and exit/signal
  statuses. Explicit paths and script changes to `PATH` are respected.
- Shell-state builtins are implemented in the interpreter: `cd`, `export`,
  `unset`, `local`, `set`, `shift`, `read`, `exit`, `return`, `break`, `continue`,
  `wait`, `command` (without options), `builtin`, `:`, `true`, and `false`.
  Commands such as `printf`, `echo`, `pwd`, and `test` use uutils.
- Implemented AST constructs include commands, assignments, functions, groups,
  subshells, pipelines, logical operators, negation, `if`, `for`, arithmetic
  `for`, `while`, `until`, `case`, arithmetic expressions, `[[ ]]`, background
  execution, and redirection. Expansion includes quote-aware splitting and
  globbing, positional parameters, command substitution, brace expansion,
  arithmetic, and common parameter operators.

This is an initial evaluator, not full Bash compatibility. Unsupported features
produce explicit errors when reached: arrays, process substitution, extended
globs, some locale/parameter features, and additional shell builtins such as
`eval`, `source`, `exec`, and `trap`. Numeric descriptors beyond standard I/O are
supported on Unix; Windows supports standard I/O redirection. Windows executable
links use hard links, with a copy fallback across volumes.

```sh
cargo run -p cssh -- run path/to/script.sh
```
