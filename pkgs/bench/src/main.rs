use crossshell::{CshAst, CshParser, CshParserError, CshParserOptions};
use std::{hint::black_box, sync::OnceLock, time::Duration};

#[global_allocator]
static ALLOCATOR: divan::AllocProfiler = divan::AllocProfiler::system();

static SOURCE: OnceLock<String> = OnceLock::new();

fn parse(source: &str) -> Result<CshAst<'_>, CshParserError<'_>> {
    CshParser::parse_with_options(
        source,
        CshParserOptions {
            keep_comments: true,
        },
    )
}

#[divan::bench]
fn parse_ast(bencher: divan::Bencher) {
    let source = SOURCE.get().expect("input is loaded before benchmarking");
    bencher.bench_local(|| {
        let ast = parse(black_box(source.as_str())).expect("validated input must parse");
        // Divan defers destruction of returned values until after timing.
        // Drop explicitly here so AST cleanup remains part of the workload,
        // just as Go's measured loop includes garbage collection work.
        drop(black_box(ast));
    });
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let path = args.next().ok_or("usage: parser-rust <shell-file>")?;
    if args.next().is_some() {
        return Err("usage: parser-rust <shell-file>".into());
    }
    let source = std::fs::read_to_string(path)?;
    drop(parse(&source).map_err(|err| format!("{err:?}"))?);
    let bytes = source.len();
    SOURCE.set(source).expect("input is initialized once");

    println!("Rust crossshell (Divan): {bytes} input bytes");
    divan::Divan::default()
        .threads([1])
        // Amortize clock reads over a parse loop, as Go's batch harness does.
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
