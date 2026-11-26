# Core 0.2 explicit budgeted coalescing (owner decision 4B)

Base main 0a1d693, containing approved counters #27. Tests-first 0fd7992 fails
for the absent CoalescedPacket/coalesce_until/coalesced_bytes APIs. This PR is
coalescing only; the approved order continues with data-checksum verification,
explicit drain, then chunk delivery. No version/dependency/release change.

`PacketBuf::coalesce_until(deadline)` explicitly copies an already complete frame
into an independently owned CoalescedPacket. Normal reading/contiguous()/spans
never invokes it. frame()/data() borrow immutable copied bytes; info() preserves
routing, ranges, secondary and admission evidence, including checksum state.
It is the same transport observation: never feed original and copy to Inspection.
Every successful call exposes offset, copied bytes, source blocks and charged
bytes through non-exhaustive CoalescingEvidence. Even one-block explicit calls
copy; check contiguous() first for zero-copy access.

Old block CAPACITY and new Vec capacity/metadata are simultaneously charged to B.
Reservation is fallible and based only on an already received complete frame;
allocator-visible excess fails closed. No extra zero-fill pass. A separate
returnable coalesced credit class participates in stats, block growth and
constructor availability; it is not treated as permanent workspace during future
frame feasibility. Storage is freed before credits return, including cancellation.
PoolStats::coalesced_bytes includes payload and copied-owner metadata.

A working set unable to fit with its own source and fixed commitments returns
BudgetTooSmall. An active reader assembly is allowed: coalescing uses only shared
byte credits, claimed under the same lock as block-growth provisional credits.
Unlike pool begin_until/next_block_until, coalesce_until never waits for credits:
_until bounds only the copy, and a shortage returns WouldBlock immediately.
Credit preflight accounts all reclaimable idle blocks before eviction; WouldBlock
preserves warm idle storage. Successful credit admission may reclaim idle blocks,
requiring re-warm. There is no waiting-build priority gate: copies never wait,
and a reader can wait cancellably with a deadline for copies/input to return.
The orchestrator must release input before waiting for an output merger. The caller may
release owners/retry; no fallback or silent discard is selected by core. Deadline
and pool cancellation are checked before/after and between <=512 KiB copies;
they cannot interrupt an OS allocator/page fault. Failure preserves the source.

Evidence:
- Byte-exact two-block fixture with secondary header, odd DATA, filler and width-4
  trailer; preserved metadata, independent lifetime, Send and drop/cancel credits.
- Impossible budget and retained-credit refusal; successful copy during active
  assembly, retry after idle reclamation; expiry/cancel leave input readable.
  All joins are bounded. Failed WouldBlock leaves idle/allocated/used unchanged.
- A gated block build waits for coalesced credits, then completes after copy Drop;
  this pins returnability and prevents false BudgetTooSmall/permanent workspace.
- Injected clock/cancellation deterministically interrupt after a block copy and
  verify rollback. Public borrowed lifetime/non-exhaustive doctests.
- Existing ignored 128 MiB maximum-setup test now asserts default B returns
  BudgetTooSmall without waiting/changed credits. Existing all-OS gates stay at
  12/24/80/144 MiB. A NEW explicit coalesce case uses B=272 MiB, holds old128+new128
  simultaneously, verifies full bytes/credits and next packet, with a separate
  272 MiB process ceiling (256 MiB touched payload +16 MiB fixed margin).

Existing warmed zero-allocation tests remain: coalescing is outside that hot path.
Exact-head CI, read-only publish dry run, measured peaks and focused mutant
outcomes are recorded in the PR after execution. Initial-head controls for block
growth accounting, credit release and post-copy cancellation failed under bounded
watchdogs and were discarded. The initial exclusivity assertion is superseded by
the coordinator's concurrent contract below. The checksum feature next will
verify whole-mode body+filler incrementally by default with an explicit speed
opt-out; this PR only preserves existing integrity evidence, never recomputes it.
No merge, tag or publication is authorized.


## Review corrections (coordinator calls a–c)

Tests-first bc5f26e pins the revised concurrent contract. The old head fails the
active-copy and real-reader pipeline tests and drops an idle block on a failing
WouldBlock path. The revised preflight permits concurrent copies without changing
permanent workspace during assembly. A reader paused after one body block permits
a worker copy; after resuming, it waits at B until the worker releases its original
input, then completes with the copy still live. Accounting is asserted at each
handshake. No special waiting-build fairness policy is imposed.

A platform-independent binary search finds exact minimal B: used==B succeeds,
B-1 returns BudgetTooSmall with exact needed/limit (M9), and PacketBuf construction
with a live exact-fit copy returns WouldBlock without exceeding B (M11). Existing
test-only allocator instrumentation now tracks largest alloc/realloc requests:
BudgetTooSmall and WouldBlock refusals request no frame-sized allocation (M1).
Patterned bytes across eight sizes catch span displacement and prove even a
one-block explicit coalesce owns a separate copy. Reported metadata charge is
pinned against the copied owner's size. Source/lease drop ordering is unchanged.

Local review controls M9 (workspace omitted), M11 (constructor ignores live copy)
and M1 (reserve before credits) now fail with assertion panics/exit101. Reinstating
the exclusivity gate, restoring evict-then-WouldBlock, or omitting owner metadata
charge (M14) also fails. All temporary mutations were discarded before final lint.
The copy's field declaration order still frees the Vec before its lease.
