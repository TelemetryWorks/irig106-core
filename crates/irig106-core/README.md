# irig106-core

irig106-core 0.2.0 supplies structural Chapter 10 traversal using the 106-24R1
Chapter 11 layout. The library is safe, std-only Rust (edition 2024, MSRV 1.85).
Its sole dependency is irig106-types ^0.1 with default features off, including
re-exported Rtc. No types 0.2 release is required.

Header::parse validates the 24-byte primary sync/checksum/basic lengths; a header
alone does not establish a complete packet. PacketReader/ReaderLimits preserve
the bounded DATA-prefix API: uncaptured DATA/filler/trailer are drained, packet-data
checksums remain Absent/NotChecked, and retained captures are the caller's concern.
Default sync recovery continues to EOF with bounded memory; optional recovery
caps are explicit. NoCompletePacket and tails preserve observed progress.

WholePacketReader and reusable PacketBuf add complete-frame delivery. BlockPool
uses lazy 512 KiB blocks. WholePolicy defaults H to 128 MiB and BlockPool defaults
B to 256 MiB, independently caller-overridable. No reservation derives from
declared Packet Length; growth follows received bytes with one bounded provisional
block. Ordinary standard-sized packets (P <= 512 KiB) fit one contiguous slice;
larger packets expose borrowed spans. One fixed budgeted
64 KiB read-ahead buffer amortizes headers/small bodies; large unbuffered
remainders read directly into pooled storage. The small-body cutoff defaults to
4 KiB and is configurable. Steady-state ordinary reuse allocates zero in scoped
tests; cold growth, explicit coalescing and first-seen inspection rows differ.

Oversoft admission records NextHeader, stable Unconfirmed(ExactEof), or explicit
non-seekable StreamOptIn. The setup allowance defaults to channel0/type01; an
explicit legacy option extends it to type01 on any channel with evidence. Edition
is never inferred from Data Type Version. StableSource adapters must exclude
undetectable concurrent mutation; Read+Seek alone does not establish stability.

Packet-data checksum verification is ON by default in whole/chunk delivery,
with an explicit opt-out for speed. Verified/Mismatch includes width and sums;
mismatch does not reject by default and is not payload authenticity. Secondary
integrity retains both byte/word readings and reserved evidence, without time
interpretation. Payload decoding and recording-wide conformance stay downstream.

Explicit coalesce_until copies under old+new budget accounting: check contiguous()
first. Maximum-setup coalescing needs a raised B; default B returns BudgetTooSmall.
HardLimit/BudgetTooSmall/TargetLimit whole refusals retain the checked header, so
callers can drain_next or stop; a log-and-continue loop otherwise spins. Drain
reports omissions. next_chunks lends fixed 4 KiB provisional chunks to callbacks;
only the returned End commits, Abort invalidates output. Neither path silently
falls back or creates complete PacketInfo inventory credit. Cancellation and
deadlines are cooperative; they cannot interrupt blocking I/O or callbacks.

Inspection has explicit channel/row limits and checked counts, admission/checksum
coverage and per-channel sequence evidence. Feed each complete frame once, finish
only on clean EOF, and mark_incomplete after omissions/errors/early stop.
Recovery makes unseen presence Unknown; setup presence remains channel-0 only.
Expected sequence successors across skipped input are Unverified, not proof of
continuity/loss. Pool B excludes caller queues/output and inspection maps, which
need separate bounds. Ordinary returns retain the warmed set under B; large
returns/explicit pressure trim target eight idle blocks.

See the release-pinned [API](https://github.com/TelemetryWorks/irig106-core/blob/v0.2.0/docs/api.md),
[migration](https://github.com/TelemetryWorks/irig106-core/blob/v0.2.0/docs/migration.md),
[streaming protocol](https://github.com/TelemetryWorks/irig106-core/blob/v0.2.0/docs/streaming.md)
and [requirements/coverage audit](https://github.com/TelemetryWorks/irig106-core/blob/v0.2.0/docs/coverage-0.2.md).
The inspect and ordered examples demonstrate bounded consumers. Linux/macOS/Windows
memory probes and i686 functional tests provide scoped regression evidence, not
a general memory guarantee. RSS only sees resident touched pages. No payload
meaning, universal OOM immunity, async transport, or no_std/alloc support is supplied.
