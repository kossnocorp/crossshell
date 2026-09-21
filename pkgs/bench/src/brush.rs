use brush_parser::{ParseError, Parser, ParserImpl, ParserOptions, ast::Program};
use std::{hint::black_box, sync::OnceLock, time::Duration};

#[global_allocator]
static ALLOCATOR: divan::AllocProfiler = divan::AllocProfiler::system();

static SOURCE: OnceLock<String> = OnceLock::new();

fn parse(source: &str) -> Result<Program, ParseError> {
    // Use the reader API so tokenization is performed afresh on every parse,
    // rather than the cached tokenize_str convenience function.
    Parser::new(
        source.as_bytes(),
        &ParserOptions {
            parser_impl: ParserImpl::Peg,
            ..ParserOptions::default()
        },
    )
    .parse_program()
}

#[divan::bench]
fn parse_ast(bencher: divan::Bencher) {
    let source = SOURCE.get().expect("input is loaded before benchmarking");
    bencher.bench_local(|| {
        let ast = parse(black_box(source.as_str())).expect("validated input must parse");
        // Match crossshell: include AST destruction inside the timed closure.
        drop(black_box(ast));
    });
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let path = args.next().ok_or("usage: parser-brush <shell-file>")?;
    if args.next().is_some() {
        return Err("usage: parser-brush <shell-file>".into());
    }
    let source = std::fs::read_to_string(path)?;
    drop(parse(&source)?);
    let bytes = source.len();
    SOURCE.set(source).expect("input is initialized once");

    println!("Rust brush-parser 0.4.0, PEG (Divan): {bytes} input bytes; comments discarded");
    divan::Divan::default()
        .threads([1])
        .sample_size(1_000)
        .sample_count(1)
        .min_time(Duration::from_secs(1))
        .skip_ext_time(true)
        .bytes_count(bytes)
        .bytes_format(divan::counter::BytesFormat::Decimal)
        .main();
    Ok(())
}

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}
