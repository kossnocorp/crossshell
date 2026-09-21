#!/usr/bin/env bash
set -euo pipefail

bench_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
if (( $# > 1 )); then
  echo "usage: $0 [shell-file]" >&2
  exit 1
fi
input="${1:-$bench_dir/fixtures/mvdan.sh}"
# Resolve relative input paths before changing into the Go module directory.
input="$(cd -- "$(dirname -- "$input")" && pwd)/$(basename -- "$input")"
if [[ ! -f "$input" || ! -r "$input" ]]; then
  echo "cannot read input: $input" >&2
  exit 1
fi

cd -- "$bench_dir"
mkdir -p dist
cargo build --release --locked --manifest-path Cargo.toml --bin parser-rust --target-dir dist/rust
go build -mod=readonly -o dist/parser-go .

printf 'Input: %s\n\n' "$input"
printf 'Fresh parser per parse; comments retained; one worker; 1 s measurement target.\n'
printf 'Compare Go Mean ns/parse with Rust mean (check the displayed time units).\n\n'
./dist/parser-go "$input"
./dist/rust/release/parser-rust "$input"
