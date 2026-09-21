# Parser comparison

Run from the repository root (requires Go and Rust):

```sh
./pkgs/bench/bench.sh
./pkgs/bench/bench.sh /path/to/input.sh
```

Or run `mise run bench` from this directory for the default fixture.
The script builds all three binaries, then runs them sequentially on the same file:
Go mvdan/sh, Rust crossshell, and Rust [Brush](https://brush.sh/).
Build artifacts live in the ignored `dist/` directory.

All binaries read the entire input into memory and validate a parse before
benchmarking. The measured functions return native AST results: Go's
`(*syntax.File, error)`, crossshell's `Result<CshAst<'source>, CshParserError<'source>>`,
and Brush's `Result<brush_parser::ast::Program, brush_parser::ParseError>`.
Go and crossshell retain comments; Brush discards them. File I/O and validation are excluded from measurements;
the shell input is parsed, never executed.

## Matched workload

- Each parse constructs fresh parser state and returns a native AST. Go and Brush also
  construct fresh in-memory readers; none copies the whole source
  as benchmark preparation inside the timed loop. This deliberately replaces
  upstream mvdan's parser/reader reuse to match crossshell's stateless API.
- Crossshell's AST borrows text from the preloaded source, allocating owned strings
  when fragments need joining or normalization. The source stays alive for the
  entire benchmark; copying it into an independently owned AST is not timed.
- All run a sequential parse loop with one worker. Go uses `GOMAXPROCS(1)`;
  Divan uses one thread. Go's garbage collector stays enabled.
- All target at least one second of measured work in one invocation. Go's
  `testing.Benchmark` calibrates a batch; Divan takes samples of 1,000 parses
  until the one-second minimum is reached, excluding external harness time
  from that minimum. Batching amortizes clock reads in both harnesses.
- All consume the returned AST and include cleanup work in the timed loop.
  Rust explicitly drops the AST inside the Divan closure, rather than letting
  Divan defer returned-value destruction outside timing.

## Brush workload

`brush-parser` is pinned to **0.4.0**, using its default **PEG** implementation
and default Bash-compatible options (including extended globbing). Each parse
calls `Parser::new(source.as_bytes(), options).parse_program()`: tokenization
and AST construction are both timed. This reader API bypasses Brush's cached
`tokenize_str` convenience function. Both Rust binaries use the same Divan
allocator, sampling settings, and explicit in-loop AST destruction.

Brush's native program AST owns its text and represents shell words as raw
strings, deferring word/expansion parsing to separate APIs. It also discards
comments and provides no comment-retention option. These results compare the
native parse APIs, not identical AST detail or ownership contracts; the Brush
measurement does not include a separate recursive word/expansion parsing pass.

## Reading the output

Go's **Parses** column and Divan's **iters** column count complete parses.
Divan's **samples** column counts timing samples, each of which can contain
multiple parses. Compare Go's **Mean ns/parse** with Divan's **mean** time,
converting Divan's displayed units as needed (1 µs = 1,000 ns). Divan also
shows fastest, slowest, and median sample timings. Throughput uses decimal
bytes in all three binaries (1 MB = 1,000,000 bytes).

Rust uses Divan's `AllocProfiler` instead of a custom counting allocator.
Its `alloc`, `grow`, `shrink`, and `dealloc` rows describe separate operations
per parse. In particular, `grow`/`shrink` bytes are size differences, not the
full new allocation size. Go reports runtime `TotalAlloc` and `Mallocs` deltas
per parse. These allocation metrics are not interchangeable: Go generally
grows buffers by allocating replacements, while Rust can use `realloc`.
Neither report is simply a measure of peak memory.
Divan also prints `max alloc`, a sample high-water mark divided by the sample
size; with immediate AST destruction and 1,000-parse batches, this is not the
peak memory of one AST. Use the operation rows for allocation comparisons.

The remaining differences are inherent to the runtimes and harnesses: Rust
destroys every AST immediately, whereas Go collects periodically and can leave
some garbage after the timed batch. Divan estimates and subtracts harness and
allocation-profiler overhead; Go reports elapsed batch time. Sampling and
calibration also differ. Large iteration counts reduce timer noise but do not
eliminate machine load, run-order effects, or systematic measurement bias.

`fixtures/mvdan.sh` is the materialized input from mvdan/sh's `BenchmarkParse`
at revision `c6351e95dbeeb2645b68c463ea916eed815bef5e`, with a final newline.
Go's dependency is pinned to that revision. Upstream benchmark copyright
(c) 2016, Daniel Martí; see `mvdan-sh-LICENSE` for the BSD-3-Clause license.
