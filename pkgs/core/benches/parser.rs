//! Run with `cargo bench -p crossshell --bench parser`.
//! For unoptimized measurements add `--profile test`.
use crossshell::CshParser;
use std::{hint::black_box, time::Instant};

fn main() {
    // `cargo test --all-targets` checks compilation without running a benchmark.
    if std::env::args().any(|arg| arg == "--test") {
        return;
    }
    let records: Vec<(String, bool)> = include_str!("../test/smoke/data/top-npm-scripts.jsonl")
        .lines()
        .map(|line| {
            let record: serde_json::Value = serde_json::from_str(line).unwrap();
            (
                record["script"].as_str().unwrap().to_owned(),
                record["rejection"].is_string(),
            )
        })
        .collect();
    let bytes: usize = records.iter().map(|(script, _)| script.len()).sum();
    let mut samples = Vec::new();
    for _ in 0..7 {
        let start = Instant::now();
        for (script, rejected) in &records {
            let result = CshParser::parse(black_box(script));
            assert_eq!(
                result.is_err(),
                *rejected,
                "unexpected result for {script:?}"
            );
            black_box(result).ok();
        }
        samples.push(start.elapsed());
    }
    samples.sort();
    let median = samples[samples.len() / 2];
    println!(
        "{} scripts, {} bytes: median {:?} ({:.1} MiB/s, {:.2} µs/script; 7 runs)",
        records.len(),
        bytes,
        median,
        bytes as f64 / median.as_secs_f64() / (1024.0 * 1024.0),
        median.as_secs_f64() * 1e6 / records.len() as f64
    );
}
