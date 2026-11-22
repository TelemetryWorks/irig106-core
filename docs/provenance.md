# Seed provenance and slice 1 evidence

## Frozen source ledger (read-only inspection, 2026-11-22)

| Source | Evidence and intended carry-over |
| --- | --- |
| [time 5899806 input.rs](https://github.com/TelemetryWorks/irig106-time/blob/5899806ae684ed38d8cad8f422cabb3d47c6be0f/apps/irig106-time-cli/src/input.rs) | Identical to input.rs at PR66 reviewed head 208b74b after directory move. Promote reader, limits, header, capture and EOF/recovery events. |
| [PR66](https://github.com/TelemetryWorks/irig106-time/pull/66) / [S16b](https://github.com/TelemetryWorks/irig106-time/blob/5899806ae684ed38d8cad8f422cabb3d47c6be0f/docs/verification-s16b.md) | Merge 5202eb3; tests-first missing-API compilation, focused runtime mutants and reported CI. Carry small-buffer/filler/bit7/cap/checksum/alignment regressions. |
| [PR67](https://github.com/TelemetryWorks/irig106-time/pull/67) / [S16b2a](https://github.com/TelemetryWorks/irig106-time/blob/5899806ae684ed38d8cad8f422cabb3d47c6be0f/docs/verification-s16b2a.md) | Merge c114171; complete-packet prerequisite, all-six-command integration, preserved content findings and tails. CLI-104 remains partial. |
| [L3-CLI-104/107](https://github.com/TelemetryWorks/irig106-time/blob/5899806ae684ed38d8cad8f422cabb3d47c6be0f/docs/L3_Requirements.md) | Promote transport/completion clauses only, not semantic joining, reports, time caches or reset heuristics. |
| [Final RSS correction](https://github.com/TelemetryWorks/irig106-time/blob/5899806ae684ed38d8cad8f422cabb3d47c6be0f/docs/verification-s16b-final-rss.md) | Earlier wait4 measurements were harness-inclusive; later isolated GNU-time results/negative control are historical time evidence. Core must measure its own process in slice 8. |
| [Secondary checksum](https://github.com/TelemetryWorks/irig106-time/blob/5899806ae684ed38d8cad8f422cabb3d47c6be0f/crates/irig106-time/src/secondary.rs) | RDG-008 byte-sum selected, alternative word sum retained; preserve interpretation status, not time conversion code. |
| [ch10-reader 2fdc477](https://github.com/TelemetryWorks/irig106-ch10-reader/blob/2fdc4773ff304169e2942a4e9934e30f32e93dd7/src/lib.rs) | Feature inventory only: counts, byte/type summaries, header checks, sequence gaps, setup presence and recovery. Do not copy mmap/unsafe, old flag meanings, growing vectors or partial-packet counts. |
| [Architecture 76acb7b](https://github.com/TelemetryWorks/irig106-docs/tree/76acb7b4545b3cecc4bd31de634ec740e7639807/src/architecture) | scanner/crates/overview supply structural-vs-semantic boundary guidance. November 10 implementation table is historical, not current ecosystem status. |
| [types 7ff62c7](https://github.com/TelemetryWorks/irig106-types/blob/7ff62c78b8b0ea9a97127e32e1e874ca05dad705/src/lib.rs) | Shared Rtc/version/raw time vocabulary; separate setup/header version maps. No whole-packet parser or interpretation dependency. |
| [Publishing standard fcceb02](https://github.com/TelemetryWorks/.github/blob/fcceb0288e76786baeac039307a16302bd468026/docs/TRUSTED-PUBLISHING.md) | Adopt in slice 9 as one PR with unsigned dry run. Existing placeholder excludes token bootstrap. No release permission. |

Historical review/CI/mutation records are not new core verification. Carry test
lineage, independent wire oracles and exact source commits into each promotion
PR; rerun focused tests and affected mutants after adapting APIs. Do not mark
whole requirements verified from partial transport evidence. The GitHub API
returned no formal review objects for PR66/67; review deltas are recorded in
their merged descriptions/verification documents, not inferred approvals.

## Approved policy correction

Owner/coordinator 2026-11-22 supersedes the proposed fatal default 1 MiB limit:
default search is uncapped, memory-bounded and linear to EOF, preserving merged
time decisions #63/#67 for large trailing block padding. Only an optional
caller-set cap yields RecoveryLimit. Every incident retains scanned-byte
evidence, including terminal errors/tails. no_std/alloc and packet-data checksum
contents validation are deferred beyond 0.1; the latter is explicitly NotChecked.

## Slice 1 verification

Base main: 38ac0700d40152d96642cb11176f2e0cdd347c53, pulled before edits.
Tests-first commit: 24e1450. Before the move,
`python3 scripts/test_workspace.py` reports one assertion failure (root package)
and two FileNotFound errors (nested source/license absent). These are executed
layout failures, not parser runtime or compilation evidence.

The final focused script checks Cargo metadata, the unchanged scaffold SHA-256
and package file inclusion/license equality. Its version/dependency/digest
assertions intentionally describe slice 1 and must evolve when slice 2 introduces
the header API/types dependency. CI also builds the archived package source.
Local commands/results and exact-head CI links belong in the PR; unexecuted
MSRV/platform/package-build checks must never be described as passing locally.
No reader code, release workflow, tag or publication is introduced here.
