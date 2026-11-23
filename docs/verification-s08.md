# Slice 8: isolated memory and package verification

Base main 6fcf5f25cf434d3bff74599b3143b78cf0db9d29 (approved PR9).
Tests-first 171af7e reports missing check_rss import and missing packaged
rss_probe example. These are executed harness/layout failures, not parser
runtime failures. During implementation, small fixture tests also caught excess
bytes in the generator/header oracle literals; corrected before CI measurement.

## Isolated Linux CI probe: L1-CORE-002 / L2-REC-003 / L3-IO-002 / L3-INV-006

The CI-only job uses Ubuntu24.04, release Rust, two build workers and GNU time
`%M` (KiB). It generates input and compiles first, then runs only the prebuilt
rss_probe executable under `/usr/bin/time`. Cargo, Python, the generator and
Rust's test harness are outside the measured child. The driver spawns no child
processes; it uses core plus its sole ecosystem dependency types, never time,
TMATS or a CLI ecosystem stack. No local large-file/RSS run is claimed.

The expanded file has a complete **67108864-byte (64 MiB) packet**, followed by
4 segments × 1048576 complete 28-byte packets, a **50331648-byte (48 MiB) FF gap**
between the first two segments, two further 4096-byte gaps, and 16777216 zero
padding bytes: **251666432 bytes** total (240 MiB plus 8 KiB). Both the declared
packet size and mid-file recovery exceed the 32 MiB RSS ceiling. L1-CORE-002
requires memory independent of declared packet size; L2-REC-003 requires bounded
recovery memory despite the default uncapped search. Dense small packets alone
cannot detect either of these regressions.

The large packet declares 67108840 DATA bytes, physically written in full, with
only four captured bytes. Its sequence255 precedes the first small sequence0.
This is a structural stress fixture, not recording-wide standard conformance.
Primary headers are independently packed with scalar checksum arithmetic. Every
DATA prefix is 01020304; the remaining large DATA is zero. One channel/type cycles
all sequence bytes. The generator writes bounded chunks (64 KiB fills and
at most 448 KiB per packet chunk), checks free disk and removes its temporary
file. The process timeout is 180 seconds; the entire CI job has a 10-minute bound.

The driver has a 4096-byte input buffer, 4-byte DATA capture and a one-channel,
one-row Inspection budget. It never retains packets or observations. It checks
captured DATA and emits exact independent completion evidence:

```text
packets=4194305 data_bytes=83886056 packet_bytes=184549376
recoveries=3 recovered=50339840 tail=16777216 tail_scanned=16777192
unverified=3 max_packet=67108864 status=Incomplete
```

The runner requires all those values and a successful child exit before accepting
RSS. The inclusive ceiling is 32768 KiB (32 MiB); zero/missing/invalid metrics
and early termination fail. Peak RSS includes the Rust runtime/allocator and
core driver, but excludes the generating parent and compilation. This checks a
specific streaming/inventory regression, not arbitrary limits, process OOM,
all allocator/platform behavior, throughput or full-standard conformance.

## Initial history control, removed (original smaller fixture)

Temporary head 758211d retained every primary [u8;24] in PacketReader's unbounded
Vec, black-boxed to keep allocations observable. It still processed the entire
fixture with the exact expected output. [CI run 45105179473 / RSS job](https://github.com/TelemetryWorks/irig106-core/actions/runs/45105179473/job/137464702242)
measured **100392 KiB**, and failed specifically against **32768 KiB**. Thus the
ceiling catches the demonstrated core-history regression rather than merely
checking a generous bound. The control was then removed; reader.rs is identical
to the approved slice 7 source. There is no retained mutation switch or history
allocation in the final source. That original 128 MiB fixture's passing RSS
measurement covered history retention, but did not expose packet-sized drain
allocations or recovery-byte retention. Review required the two larger axes above.

## Review controls on a throwaway branch

Tests-first b8e01cd records missing extended fixture signatures (TypeError),
not a parser runtime failure. Small golden fixtures check the complete large
packet header/DATA, checksum, offsets, sequence wrap and a separate long gap.
Both one-off mutations run only on work/core-s08-rss-controls in draft PR11,
separately from the PR10 implementation branch. Each must produce the complete
expanded expected output before failing specifically on the RSS ceiling.

- Packet-sized drain (L1-CORE-002): head 1bc1a5b allocates/touches the entire
  uncaptured packet remainder during drain. [Run 45109062896](https://github.com/TelemetryWorks/irig106-core/actions/runs/45109062896/job/137476986902)
  reports **67652 KiB > 32768 KiB**, with all expected packet/recovery/tail values.
- Recovery-byte retention (L2-REC-003): separate head 1a4b88d restores bounded
  drain, then retains each scanned byte in a Vec during recovery.
  [Run 45109182710](https://github.com/TelemetryWorks/irig106-core/actions/runs/45109182710/job/137477378754)
  reports **51256 KiB > 32768 KiB**, with the full expected output unchanged.

The mutations are removed after their runs and PR11 is closed without merging.
There is no mutation switch or library-source change in final PR10. Final-head
positive RSS and package logs are linked at their exact SHA in
[PR10](https://github.com/TelemetryWorks/irig106-core/pull/10).

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
