# irig106-core

**Note:** This crate is currently a placeholder.  

The structural traversal and packet parsing engine for the IRIG 106 ecosystem is under active development. We have secured this name on `crates.io` while we finalize the initial version. 

Please check the [GitHub repository](https://github.com/TelemetryWorks/irig106-core) to see our current progress.

The high-performance engine of the IRIG 106 ecosystem. `irig106-core` handles the heavy lifting of structural traversal, header parsing, and byte-level packet extraction, optimized for throughput and determinism on massive telemetry streams.  

## Project Structure

```shell
irig106-core/
├── Cargo.toml               # Crate metadata, dependencies, and feature flags
├── README.md                # Crate-specific documentation and placeholder notice
├── benches/                 # Performance benchmarks (Criterion)
│   └── parser_throughput.rs # Benchmarks testing MB/s processing speed
├── examples/                # Example binary targets demonstrating usage
│   └── scan_file.rs         # A simple CLI example to traverse a ch10 file
├── fuzz/                    # Fuzz testing targets (cargo-fuzz)
│   └── fuzz_targets/
│       └── parse_header.rs  # Feed random bytes to ensure the parser never panics
├── src/                     # The library source code
│   ├── lib.rs               # Root module: exports the public API facade
│   ├── error.rs             # Core error types (e.g., `InvalidSyncWord`, `ChecksumMismatch`)
│   ├── header.rs            # Structs and parsers for the Chapter 10 Packet Header
│   ├── packet.rs            # Core `Packet` structure (Header + Secondary Header + Payload slice)
│   ├── scanner.rs           # The traversal engine (finding sync words, skipping packets)
│   └── utils.rs             # Internal helpers (e.g., checksum calculation)
└── tests/                   # Integration tests
    ├── data/                # Small, mocked, or sanitized .ch10 binary files for testing
    └── structural_tests.rs  # End-to-end traversal and parsing assertions
```