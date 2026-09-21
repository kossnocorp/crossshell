package main

import (
	"bytes"
	"fmt"
	"os"
	"runtime"
	"testing"

	"mvdan.cc/sh/v3/syntax"
)

// Match Rust's stateless API: each call constructs fresh parser/reader state
// over the already-loaded source and returns an owned AST result.
func parse(source []byte) (*syntax.File, error) {
	parser := syntax.NewParser(syntax.KeepComments(true))
	return parser.Parse(bytes.NewReader(source), "")
}

func run() error {
	if len(os.Args) != 2 {
		return fmt.Errorf("usage: parser-go <shell-file>")
	}
	source, err := os.ReadFile(os.Args[1])
	if err != nil {
		return err
	}
	// Match Divan's single-threaded workload and avoid parallel GC workers.
	runtime.GOMAXPROCS(1)
	if _, err := parse(source); err != nil {
		return err
	}
	result := testing.Benchmark(func(b *testing.B) {
		b.ReportAllocs()
		b.SetBytes(int64(len(source)))
		for i := 0; i < b.N; i++ {
			ast, err := parse(source)
			if err != nil {
				b.Fatal(err)
			}
			runtime.KeepAlive(ast)
		}
	})
	fmt.Printf("Go mvdan/sh (testing.Benchmark): %d input bytes\n", len(source))
	fmt.Printf("%-12s %14s %14s %14s %14s\n", "Parses", "Mean ns/parse", "MB/s", "Bytes/parse", "Allocs/parse")
	fmt.Printf("%-12d %14.0f %14.2f %14d %14d\n\n",
		result.N,
		float64(result.T.Nanoseconds())/float64(result.N),
		float64(len(source))*float64(result.N)/result.T.Seconds()/1e6,
		result.AllocedBytesPerOp(), result.AllocsPerOp())
	return nil
}

func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
