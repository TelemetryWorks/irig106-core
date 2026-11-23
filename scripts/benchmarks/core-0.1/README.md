# Published core 0.1 baseline (not a 0.2 implementation)

Isolated, unpublished consumer of registry **irig106-core =0.1.0**, types 0.1.0.
Its own workspace/lockfile prevents measuring an edited checkout by accident.
No production dependency, root workspace member, or new third-party tool.
The counting feature's unsafe GlobalAlloc shim delegates to System; unsafe is
confined to this measurement binary, not the library. Separate builds keep that
wrapper out of the final timing path.

From this directory, with Rust 1.100.0 and at least 1.2 GiB free fixture space:

```sh
cargo build --release --locked -j 2
# Use a disposable working directory: generation overwrites recording.ch10.
bench_binary="$PWD/target/release/core-02-readonly-baseline"
bench_inputs=$(mktemp -d)
"$bench_binary" generate "$bench_inputs/recording.ch10"
# Expected SHA-256: 4132c798dc92b802c0af607b74ce471fa59d506d10fcdba1f9f7a55fa4edded0
(cd "$bench_inputs" && timeout 120 "$bench_binary" bench recording.ch10 > timing.csv)
cargo build --release --locked --features count-alloc -j 2
(cd "$bench_inputs" && timeout 120 "$bench_binary" bench recording.ch10 > allocation.csv)
```

`timeout` is GNU coreutils; on other platforms use an equivalent outer deadline.
Generate the recovery control with `python3 make_recovery.py INPUT_DIRECTORY`,
then bench its recovery.ch10 separately. Keep fixture generation outside any RSS
measurement. Remove only your disposable inputs when finished. No CI dispatch or
publication command is part of these instructions. The benchmark intentionally
asserts this fixture's packet count; it is not a general recording validator.

Recorded 2026-11-23: Rust 1.100.0, Linux aarch64 (CPU 0x41/0xd4f), 4-CPU cgroup
quota/8 GiB, one measuring thread, release/codegen-units=1, default 8 KiB input.
A warm raw pass is excluded; three timed repetitions/mode, median below. Pages
remain cached; no CPU pinning/cold-disk/parallel-decoder claim. Final raw samples
are in baseline.csv. `system`/`recovery-system` rows use the ordinary allocator;
`counted` rows count calls in a separate feature build. In the program's output,
use instrumented=false rows for timing and instrumented=true in the **counting
build only** for allocations; otherwise zero counters mean instrumentation is off.

Fixture: 568,748,032 bytes / 2,000,001 complete hand-packed frames, 64 interleaved
data channels plus channel0 setup; wrapping sequences and nonzero payload bytes.
Main sizes 64/128/256/512 bytes; 366 x 64 KiB, 122 x 512 KiB, one 1 MiB setup.
Seven raw types: 01/09/11/19/21/29/68. DATA is synthetic, not conformant decoded
payload; no secondary headers or data checksums. Primary sums are generated
independently of core. Capture cap is explicitly 128 MiB to admit setup DATA.

| Mode | Mpacket/s | Decimal MB/s | Scan allocation calls |
| --- | ---: | ---: | ---: |
| Raw 8 KiB read/drain | — | 7101.36 | 0 |
| Zero capture | 11.617 | 3303.59 | 1 |
| Four-byte capture | 9.843 | 2798.99 | 2,000,002 |
| Full DATA capture | 9.421 | 2679.05 | 2,000,002 |
| Inventory/sequence, zero capture | 7.017 | 1995.48 | 21 |
| Full DATA + dependent payload-hash CPU proxy | 2.134 | 606.90 | 2,000,002 |

Counts cover reader construction, scan and teardown; file opening, generation,
startup and printing are excluded. No reallocations. Captured DATA adds one
allocation/packet; scan/inventory allocate once/bounded map growth respectively.
Every packet-processing pass checks the accepted count; inventory also checks its
total. Hash work is a CPU proxy, not an IRIG decoder. Timing samples in one run
are close, but earlier runs differed: no portable throughput guarantee.

Recovery control adds 8 MiB mid-file gap and 8 MiB trailing padding, total
585,525,248 bytes: zero-capture 6.502 Mpacket/s / 1903.46 MB/s; inventory
4.789 Mpacket/s / 1402.10 MB/s. Prior separately counted runs retained 1/21
allocations; those preliminary rows are not the final throughput baseline here.
These results support measuring reader, inventory and heavy decode separately.
They do not prove the performance or memory behavior of proposed 0.2 storage.
