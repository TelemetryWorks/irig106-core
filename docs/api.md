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
ReaderLimits literals now need recovery_bytes (or `..Default::default()`).
All errors/tails terminate iteration; repeated calls yield None. This is framing
completion, not full packet/recording conformance. Behavior follows
[requirements](requirements.md).

```text
ReaderLimits {
    buffer_bytes: usize,           // default 8192; nonzero
    capture_bytes: usize,          // default 65536
    recovery_bytes: Option<u64>,   // default None, NOT a fatal 1 MiB cap
}
PacketReader<R: std::io::Read>::new(input, limits) -> Result<Self, Error>
PacketReader::next_packet(capture: impl FnOnce(&Header) -> usize)
    -> Result<Option<Event>, Error>
Event::Packet(Packet)
Event::Tail { offset, bytes, kind, recovery }
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
check trailer space, packet maximums, the nonempty-DATA rule or recording-wide
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

Primary checksum is mandatory. Secondary evidence keeps stored checksum, byte
sum and alternative word sum over the ten non-checksum bytes (time RDG-008).
The selected reading is byte sum; preserve the interpretation's suspect status
and word-only matches. Reserved fields are separate evidence. A bad secondary
checksum does not invalidate primary RTC or erase a structurally complete packet.
Time consumers determine which time answers remain usable.

Trailer widths/ranges are checked, but packet-data checksum contents are
`NotChecked` in 0.1 (or absent when flags select no checksum), never `Valid`.
No payload meaning, secondary time conversion, calendar policy or TF codes move.

The optional inspection accumulator has explicit channel/row caps and checked
counts, emits sequence/presence observations and retains no packet history.
Summary formatting, diagnostic samples, names, filtering, output staging, exit
thresholds and `irig106 inspect` itself remain downstream. An observed TMATS
packet is not valid TMATS; absence after an incomplete scan is inconclusive.
