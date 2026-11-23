# Consumer migration contract for core 0.1

This is a future adoption contract, not evidence of changes in downstream repos.
Core remains an unreleased 0.0.1 development manifest. Time is paused until core
is published under separate authorization; its resume replaces the duplicate
reader. No downstream implementation or publication is performed by this slice.

## Time's promoted transport

The source is [time 5899806 input.rs](https://github.com/TelemetryWorks/irig106-time/blob/5899806ae684ed38d8cad8f422cabb3d47c6be0f/apps/irig106-time-cli/src/input.rs),
with review lineage in [provenance](provenance.md). The structural reader moved;
time conversion, TF/calendar policy, semantic joins, command reports and finding
severities stay with time. Core's sole ecosystem dependency is types 0.1 with
default features off; it re-exports the same Rtc vocabulary.

| Seed consumer concern | Core contract |
| --- | --- |
| Reader/limits/next_packet | Import PacketReader, ReaderLimits and Event. Use constructors/builders, open enum patterns and `..` for extensible variants/structs. |
| io::Error handling | Match Error.reason, including NoCompletePacket; use kind()/get_ref()/source() for typed underlying causes. Do not match Display text. |
| Empty or lone truncated input | NoCompletePacket at observed EOF; preserve tail and recovery evidence. The owning CLI maps fatal input to exit 3 across its commands. |
| Complete packet then tail | Keep earlier analysis and report the tail. Established-stream EOF padding remains a finding; do not impose a default recovery cap. |
| Payload capture | body_prefix contains DATA only. Compare captured length with Data Length before a complete-body decoder. Filler/trailer are drained. |
| Time/integrity | Primary RTC survives secondary checksum findings. Secondary bytes remain raw; packet-data checksums are Absent/NotChecked, never claimed Valid. |

When time resumes, it should remove its duplicate transport, retain the seed
regressions and re-run its command-level exit/report and content-vs-structure
checks against the published core. Core's checks do not substitute for those
integration checks. No edition inference from data-type versions is permitted;
the layout baseline remains 106-24R1 Chapter 11.

## Inspection consumers

Feed each reader event exactly once to either observe or observe_with_sequence.
Both update the same inventory and last-sequence state. The latter returns one
SequenceObservation per accepted complete packet, None for a tail, or a typed
InspectionError. Callers may stream diagnostics; retaining every observation
would remove the caller's memory bound. Never pre-filter events if using presence
or continuity facts; explicitly mark omitted input/early stops incomplete.

Choose channel/row budgets explicitly. Exhaustion and counter overflow stop the
accumulator without updating counts, presence or last sequence for the rejected
packet. Preserve the accepted inventory on reader errors and mark it incomplete;
finish only after clean EOF. Recovery keeps unseen facts Unknown even after EOF.
Absence is header evidence, not proof about enabled features or valid content.

Each channel has its own sequence across all types. The initial value is
arbitrary; subsequent expected values wrap modulo 256. Duplicate means repeated
sequence byte, not identical packet content. Discontinuity is an observation,
not a lost-packet estimate or reset/recording-boundary decision. Comparisons
continue across recovery and expose skipped-byte evidence. Filtering, reordering,
resets and multiples of 256 missing packets can defeat naive loss inference.

The archived ch10-reader supplies feature requirements, not a compatible API.
Core covers structural counts/types, primary checks, presence, recovery and
sequence observations; names, report formatting, diagnostic sampling, semantic
validation and a product `irig106 inspect` stay downstream. Do not carry mmap,
unsafe code, obsolete flag meanings or growing diagnostic vectors into core.

## Executable public examples

- `cargo run -p irig106-core --example inspect --locked < recording.ch10` streams
  header facts with zero DATA capture and illustrative 1024-channel/4096-row
  budgets. It prints retained totals even on errors; it does not decode payloads.
- The crate's runnable documentation example demonstrates bounded complete-DATA
  capture using an independent four-byte fixture. A decoder remains responsible
  for payload validity, including when the entire DATA field was captured.
