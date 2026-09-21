//! Port of mvdan/sh's syntax/bench_test.go BenchmarkParse.
//! Source revision: c6351e95dbeeb2645b68c463ea916eed815bef5e.
//! Run with `mise run bench:parser` from pkgs/core.
//!
//! Input construction is excluded. Like Go's default benchmark, increase the
//! iteration count until a measured batch takes at least one second. Report
//! elapsed time, total allocated bytes, and allocation calls per complete parse.
//! Reallocations count as allocations of the new size; frees don't subtract bytes.
//!
//! Both parsers retain comments. Comparison limits: Go reuses a parser and reader,
//! while CshParser::parse_with_options has no reusable state. Rust drops
//! each AST within the measurement; Go uses garbage collection. The counting
//! allocator adds instrumentation overhead, so these aren't identical runtimes.
//!
//! Upstream benchmark copyright (c) 2016, Daniel Martí <mvdan@mvdan.cc>.
//! See mvdan-sh-LICENSE for the upstream BSD-3-Clause license.

use crossshell::{CshParser, CshParserOptions};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    hint::black_box,
    time::{Duration, Instant},
};

struct CountingAllocator;

thread_local! {
    static ALLOCATIONS: Cell<(u64, u64)> = const { Cell::new((0, 0)) };
}

fn record_allocation(size: usize) {
    ALLOCATIONS.with(|counts| {
        let (allocs, bytes) = counts.get();
        counts.set((allocs + 1, bytes + size as u64));
    });
}

// SAFETY: All operations delegate to System with the original pointer/layout.
// Accounting uses allocation-free thread-local cells and does not alter memory.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            record_allocation(layout.size());
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            record_allocation(layout.size());
        }
        ptr
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let ptr = unsafe { System.realloc(ptr, layout, size) };
        if !ptr.is_null() {
            record_allocation(size);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn source() -> String {
    [
        "\n\n\t\t        \n".repeat(10),
        format!("# {}\n", "foo bar ".repeat(10)),
        format!("{}\n", "longlit_".repeat(10)),
        format!("'{}'\n", "foo bar ".repeat(10)),
        format!("\"{}\"\n", "foo bar ".repeat(10)),
        "aa bb cc dd; ".repeat(6),
        "a() { (b); { c; }; }; $(d; `e`)\n".into(),
        "foo=bar; a=b; c=d$foo${bar}e $simple ${complex:-default}\n".into(),
        "if a; then while b; do for c in d e; do f; done; done; fi\n".into(),
        "a | b && c || d | e && g || f\n".into(),
        "foo >a <b <<<c 2>&1 <<EOF\n".into(),
        "somewhat long heredoc line\n".repeat(10),
        "EOF".into(),
    ]
    .concat()
}

fn main() {
    let src = source();
    let options = CshParserOptions {
        keep_comments: true,
    };
    // Validate even when cargo test --all-targets skips the timed benchmark.
    let ast = CshParser::parse_with_options(&src, options)
        .expect("mvdan BenchmarkParse input must parse");
    assert_eq!(ast.comments.len(), 1);
    assert_eq!(ast.comments[0].text, format!(" {}", "foo bar ".repeat(10)));
    drop(ast);
    if std::env::args().any(|arg| arg == "--test") {
        return;
    }

    let target = Duration::from_secs(1);
    let mut iterations = 1_u64;
    loop {
        ALLOCATIONS.with(|counts| counts.set((0, 0)));
        let start = Instant::now();
        for _ in 0..iterations {
            let ast = CshParser::parse_with_options(black_box(src.as_str()), options)
                .expect("mvdan BenchmarkParse input must parse");
            drop(black_box(ast));
        }
        let elapsed = start.elapsed();
        let (allocs, bytes) = ALLOCATIONS.with(Cell::get);
        if elapsed >= target {
            println!("mvdan/sh BenchmarkParse input: {} bytes", src.len());
            println!(
                "BenchmarkParse\t{iterations}\t{:.0} ns/op\t{} B/op\t{} allocs/op",
                elapsed.as_nanos() as f64 / iterations as f64,
                bytes / iterations,
                allocs / iterations,
            );
            break;
        }
        // Predict with 20% headroom, capped at 100x growth as in Go's harness.
        let predicted =
            (iterations as f64 * target.as_secs_f64() / elapsed.as_secs_f64() * 1.2) as u64;
        iterations = predicted.clamp(iterations + 1, iterations * 100);
    }
}
