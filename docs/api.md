# Proposed 0.1 API contract

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
unfiltered scan; otherwise Unknown. Previously observed Present facts survive
incomplete scans. The caller must mark read errors or omitted input explicitly.
Sequence continuity and user-facing reports remain later work.
