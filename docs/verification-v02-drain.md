# Core 0.2 explicit drain (L2-WHOLE-008)

Originally branched from main 7273764 independently of #29. Owner Q7/Q8 on
2026-11-28 approved default-ON checksums and drain option A. #29 is now merged;
main 7b3783c was merged in (no rebase), preserving both API docs/package entries.
Tests-first compilation failed for the absent drain APIs before implementation.

`WholePacketReader::drain_next(deadline)` is the ONLY opt-in entry point. It
finds/checks a primary header and ranges, then discards exactly the declared
remainder through the existing charged 4 KiB scratch. Pending hybrid read-ahead
is consumed before source reads. No PacketBuf/block/assembly lease, new payload
allocation, credit wait, seek probe or hidden fallback. Above-H/whole-B/target
refusals are reported as reasons; ordinary omission is CallerRequested.
This is NOT whole admission: oversoft bytes may be explicitly discarded without
claiming size authorization or a valid next header. No PacketInfo is produced.

Reports include declared header/ranges, absolute offset, requested/consumed frame
bytes, recovery and Drained/Truncated/Aborted disposition. Counts include primary
bytes but exclude preceding recovery. Real I/O errors retain typed causes through
DrainError.error, with an inline aborted report when a header was found. Errors
before a valid header retain position/recovery but no fabricated report. Inline
errors intentionally allow result_large_err: allocating a Box while reporting
failure would undermine allocation-free bounded failure handling. A compile-time
size assertion covers metadata in the existing 512-byte allowance.

Source stability is rechecked; mutation aborts even after all bytes arrived.
Pool cancellation and absolute deadlines are checked during header recovery,
body reads and Interrupted retries, and before committing Drained. No check can
interrupt an arbitrary blocking I/O operation; transport adapters own that bound.
Drain errors/truncation are terminal. Whole H/B/target refusals alone are positioned. No sync search inside a refused/partial body.
A fully drained frame establishes framing for subsequent EOF/tail handling only;
it is not a complete delivered/inventory packet. Drained contents are deliberately
unverified (Absent/NotChecked), so no checksum coverage may be claimed.

Evidence: independent hand-packed fixtures; normal refusal never auto-drains;
read-ahead/offset exactness, full/active pool with no wait, impossible budget,
~4 GiB short claim (32-bit too), nested body sync never rescanned, short and
Interrupted reads, exact I/O/cancel progress, recovery/cap/header errors,
source mutation and deterministic deadline expiry (including final-body commit).
Joins and infinite-Interrupted probes have watchdog bounds. Scoped allocator
oracle asserts zero allocations for successful omission and deadline failure.
CI's new 64 MiB synthetic drain case holds zero blocks and resumes at the next
frame under the 12 MiB all-OS memory ceiling; all old gates stay unchanged.
Eight temporary controls fail with assertion panics/exit101: wrong H/B reason,
secondary-header remainder error, invented consumed bytes, nonterminal truncation,
all post-read unchanged() checks removed, all explicit boundary deadline checks
removed (inner read/retry hooks retained), and missing framing commit. Removing
one redundant final source/deadline check alone is not claimed killed.
All mutations were discarded. CI run IDs and measured peaks are recorded in the PR.
No merge, tag, publication, chunk implementation or new dependency in this PR.


Review follow-up (tests only; refusal/drain API decision held): D1 pins P == H
as CallerRequested when B fits and P == H+4 as HardLimit. D3 changes a stable
source stamp at the end of its first 4 KiB body quantum in a three-quantum frame;
SourceChanged/Aborted must report only header+one quantum consumed, strictly less
than requested. This pins the purpose of the current per-quantum source recheck,
without changing the I/O/stamping strategy. The new tests kill >= at H and removal
of only the per-quantum unchanged() call, while retaining the final check.


Q8: one validated header/ranges/recovery cache keeps the body positioned after
H/B/target errors; refused_header() exposes primary metadata, not PacketInfo.
Only those three errors retain it; other errors clear it and terminate. Repeated
whole calls re-evaluate the same frame. Explicit drain uses the cache, even on a
live nonseekable source. Refusal→drain→normal packet and refusal→stop/no automatic
drain are tested for H/B; a short huge claim pins TargetLimit on i686, and a
CI-only full synthetic 4 GiB i686 test pins target refusal→drain→normal packet.
Expired explicit drain after recovery retains aborted progress and recovery
without additional consumption. Constructor accounting includes the new fixed
cache. Q7 checksum policy is untouched; explicit omissions remain NotChecked.
