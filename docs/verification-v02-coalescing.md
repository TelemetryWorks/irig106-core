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
copy; ordinary callers should borrow contiguous() for zero-copy access.

Old block CAPACITY and new Vec capacity/metadata are simultaneously charged to B.
Reservation is fallible and based only on an already received complete frame;
allocator-visible excess fails closed. No extra zero-fill pass. A separate
returnable coalesced credit class participates in stats, block growth and
constructor availability; it is not treated as permanent workspace during future
frame feasibility. Storage is freed before credits return, including cancellation.
PoolStats::coalesced_bytes includes payload and copied-owner metadata.

A working set unable to fit with its own source and fixed commitments returns
BudgetTooSmall. An active assembly or other retained credits returns WouldBlock:
coalescing NEVER waits while holding source storage another build may need.
Explicit pressure may reclaim idle blocks, requiring re-warm. The caller may
release owners/retry; no fallback or silent discard is selected by core. Deadline
and pool cancellation are checked before/after and between <=512 KiB copies;
they cannot interrupt an OS allocator/page fault. Failure preserves the source.

Evidence:
- Byte-exact two-block fixture with secondary header, odd DATA, filler and width-4
  trailer; preserved metadata, independent lifetime, Send and drop/cancel credits.
- Impossible budget, active assembly and retained-credit refusal; successful retry
  after idle reclamation; expiry/cancel leave input readable. All joins bounded.
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
outcomes are recorded in the PR after execution. Local mutants removing copied
credits from block-growth checks, leaking copy credits, permitting an active
assembly, or omitting post-copy cancellation all fail with assertion panics/exit
101 under bounded watchdogs; all discarded before final lint/diff checks. The checksum feature next will
verify whole-mode body+filler incrementally by default with an explicit speed
opt-out; this PR only preserves existing integrity evidence, never recomputes it.
No merge, tag or publication is authorized.
