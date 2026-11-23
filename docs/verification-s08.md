# Slice 8: isolated memory and package verification

Base main 6fcf5f25cf434d3bff74599b3143b78cf0db9d29 (approved PR9).
Tests-first 171af7e reports missing check_rss import and missing packaged
rss_probe example. These are executed harness/layout failures, not parser
runtime failures. During implementation, small fixture tests also caught excess
bytes in the generator/header oracle literals; corrected before CI measurement.

## Isolated Linux CI probe: L3-IO-002 / L3-INV-006

The CI-only job uses Ubuntu24.04, release Rust, two build workers and GNU time
`%M` (KiB). It generates input and compiles first, then runs only the prebuilt
rss_probe executable under `/usr/bin/time`. Cargo, Python, the generator and
Rust's test harness are outside the measured child. The driver spawns no child
processes; it uses core plus its sole ecosystem dependency types, never time,
TMATS or a CLI ecosystem stack. No local large-file/RSS run is claimed.

The file has 4 segments × 1048576 complete 28-byte packets, three 4096-byte FF
recovery gaps and 16777216 zero padding bytes: 134230016 bytes total. Primary
headers are independently packed with scalar checksum arithmetic; DATA is 01020304.
One channel/type cycles all sequence bytes. The generator writes bounded chunks
(at most 448 KiB per packet chunk), checks free disk and removes its temporary
file. The process timeout is 180 seconds; the entire CI job has a 10-minute bound.

The driver has a 4096-byte input buffer, 4-byte DATA capture and a one-channel,
one-row Inspection budget. It never retains packets or observations. It checks
captured DATA and emits exact independent completion evidence:

```text
packets=4194304 data_bytes=16777216 packet_bytes=117440512
recoveries=3 recovered=12288 tail=16777216 tail_scanned=16777192
unverified=3 status=Incomplete
```

The runner requires all those values and a successful child exit before accepting
RSS. The inclusive ceiling is 32768 KiB (32 MiB); zero/missing/invalid metrics
and early termination fail. Peak RSS includes the Rust runtime/allocator and
core driver, but excludes the generating parent and compilation. This checks a
specific streaming/inventory regression, not arbitrary limits, process OOM,
all allocator/platform behavior, throughput or full-standard conformance.

## One-time negative control, removed

Temporary head 758211d retained every primary [u8;24] in PacketReader's unbounded
Vec, black-boxed to keep allocations observable. It still processed the entire
fixture with the exact expected output. [CI run 45105179473 / RSS job](https://github.com/TelemetryWorks/irig106-core/actions/runs/45105179473/job/137464702242)
measured **100392 KiB**, and failed specifically against **32768 KiB**. Thus the
ceiling catches the demonstrated core-history regression rather than merely
checking a generous bound. The control was then removed; reader.rs is identical
to the approved slice 7 source. There is no retained mutation switch or history
allocation in the final source. Final-head positive RSS and package logs are
linked at their exact SHA in [PR10](https://github.com/TelemetryWorks/irig106-core/pull/10).

## Package and documentation: L3-PKG-007

CI checks an exact allowlist of normalized Cargo metadata, README/LICENSE, seven
source modules, six integration test files and both public examples. The list
is printed separately. cargo package --locked keeps default verification (no
--no-verify), compiling the archived source; no upload/tag/version bump occurs.
Registry-only types/default-feature and license identity checks remain in place.

Local checks use a ~28 KiB fixture and a normal process smoke without GNU time.
Three Python controls check generator bytes/checksum/wrap/gaps/padding, the exact
ceiling boundary and invalid metrics, and early-stop rejection. Rust/API evidence
and command results belong in the final PR. README/API/requirements now describe
implemented traversal, inventory, recovery-qualified continuity and scoped CI
evidence; historical slice ledgers remain historical. Trusted Publishing is
still slice 9, with publication requiring separate owner authorization.
