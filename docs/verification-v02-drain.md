# Core 0.2 explicit drain (L2-WHOLE-008)

Branched from main 7273764, independently of parked checksum PR #29. Rebase onto
main after that PR merges; this PR changes no checksum default or delivery policy.
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
Any error/truncation is terminal. No sync search inside a refused/partial body.
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
omitted source checks, omitted final deadline checks and missing framing commit.
All mutations were discarded. CI run IDs and measured peaks are recorded in the PR.
No merge, tag, publication, chunk implementation or new dependency in this PR.
