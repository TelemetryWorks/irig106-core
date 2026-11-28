# Consumer migration contract: core 0.1 to 0.2

This contract guides adoption of core 0.2.0 and its retained 0.1 capture API; it is not evidence of
changes in downstream repos. Time's resume replaces its duplicate reader after
core publication under separate authorization. Downstream implementation and
publication remain separate work.

## Retained capture API / time's promoted transport

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
| Time/integrity | Primary RTC survives secondary checksum findings. Secondary bytes remain raw; capture packet-data checksums stay Absent/NotChecked; whole/chunk results differ. |

Concrete source edits for seed consumers:

- Replace header field reads with accessors: channel(), packet_length(),
  data_length(), data_type_version(), sequence(), flags() and data_type().
  For example, `packet.header.sequence` becomes `packet.header.sequence()`.
- Replace `Rtc::from_le_bytes(packet.header.rtc)` with `packet.header.rtc()`;
  use `packet.header.rtc_bytes()` only when raw six-byte evidence is needed.
- Add a `_` arm to matches over Event and TailKind; both are non-exhaustive.
- Add `..` to Event::Tail field patterns, for example
  `Event::Tail { offset, bytes, kind, .. }`.

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
continue across recovery. skipped_since_previous sums every stream recovery
since that channel's prior accepted packet, including skips attached to other
channels; recovered_bytes describes only the current packet's incident. An
expected successor after any such skipped input is Unverified, never Contiguous.
First has no prior interval and reports zero skipped_since_previous. Filtering, reordering,
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


## Whole-packet orchestration (0.2)

`cargo run -p irig106-core --example ordered --locked < recording.ch10` uses
WholePacketReader and four reusable PacketBuf owners with two bounded workers.
Observe PacketInfo once on the ordered producer before transferring ownership.
Workers borrow spans; don't convert to captured Packet or feed inventory twice.
Control revisions are illustrative notifications, not a downstream setup/time
implementation. Read the [ordering and optional streaming contract](streaming.md)
before integrating context, cancellation or output merging. CI executes independent
stdin fixtures with an external timeout; downstream integration is still separate.


The 0.2 examples append an `admission=AdmissionCounts { ... }` summary.
Use Inspection::admission_counts() for typed counters rather than parsing Debug.
Capture events have no authorization evidence, so capture-only evaluated_packets
is zero even with nonzero totals. Whole PacketInfo contributes exactly once after
successful inspection. Over-soft and legacy counts overlap the authorization
partition; Unconfirmed(ExactEof) retains its uncertainty. See [API scope](api.md).


## Adopting 0.2 whole delivery and optional modes

- Existing PacketReader/ReaderLimits calls keep the bounded DATA-prefix contract;
  upgrading does not enable whole retention or checksum verification in capture.
  Use WholePacketReader + reusable PacketBuf explicitly when full frames are needed.
  One owner per in-flight packet; borrow contiguous()/spans inside workers and
  return owners without waiting on a merger holding their input credits.
- Construct BlockPool and WholePolicy explicitly. Defaults H=128 MiB, B=256 MiB,
  512 KiB blocks, 64 KiB read-ahead and 4 KiB cutoff do not preallocate H/B.
  A tiny whole packet still occupies a block; queue depth needs its own bound.
  Ordinary returns retain warm capacity, not an automatic eight-block idle cap.
- DataChecksumState is non-exhaustive: retain a `_` arm and handle Verified and
  Mismatch explicitly. Whole/chunk verification is ON, opt-out yields NotChecked;
  Mismatch is delivered evidence, not a default error or payload-validity claim.
  Inspection::data_checksum_counts() reports absent/not_checked/verified/mismatch.
  Example summaries append data_checksums as well as admission; use typed
  accessors rather than parsing Debug, and account for capture's missing coverage.
- Match typed Admission/Pool errors. Q8 HardLimit/BudgetTooSmall/TargetLimit
  retain the checked header at offset+24; logging and continuing repeats the same
  refusal forever. Explicitly drain_next or stop (authorized fixed chunks are an
  option for whole B/target storage refusal). Other errors remain terminal.
  into_inner discards pending header/read-ahead; do not resume from its physical
  cursor as though it were a packet boundary.
- Check contiguous() before coalesce_until, which always copies even one block.
  Budget includes source blocks plus new copy/metadata, and default B refuses a
  maximum-setup copy. Copy credit shortage is immediate WouldBlock; its deadline
  bounds the copy, not a credit wait. Inspect original OR copy, never both.
- drain_next is an explicit reported omission. next_chunks borrows fixed 4 KiB
  provisional spans only for the callback; stage output until returned End and
  invalidate on Abort. Neither report is PacketInfo or accepted inventory input.
  Mark omitted inventory incomplete or define a separate retained scope. Semantic
  setup/time closure cannot be inferred from header notices or drained contents.
- Pool waits need deadlines or non-blocking try-acquire when a caller retains
  earlier input on the same thread. Cancellation is pool-wide/terminal. Source
  reads and callbacks need their own blocking bounds: cooperative deadlines do
  not preempt them. StableSource adapters must supply trustworthy identity/size/
  revision evidence; pipes need explicit oversoft StreamOptIn. UDP reassembly,
  packet loss/order policy and async scheduling remain outside core.

Core's registry requirement remains irig106-types ^0.1 (default features off),
locked to 0.1.0. Core uses/re-exports Rtc; types 0.2 is neither required nor in
this release set. Downstream crates may keep types 0.1, and no cross-repository
version bump or publication was performed here. See [coverage/open limits](coverage-0.2.md)
and [the streaming protocol](streaming.md) before choosing a delivery mode.
