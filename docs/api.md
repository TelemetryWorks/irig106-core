# Core 0.1 development API contract

**Implemented in slice 2:** `Header::parse(&[u8])`, `primary_checksum(&[u8; 24])`,
`HeaderError`, `PRIMARY_HEADER_LEN` and re-exported shared `Rtc`. `Header` stores
immutable primary bytes and exposes accessors named below (`channel()`, etc.).
**Slice 4** returns `Result<_, Error>` from reader construction/iteration.
`Error.reason: ErrorKind` distinguishes I/O, HeaderError, NoCompletePacket,
RecoveryLimit, invalid buffer/capture bounds, allocation and offset overflow.
Errors retain consumed `position`, optional `recovery`, and their original source.
`kind()`/`get_ref()` retain broad seed classifications and HeaderError downcasts.
NoCompletePacket carries optional `Tail` metadata; Event::Tail adds recovery.
Slice 5 makes Error, Recovery, Tail, ReaderLimits, Packet, SecondaryIntegrity,
and PacketRanges non-exhaustive; Event::Tail is a non-exhaustive variant.
Construct limits with ReaderLimits::new()/Default and with_buffer_bytes,
with_capture_bytes, with_recovery_bytes builders. Other evidence is read through
fields or patterns containing `..`; consumers cannot construct struct literals.
Event and TailKind are also non-exhaustive. Every field-carrying ErrorKind
variant is non-exhaustive: Io/Header/Allocation now use named `source` fields,
so consumers can match `ErrorKind::Header { source, .. }`. Non-exhaustive tuple
variants would be private to external matching in Rust. ErrorKind Display
includes NoCompletePacket tail kind/bytes/offset; Error Display appends recovery
start/scanned bytes when present. Typed sources and classifications are unchanged.
All errors/tails terminate iteration; repeated calls yield None. This is framing
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
configuration in 0.1; keep parsing and I/O separate for later consumers.

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

Trailer widths/ranges are checked, but packet-data checksum contents are
`NotChecked` in 0.1 (or absent when flags select no checksum), never `Valid`.
Packet.ranges exposes packet-relative u32 ranges for DATA, filler and checksum.
Widths from flag bits1–0 are 0/1/2/4; overlaps/insufficient space are fatal typed
HeaderError before capture. Variable filler is allowed to preserve per-channel
packet size (§11.2.1.4); filler byte values are not checked. Checksum contents are
DataChecksumState::Absent or NotChecked, never Valid. No payload meaning,
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

The virtual-workspace library targets the 0.1.0 release. CI checks
the exact crate file list and runs cargo package with archive verification; the
inspect example executes against piped fixtures. A separate Linux CI job measures
only the prebuilt release core driver using GNU time, outside fixture generation
and compilation, with a 32 MiB peak-RSS ceiling. The input includes a complete
64 MiB packet with 4-byte capture (L1-CORE-002) and 48 MiB mid-file recovery
(L2-REC-003), plus dense small packets and padding. See [slice 8](verification-s08.md)
for input parameters and negative control. These checks do not establish whole
recording conformance, validate payload/checksum contents, or authorize release.
The Trusted Publishing pipeline keeps publication separately authorized.

## 0.2 development: storage primitives (slice 2)

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
[slice-2 evidence](verification-v02-s02.md) for partial requirement coverage;
these primitives do not yet supply packet admission, valid-byte assembly or I/O.

### Unreleased whole-packet integration (0.2 slice 3)

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
A terminal tail/error is followed by None, preserving 0.1 completion semantics.
Wait deadlines bound pool waits, not a blocking source's Read/Seek.
DeadlineExceeded or Cancelled from next_packet_into is terminal: the header and
possibly body bytes have already been consumed, PacketBuf is cleared, and calling
again returns None. Returning credits does not resume the interrupted frame.
into_inner discards recovery read-ahead; its physical cursor may be up to 8 KiB
past position(). A caller restarting must explicitly establish a frame boundary,
not retry from the reported consumed position or assume the source was rewound.
The low-level pool deadline API remains reusable after timeout; this reader's
consumed-input contract is different. Error::kind maps Cancelled and permanent
BudgetUnavailable to Other, not retryable Interrupted/WouldBlock.

The shared framing engine uses a zero-capacity input wrapper in this mode:
normal header reads use fixed primary scratch; ordinary bodies bypass buffering
into final pooled slices bounded by remaining block/frame (at most 512 KiB).
Recovery alone reads fixed 8 KiB scratch chunks, charged to B at construction.
Bytes prefetched after a recovered header are copied from that bounded scratch;
the remainder goes directly into final storage. The next header may also lie in
this recovery read-ahead. Logical position/recovery counts advance on consumption,
not prefetch. Seek probes preserve this pending scratch and restore the physical
source cursor. No normal-path body read-ahead or second body staging buffer is
introduced. One-block ordinary packets stay contiguous. Additional blocks are acquired only
after the preceding one fills. Interrupted reads retry in place. The existing
0.1 input buffer/capture behavior is unchanged.

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
Secondary integrity stays structural; DATA checksum contents remain NotChecked.
Explicit coalescing, drain/chunk delivery, broader platform/RSS controls and
performance comparison remain later slices. See [slice-3 evidence](verification-v02-s03.md).

Exact-EOF's immediate pre-admission stamp recheck can catch a change between
metadata queries; it does not protect the later read interval. The post-body
stamp/consumed-size recheck is the guard over body consumption. Neither query
proves the adapter's promise against undetectable same-size concurrent writes.
