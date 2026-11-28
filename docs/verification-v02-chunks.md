# Core 0.2 explicit chunk-delivery evidence

Owner decision 4B (2026-11-26), L2-WHOLE-008/L2-BUD-008 and
L3-CHUNK-014: explicit callback-scoped streaming is the final feature, separate
from drain and complete PacketInfo delivery. This PR is stacked on #30's Q8
positioned-refusal changes; it does not merge either feature or authorize release.

## Contract and accounting

next_chunks is explicit; whole delivery never falls back. Begin and borrowed
Chunk callbacks are provisional. Only a returned ChunkOutcome::End commits;
no callback follows it. Failures after attempted Begin attempt Abort and retain
both original and Abort sink errors inline. Consumers must roll back or label
provisional output; callbacks may not retain borrowed slices. A panic propagates
with the reader terminal, without a guaranteed Abort during unwinding.

The existing fixed 4 KiB scratch is reused. An additional fixed 1,024-byte control
allowance is charged at reader construction, with a compile-time size assertion
for transfer/evidence/checksum state. No per-transfer allocation, payload block,
assembly or credit wait occurs. Callback/source/error-type-owned state is external
and needs its own bound; the accounting assertion is not a portable thread-stack
or compiler-temporary memory guarantee. One reader cannot execute two modes at
once. Existing whole/capture APIs, hybrid read policy and all old memory ceilings
stay unchanged.

Primary bytes are the first 24-byte chunk. Subsequent frame-relative ranges are
monotone, disjoint and at most CHUNK_BYTES; DATA intersections exclude secondary,
filler and trailer. Counts distinguish logical received, offered and successfully
acknowledged bytes, excluding recovery. Failed reads may receive bytes that were
never offered. Sink acknowledgement does not assert semantic consumption.

H/edition soft admission, restored next-header probing with pending read-ahead,
Unconfirmed(ExactEof) and explicit StreamOptIn remain required. Whole-frame
B/isize storage feasibility does not apply: fixed streaming storage can follow
a positioned whole BudgetTooSmall/32-bit TargetLimit refusal. H/absolute offset
overflow still refuse before Begin and retain the header for explicit drain/stop.
Known-stable-short input retains truncation-only handling with no Begin. Unknown
stream EOF after Begin aborts; before establishment it preserves NoCompletePacket,
otherwise it is typed Truncated. All other errors after Begin are terminal.

Source checks occur after each read quantum and before End. Cancellation/deadlines
cover recovery, reads, Interrupted retries and callbacks cooperatively; arbitrary
blocking source/sink work still requires transport/orchestrator bounds. No credit
cycle is introduced because no packet-credit lease/wait exists in this mode.

Checksums accumulate once over arriving DATA plus filler, preserving Q7 default
ON / opt-out and Absent. End carries Verified/Mismatch without a default refusal,
plus secondary integrity and final admission evidence. Begin has provisional
checksum evidence. No PacketInfo/Inspection observation is fabricated. End does
not prove semantic setup/time validity; stage state until End and semantic checks.

## Executed checks and qualification

Hand-packed patterned fixtures and independent weighted-byte checksum oracles
cover widths 0/1/2/4, secondary headers, odd DATA, filler and multiple chunks with
three-byte/Interrupted reads. Tests pin exact frame/DATA bytes, ranges and final
integrity, mismatch/opt-out, recovery separate from payload, tails, panic, Begin
rejection, original and secondary sink errors, actual received-but-unoffered I/O
progress, cancellation and source change after the last callback, deterministic
deadline expiry (including infinite Interrupted), and no successful End on abort.
Watchdogs bound looping/recovery/Interrupted cases. Compile-fail doctests reject
retained chunk borrows and an attempt to feed End to complete-packet inspection.

A budget-refused frame is explicitly streamed with restored seek probe/pending
read-ahead, then the next frame reads normally. An active build holding all pool
blocks does not prevent fixed-memory transfer. A 0xFFFFFFFC declaration on a short
source after whole refusal asserts u64 progress, no blocks and truncation; the
same test runs on i686, where whole delivery refuses TargetLimit. A scoped counting
allocator asserts zero allocations on successful transfer and deadline error.

CI extends the existing isolated all-OS resource probe: a synthetic 64 MiB frame
under B = 4 MiB is whole-refused, explicitly chunked with zero blocks, then resumes
at the next normal frame. The 12 MiB process ceiling catches retained input;
source generation/compilation are outside measurement. Linux/macOS RSS and Windows
working-set/private-commit gates remain scoped regression evidence, not a general
memory guarantee. Capture's 32 MiB ceiling and all whole-mode ceilings are unchanged.
CI run IDs and measured peaks are recorded in the PR at its exact head.

Temporary mutation controls remove Abort notification, include headers as DATA,
skip checksum updates, ignore opt-out, omit the final source check, make a sink
failure nonterminal, and corrupt chunk offsets. Each fails an assertion/runtime
test; all controls are discarded. A header-only mutation fixture isolates the
final source check rather than claiming redundant checks are individually killed.
No unsafe library code, new dependency, tag or publication is introduced.
