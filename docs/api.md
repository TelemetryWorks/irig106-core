# Core 0.2 API contract

**Primary and retained capture API:** `Header::parse(&[u8])`, `primary_checksum(&[u8; 24])`,
`HeaderError`, `PRIMARY_HEADER_LEN` and re-exported shared `Rtc`. `Header` stores
immutable primary bytes and exposes accessors named below (`channel()`, etc.).
The capture reader returns `Result<_, Error>` from reader construction/iteration.
`Error.reason: ErrorKind` distinguishes I/O, HeaderError, NoCompletePacket,
RecoveryLimit, invalid buffer/capture bounds, allocation and offset overflow.
Errors retain consumed `position`, optional `recovery`, and their original source.
`kind()`/`get_ref()` retain broad seed classifications and HeaderError downcasts.
NoCompletePacket carries optional `Tail` metadata; Event::Tail adds recovery.
Error, Recovery, Tail, ReaderLimits, Packet, SecondaryIntegrity,
and PacketRanges are non-exhaustive; Event::Tail is a non-exhaustive variant.
Construct limits with ReaderLimits::new()/Default and with_buffer_bytes,
with_capture_bytes, with_recovery_bytes builders. Other evidence is read through
fields or patterns containing `..`; consumers cannot construct struct literals.
Event and TailKind are also non-exhaustive. Every field-carrying ErrorKind
variant is non-exhaustive: Io/Header/Allocation now use named `source` fields,
so consumers can match `ErrorKind::Header { source, .. }`. Non-exhaustive tuple
variants would be private to external matching in Rust. ErrorKind Display
includes NoCompletePacket tail kind/bytes/offset; Error Display appends recovery
start/scanned bytes when present. Typed sources and classifications are unchanged.
Capture errors/tails terminate iteration; repeated calls yield None. Whole-mode
H/B/target size refusals alone are positioned and nonterminal (see below). This is framing
completion, not full packet/recording conformance. Behavior follows
[requirements](requirements.md).

```text
ReaderLimits::new()               // defaults 8192 buffer / 65536 capture / None recovery
    .with_buffer_bytes(8192)
    .with_capture_bytes(65536)
    .with_recovery_bytes(None)     // never a default fatal cap
PacketReader<R: std::io::Read>::new(input, limits) -> Result<Self, Error>
PacketReader::next_packet(capture: impl FnOnce(&Header) -> usize)
    -> Result<Option<Event>, Error>
Event::Packet(Packet)
Event::Tail { offset, bytes, kind, recovery, .. } // pattern, not construction
```

The synchronous std-first adapter sits above pure header/checksum functions.
The sole dependency is published irig106-types 0.1, default features off;
no time, TMATS, decode or retired-reader dependency. No supported no_std/alloc
configuration in 0.2; keep parsing and I/O separate for later consumers.

`Header` retains channel, Packet/Data Length, data-type version, sequence, flags,
data type and raw RTC bytes; a shared `Rtc` accessor avoids a competing counter.
Bit 7 selects twelve secondary bytes; bits 3–2 remain raw format evidence. No
recording edition is inferred. Routing codes unknown to core remain observable. `Header::parse` validates only
sync, checksum, minimum/aligned Packet Length and DATA containment. It accepts
a primary-only slice even when secondary/body bytes are declared. It does not
check trailer space itself: PacketRanges::from_header and PacketReader do so
before capture. It does not check packet maximums, nonempty DATA or recording-wide
channel/version constraints; success is basic framing evidence, not conformance.

`Packet` owns offset, header, raw secondary bytes, bounded `body_prefix` (DATA
only), capture-completeness/range accessors and recovery/integrity evidence.
Complete capture means prefix length equals Data Length, not Packet Length.
Uncaptured DATA and trailer are drained, never interpreted. Reader storage is
buffer + 24-byte window + bounded capture + fixed metadata; retaining packets is
the caller's responsibility. No packet history, report rows or diagnostic strings.

## Recovery and termination

Default recovery continues to EOF. An incident starts at an expected packet
offset that fails sync/checksum, including leading junk. Scanned-byte evidence
counts candidate displacement reached from that offset; successful recovery
therefore reports `packet.offset - incident_start`. The fixed 24-byte validation
lookahead is separate from this displacement. Terminal tails also report their
full consumed byte count; recovery evidence is not lost on errors or EOF. For
27 unsynchronized bytes, displacement is 3 and the tail count is 27. A recovered
truncated packet retains its packet offset separately from recovery.start_offset.

`Some(N)` permits a candidate at displacement N, including N=0; a candidate at
N+1 exceeds the cap and returns typed `RecoveryLimit` with start/scanned/limit
evidence. Never silently stop, emit EOF or call unsearched bytes padding on cap
exhaustion. Default None permits arbitrarily large block padding with fixed RAM.
A rejected candidate at the cap returns RecoveryLimit before another read, even
if that read would discover EOF. Only observed EOF can establish a padding tail.
The cap resets per incident. Exact-boundary/zero-cap tests isolate these rules.

Checksum-qualified impossible lengths are fatal framing, preserving the seed
policy. Bad checksums instead trigger recovery, even at an expected offset.
Once a header declares a structurally possible packet, a cut body is terminal:
do not rescan sync-looking bytes swallowed within it; report consumed tail bytes.

Observed EOF before any complete packet yields `NoCompletePacket`, retaining available
tail/recovery evidence. A header alone does not establish the stream. After a
complete packet, partial final headers/truncated packets and EOF padding yield
typed tail events, preserving all prior packets. Errors and tails terminate the
reader; subsequent calls return None. I/O, structural and caller-limit failures
retain their own typed reasons even before the first complete packet; they are
never masked by NoCompletePacket. Real I/O errors retain their source.

Core never exits the process or assigns semantic finding severities. Consumers
map no-complete/fatal input to input 3; recovered sync and established-stream cut
tails are warnings, EOF padding information under the accepted CLI policy.
Short time/TMATS DATA is not malformed framing; downstream content findings must
not become fatal input merely because the content cannot be interpreted.

## Integrity and downstream ownership

Primary checksum is mandatory. Packet.secondary_integrity keeps stored checksum, byte
sum and alternative word sum over the ten non-checksum bytes (time RDG-008).
The selected reading is byte sum; preserve the interpretation's suspect status
and word-only matches. Reserved fields are separate evidence. A bad secondary
checksum does not invalidate primary RTC or erase a structurally complete packet.
SecondaryIntegrity::inspect accepts exactly twelve bytes; the reader rejects
truncated secondary headers before inspection. It reports reserved bytes 8–9,
not format-dependent bits inside the time value. Time consumers determine which
time answers remain usable; RDG-008's interpretation status is unchanged.

In retained capture mode trailer widths/ranges are checked, but contents stay
NotChecked (or Absent when flags select no checksum). Whole/chunk delivery
verifies by default, as described below.
Packet.ranges exposes packet-relative u32 ranges for DATA, filler and checksum.
Widths from flag bits1–0 are 0/1/2/4; overlaps/insufficient space are fatal typed
HeaderError before capture. Variable filler is allowed to preserve per-channel
packet size (§11.2.1.4); filler byte values are not checked. Checksum contents are
Absent or NotChecked in capture; whole/chunk may report Verified/Mismatch. No payload meaning,
secondary time conversion, calendar policy or TF codes move.

The optional inspection accumulator has explicit channel/row caps and checked
counts, emits sequence/presence observations and retains no packet history.
Summary formatting, diagnostic samples, names, filtering, output staging, exit
thresholds and `irig106 inspect` itself remain downstream. An observed TMATS
packet is not valid TMATS; absence after an incomplete scan is inconclusive.

## Bounded inspection

Inspection::new(InspectionLimits::new(max_channels, max_rows)) requires explicit
entry budgets; zero accepts no entries. observe(&Event) counts complete frames
only, finish() declares clean EOF, and mark_incomplete() records reader failure
or early stop. Feed all events without filtering. A tail/limit/overflow makes
incompleteness sticky; finish cannot upgrade it, nor certify empty input.

Global, per-type and per-(channel,type) counts use checked u64 packet/DATA/packet
byte sums. DATA means declared Data Length, not captured bytes. Packet bytes
include headers/trailers but exclude recovered junk and terminal tails. Row
DATA minima/maxima and raw unknown type codes are retained; iterators are sorted.
State is bounded channel/row maps plus 256 fixed type counters. A rejected
observation changes no counters/rows/presence; no eviction or partial updates.
Further observations after closure fail. Allocation failure is not forced or
made fallible for std BTree storage; entry budgets are not process-RSS claims.

PresenceFacts uses Present/Absent/Unknown. Setup is channel0/type01; index and
events are raw types03/02. Matching headers do not validate payload contents or
imply enabled features. Unseen records become Absent only after a complete,
unfiltered scan with no recovered bytes; otherwise Unknown. Recovery is sticky:
any accepted packet with recovered_bytes > 0 prevents absence claims even after
later unrecovered packets and clean EOF (ScanStatus may still be Complete).
Previously observed Present facts survive incomplete or recovered scans. The
caller must mark read errors or omitted input explicitly.
User-facing reports remain downstream work.

## Sequence observations

observe_with_sequence(&Event) shares observe's transactional update, returning
Result<Option<SequenceObservation>, InspectionError>. Feed each event once to
either method. Packets return Some; tails return None. Both keep a last u8 and
recovery checkpoint per channel inside the existing bounded channel map,
independent of raw data type. One checked u64 stream recovery total is shared.
No sequence history, diagnostic list or separate unbounded channel state exists.

SequenceObservation exposes channel, packet offset, previous/observed sequence
and recovered_bytes (this packet's incident). skipped_since_previous counts ALL
stream recoveries since this channel's previous accepted packet, including skips
attached to other channels. It is zero for First because there is no previous
interval. Each accepted packet resets only its channel's checkpoint; cumulative
u64 overflow returns CountOverflow before any state change. expected() returns
the previous value plus one modulo 256, or None on first observation. kind() is
First, Contiguous, Unverified, Duplicate or
Discontinuity; new evidence/enums are non-exhaustive. The initial byte need not
be zero. Every accepted packet becomes the next reference, including duplicates
and discontinuities. An expected successor with nonzero skipped_since_previous
is Unverified, never Contiguous. Recovery does not reset sequence state. Sequence evidence does not
prove loss, identity, content validity or a recording reset; no missing-count
estimate or severity is assigned. See [migration](migration.md) and its examples.

## Verification and release boundary

The 0.2.0 candidate has complete-only whole delivery, retained capture and explicit
coalescing/drain/chunk modes. See [the full coverage audit](coverage-0.2.md) for
L1/L2/L3 mappings, tested scopes and outstanding verification. CI checks the exact
package list, compiled packaged source, runnable inspect/ordered examples, MSRV,
64-bit Linux/macOS/Windows memory and executed Windows i686 functional cases.
Capture keeps its 32 MiB RSS gate; whole small cases use 12 MiB with separate
large/pressure ceilings. Benchmarks are informational generated warm-cache data.
No whole-recording/payload conformance or universal memory/performance claim.
The Trusted Publishing Build repeats Linux qualification before packaging, and
the unsigned publish-path job compares the frozen candidate and pinned 0.0.1
known-good fixture. Passing a PR dry run does not sign, publish or authorize a tag.

## Shared storage (0.2)

BlockPool::with_defaults() uses B = 256 MiB; new(bytes) is fallible. Construction
reserves charged control descriptors, with zero 512 KiB payload blocks.
BlockPool::try_begin(block_count) checks eventual feasibility without reserving
payload and admits one BlockAssembly at a time. Explicit try_begin()/try_next_block()
return WouldBlock on admission/credit contention. begin_until()/next_block_until()
wait to an absolute deadline while a possible returner exists; spurious wakes
recheck predicates. Expiry returns DeadlineExceeded without cancelling the pool
or consuming an assembly slot. BudgetUnavailable is reserved for provable absence
of outstanding returners, not inferred ownership/thread identity. Current public
fixed-block acquisition normally has room, a possible returner, or an upfront
BudgetTooSmall error; the no-returner guard protects the internal wait boundary.
A single-threaded reader retaining earlier blocks uses a deadline or try-acquire.
Readiness is checked before expiry; deadlines bound waiting, not I/O/allocation.
try_next_block() or next_block_until() yields owned PooledBlock;
storage_mut() is an initialized direct-read destination, not a valid packet view.
Workers return blocks by Drop. Ordinary returns retain the warmed high-water set;
trim_after_return(actual_packet_blocks) trims only above eight, and
trim_for_pressure() explicitly trims idle storage to eight. The packet owner must
call the former after returning that packet's actual storage, never from its P.
Pool-wide cancel() is terminal, clears idle storage/credits and wakes both waits.
Cold allocation/zeroing occurs outside the mutex with credits claimed first;
cancellation or allocator failure rolls that claim back. At default B, control
and lease charges leave at most 511 blocks, including idle and worker-held blocks.
After a burst, idle storage can stay up to B (about 511 blocks / 256 MiB resident
at defaults) until a large packet returns and its owner calls trim_after_return(),
or trim_for_pressure() is called. Nothing trims automatically. Retention never
exceeds the caller's accounted budget; choose a lower budget at construction or
call trim for lower residency. Process/allocator overhead is outside this budget.
PoolError
preserves typed budget/allocation/synchronization failures; PoolStats is accounted
storage, not RSS. No new dependencies or unsafe library code. See
[slice-2 evidence](verification-v02-s02.md) for historical scope;
WholePacketReader integrates these primitives with packet admission and I/O.

### Whole-packet delivery (0.2)

`WholePolicy::new()` selects explicit 106-24R1 thresholds, H=128 MiB, no recovery
cap, and legacy/stream oversoft allowances off. H is independent of the B selected
on `BlockPool`; neither setting allocates payload capacity. Unknown edition codes
use the ordinary soft threshold. Type-version bytes never select an edition.

Construct `WholePacketReader::stream(source, policy, pool)` for a pipe/Read source,
or `seek_checked(source, policy, pool)` for a `StableSource`. The latter supplies
comparable `SourceStamp` size/identity/revision evidence and an exclusive cursor
starting at zero; Read+Seek alone cannot prove stability. The adapter contract
excludes concurrent same-size in-place writes that its stamps cannot detect.
No File identity implementation or transport adapter is guessed by core.

Create a caller-owned `PacketBuf::new(pool.clone())` and reuse it with
`next_packet_into(&mut buf, absolute_deadline)`. `WholeEvent::Packet` means complete
metadata is available through `buf.info()` and bytes through `contiguous()` (one
block), `frame_spans()` or `data_spans()`. Spans borrow the buffer; workers move
buffer ownership and return the same descriptors for reuse. Before each call,
the previous frame is returned. No partial frame survives EOF/error as a packet.
A tail or non-size error is followed by None; H/B/target size refusals instead
retain the checked primary for explicit drain/stop (Q8 below).
Wait deadlines bound pool waits, not a blocking source's Read/Seek.
DeadlineExceeded or Cancelled from next_packet_into is terminal: the header and
possibly body bytes have already been consumed, PacketBuf is cleared, and calling
again returns None. Returning credits does not resume the interrupted frame.
into_inner discards recovery read-ahead; its physical cursor may be up to the configured buffer capacity
past position(). A caller restarting must explicitly establish a frame boundary,
not retry from the reported consumed position or assume the source was rewound.
The low-level pool deadline API remains reusable after timeout; this reader's
consumed-input contract is different. Error::kind maps Cancelled and permanent
BudgetUnavailable to Other, not retryable Interrupted/WouldBlock.

WholePolicy defaults to one fallibly reserved **64 KiB read-ahead buffer**, charged
to B, and a **4 KiB body cutoff**. Set with_read_ahead_bytes(bytes) (at least 24,
target/B-representable) and with_small_body_cutoff(bytes); getters expose both.
The size is fixed at construction, never derived from a header. Headers/recovery
share it. Already-buffered bytes (including large prefixes) copy to pooled blocks;
a body strictly below the cutoff may refill through it and copy. A fully buffered
body copies regardless of cutoff. This two-copy path is owner-approved 2026-11-26.

Larger bodies drain pending bytes, then read directly into pooled frame/block
remainders. The decision uses the complete body (P minus 24, including secondary/
trailer) once; short reads and tiny final blocks do not switch back to buffering.
Cutoff zero disables size-triggered refills; buffer 24/cutoff zero supplies the
benchmark's direct-body baseline. Logical offsets/recovery counts reflect bytes
consumed, not prefetch. Probes preserve pending bytes and restore the physical
source cursor. One-block ordinary frames stay contiguous. Interrupted reads retry
in place. The 0.1 buffered-capture path is unchanged.

SeekChecked snapshots source identity/size/revision before any read-ahead. Later
checks compare that baseline, including changes during initial prefetch; body-end
checks still protect delivery. The adapter must exclude undetectable same-size
concurrent writes. Exact EOF and known-short truncation retain their evidence.

PacketBuf descriptors are reserved fallibly from pool geometry at construction,
charged alongside reader control/scratch via workspace leases. Payload is not
reserved from P. Constructors are nonblocking: outstanding assembly or occupied
credits can return WouldBlock; explicitly release/trim and retry. Assembly checks
include committed workspace, and workspace cannot grow during an active build.
This prevents a new reader's metadata from invalidating an admitted build.
Ordinary frames allocate zero after pool/descriptor warm-up in the scoped test;
cold growth and trimming rare large records remain fallible allocation paths.

Admission failures are `ErrorKind::Admission` with typed HardLimit,
Uncorroborated, SourceChanged or TargetLimit causes; storage failures are
`ErrorKind::Pool`. Oversoft frames require NextHeader, Unconfirmed(ExactEof), or
explicit StreamOptIn evidence. Probe errors restore the cursor or fail closed;
lookahead never advances logical offsets/recovery. Known short stable input drains
to EOF with fixed scratch and emits a truncation, not a complete prefix.
Slice-3 evidence predates data-checksum verification; the current whole-mode
checksum contract is below. Capture remains NotChecked.
Explicit coalescing, drain/chunk delivery and platform/performance qualification
are implemented separately below; [slice-3 evidence](verification-v02-s03.md)
records only that earlier slice, not the final coverage boundary.

Exact-EOF's immediate pre-admission stamp recheck can catch a change between
metadata queries; it does not protect the later read interval. The post-body
stamp/consumed-size recheck is the guard over body consumption. Neither query
proves the adapter's promise against undetectable same-size concurrent writes.

Whole mode adapts only the header refill request: after a completed body at least
as large as cutoff, request 24 bytes for the next header refill (within
min(read-ahead capacity, 24 + cutoff)). Cold readers and readers after small bodies allow full capacity.
Pending bytes are consumed first, including at straddling headers. Small-body
and recovery refills still use full capacity. Body-copy selection, allocation,
limits and public policy defaults are unchanged. See
[adaptive-refill evidence](verification-v02-adaptive.md).

### Whole-packet inventory (0.2)

`Inspection::observe_packet_info(buf.info().unwrap())` accepts a complete whole
packet's metadata and returns the same `SequenceObservation` as the capture-event
API, without copying or converting its payload. Feed each frame exactly once on
the ordered reader path, before dispatching its owning PacketBuf to a worker.
Do not also feed a capture event for that frame. Workers borrow frame/DATA spans
and return the owning buffer for reuse; inspection never retains those borrows.

Call `mark_incomplete()` on a whole-reader tail, error or early stop; call
`finish()` only after clean EOF. Entry-limit failures preserve prior counts and
stop subsequent observation. Recovery remains sticky for unseen presence facts,
and sequence uncertainty includes skips attached to other channels' packets.

Inspection keeps its existing caller-set channel/row entry limits, independently
of input pool B. Its BTreeMaps allocate on first-seen entries; those consumer
output allocations are not charged to B. Once entries and input buffers are
warmed, repeated metadata observation and ordinary buffer reuse allocate zero in
the scoped test. This is not a claim about arbitrary worker/channel allocations.
Ordinary returns retain the warmed high-water storage; a rare large return or
explicit pressure trim can require re-warm before that guarantee resumes. See
[slice-5 evidence](verification-v02-s05.md).

### Ordered consumers and optional delivery (0.2)

The executable `ordered` example moves complete owning buffers through bounded
channel-affine workers, with inspection/control notifications on the producer.
See [the consumer/streaming contract](streaming.md) for setup/time ordering,
credit-return/shutdown rules, live-input limits and implemented explicit
next_chunks protocol (Begin/Chunk/Abort callbacks, returned End). Admission and
checksum counters below cover complete observations only; chunks/drains never
fabricate PacketInfo or complete-recording inventory credit.


### Admission-evidence inventory (owner 2026-11-26, decision 3A)

`Inspection::admission_counts()` returns non-exhaustive `AdmissionCounts`: fixed,
checked u64 counters for evaluated_packets, over_soft, within_soft, next_header,
unconfirmed_exact_eof, stream_opt_in and legacy_setup_any_channel. Only successful
`observe_packet_info` observations contribute. Current authorization kinds
partition evaluated_packets; over_soft and legacy are overlapping evidence facts.
Legacy counts the reader's reported allowance use, not simply an enabled option.
These are size-policy facts, not payload authenticity or checksum validation.

Capture events lack admission evidence and leave these counters unchanged. Compare
evaluated_packets with totals().packets for coverage; zero coverage is not proof
that no oversoft input existed. Tails/refusals/errors add no complete observations;
retained counts describe accepted input only, qualified by scan status/recovery.
Every addition and ordinary inventory count is computed before committing state;
limit/overflow failure preserves all prior counts, rows and sequence/recovery state.
The counters add seven fixed scalars, no entry growth or warm-path allocation.

Output change: the public `inspect` and `ordered` example summaries append
`admission=AdmissionCounts { ... }`. Capture-only inspect reports evaluated_packets=0;
ordered reports whole-packet evidence. Existing fields and exit policies remain.
The examples' Debug output is illustrative, not a stable serialization format.
See [verification](verification-v02-admission-counts.md).


### Explicit owned coalescing (owner decision 4B)

`buf.coalesce_until(deadline)` returns an independent `CoalescedPacket` with
borrowed frame()/data(), preserved info() and explicit evidence() describing the
copy and its budget cost. Check contiguous() first; coalesce_until always copies,
even for one block. Spans/contiguous() are the default allocation-free borrow. Original and copy
are one transport packet, so inspect only one. The original is unchanged on error.

B covers old blocks plus new capacity and metadata simultaneously. Default B
refuses a maximum 128 MiB setup copy with BudgetTooSmall; raise B above 256 MiB
plus overhead or consume spans. Copies may run concurrently with reader assembly:
shared credits govern admission, with priority only for block-credit waiters
(next_block_until). Readers queued for the assembly permit (begin_until) do not
block copying. PoolStats::waiting includes both classes; waiting_for_credits
is the subset that causes WouldBlock without idle eviction.
Credit shortage also returns WouldBlock
without evicting warm idle blocks; successful credit admission may reclaim idle
storage and require re-warm. Release/retry explicitly. Unlike pool begin_until/
next_block_until, coalesce_until never waits for credits: _until bounds only the
copy, and shortage returns WouldBlock immediately; no try_ variant is needed. Deadlines/cancellation are cooperative between bounded copies,
not inside allocation/page faults. PoolStats::coalesced_bytes are returnable live
credits, included in used_bytes and released after storage. See [evidence](verification-v02-coalescing.md).


### Explicit omission with drain

`WholePacketReader::drain_next(deadline)` returns DrainEvent::Report with offset,
header/declared ranges, requested/consumed frame bytes, recovery, reason and
Drained/Truncated disposition. Errors return DrainError with typed error and an
Aborted report when a header was available. Header-finding tails remain separate
DrainEvent::Tail. No PacketInfo, successful chunk End or inventory observation.

This explicit call consumes a declared remainder even above H/B/target whole
limits, using fixed charged scratch with no packet credits or assembly wait.
It does not corroborate or authorize whole delivery; contents stay unverified.
Normal next_packet_into never falls back. Owner Q8 (2026-11-28) makes only
H/B/target size refusals positioned and nonterminal; drain_next then consumes
that same cached header/body, or the caller may stop. Interrupted
retries preserve progress; deadlines/cancellation are cooperative including
recovery. Source mutation, I/O error, deadline, cancellation and truncation are
terminal. No body rescan or silent prefix. A complete omission establishes framing
for EOF/tail handling only. Mark unfiltered inventory incomplete and invalidate
omitted setup/time context as appropriate. See [streaming contract](streaming.md)
and [drain evidence](verification-v02-drain.md). Chunk delivery is explicitly available as described below.


### Whole-mode packet-data checksums (owner 2026-11-26)

Verification is ON by default. Explicit `WholePolicy::with_data_checksum_verification(false)`
skips work for speed and reports NotChecked when a checksum is declared; no
checksum remains Absent. `PacketInfo.ranges.data_checksum` gains Verified
{width,value} and Mismatch {width,stored,computed}, with byte widths 1/2/4.
Mismatch is evidence on a complete packet, never a default refusal/drop/recovery.
Accumulate each arriving pooled span once, covering body plus filler and excluding
both headers and stored checksum, without payload copies or a final frame pass.
Capture mode is unchanged. Coalesced metadata preserves the result.

`Inspection::data_checksum_counts()` partitions accepted complete observations
into absent/not_checked/verified/mismatch; capture and opt-out report coverage
explicitly. Counters commit atomically with existing inventory state. Example
summaries append `data_checksums=...`; they retain counts after errors and keep
their existing exit policy. See [scope and cost evidence](verification-v02-data-checksum.md).


Q8 positioned refusal detail: `refused_header()` borrows the checked primary.
`position()` stays at frame offset + 24; body bytes are not logically consumed.
Fixed read-ahead may already hold payload bytes. Retrying whole delivery retries
the SAME frame and recovery evidence, never skips it. Only H/B/target size errors
are nonterminal; Uncorroborated, allocation, cancellation/deadline, source/I/O and
structural failures remain terminal. into_inner discards cached header/read-ahead;
stopping does not rewind the underlying transport. The cache is one fixed
metadata record charged through reader size at construction, never a body buffer.


### Explicit provisional chunk transfer

```rust
use irig106_core::{BlockPool, ChunkEvent, ChunkOutcome, WholePacketReader, WholePolicy};
use std::{io, time::{Duration, Instant}};
let mut reader = WholePacketReader::stream(&b""[..], WholePolicy::new(), BlockPool::with_defaults()?)?;
let result = reader.next_chunks(Instant::now() + Duration::from_secs(1), |event| {
    match event {
        ChunkEvent::Begin(_) => { /* open provisional decoder transaction */ }
        ChunkEvent::Chunk(c) => { /* borrow c.data only for this callback */ let _ = c.data; }
        ChunkEvent::Abort(_) => { /* invalidate provisional output */ }
        _ => {}
    }
    Ok::<(), io::Error>(())
});
if let Ok(Some(ChunkOutcome::End(end))) = result {
    // Commit staged output only here, after checking final integrity evidence.
    let _ = end.ranges.data_checksum;
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

The fixed 4 KiB CHUNK_BYTES workspace and fixed control metadata are budgeted
at reader construction; next_chunks needs no packet blocks or credit waits.
Callbacks return caller-owned errors without boxing; ChunkError retains both the
original cause and any failed Abort notification. Borrowed frame bytes and DATA
intersections cannot escape the callback. H/soft authorization still applies;
whole-frame B/isize storage does not. Q8 positioned whole-size refusals can be
followed by explicit chunk transfer when H/authorization allows, or explicit drain.
End is a returned ChunkOutcome, never a callback or PacketInfo. Source mutation,
truncation, sink/I/O failure, cancellation or deadline prevents End and invalidates
provisional bytes. Received/offered/acknowledged progress is distinguished;
Interrupted retries preserve it and WouldBlock is terminal. Deadlines are
cooperative and cannot preempt blocking I/O/callbacks. See [protocol, inventory
scope and rollback obligations](streaming.md#explicit-callback-scoped-chunk-delivery).
