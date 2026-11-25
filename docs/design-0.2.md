# Core 0.2 slice 1: complete packets with bounded incremental storage

**Design from #16, aligned with owner decisions approved 2026-11-25.**
No whole-packet reader implementation is claimed here. This supersedes the
fixed-capture/drain proposal as the normal 0.2 path. The 0.1 bounded-capture API,
its allocation rule and RSS gates are preserved; the whole-packet requirements
below are approved but await implementation and independent verification.
Version, publishing and library dependencies are unchanged. Baseline: [published-0.1 harness](../scripts/benchmarks/core-0.1/README.md).

## Approved whole-packet requirements

These clauses apply to the **new whole-packet API**, as approved 2026-11-25 and
applied in [requirements](requirements.md). Retain the 0.1 capture behavior and
its independent-of-Packet-Length allocation rule.

| ID | Approved wording |
| --- | --- |
| L1-CORE-002 | Stream with memory bounded by caller limits and a shared aggregate storage budget, independent of recording length. Grow packet storage only as bytes are actually received, never by reserving a declared Packet Length. Include all live packet/pool storage, retained capacity and allocation/growth workspace in budget accounting. |
| L2-IO-002 | Use safe, std-only synchronous buffered Read and fixed-size I/O scratch. For whole-packet delivery, acquire budget credits and fallibly extend storage only for received bytes in bounded quanta. Never preallocate from declared lengths, use mmap/unsafe, or retain a recording. Deliver a complete packet only after its entire declared frame has arrived. Preserve bounded capture as a separate compatible mode. |
| L2-CAP-002 | Preserve the 0.1 DATA-prefix API and its explicit capture cap. The new default whole-packet API applies explicit edition soft thresholds, an absolute caller hard ceiling and aggregate budget; an over-soft packet requires corroboration or explicit stream opt-in. Refusal/drain/chunk disposition must be reported, never represented as an ordinary complete packet. |

Additional design contracts: L2-ADM-008 defines admission/evidence below;
L2-BUD-008 defines budget, cancellation and reuse; L2-WHOLE-008 defines atomic
complete-packet delivery and optional streaming. All three trace to L1-CORE-002
and L1-CORE-004. Tests below are L3 implementation obligations, **not passing
checks**. No other clause, including uncapped default recovery, is relaxed.
Owner approval of requirements does not authorize a release or publication.

Proposed API boundary: the shared PacketReader framing engine gains an explicit
whole-packet mode configured with a non_exhaustive resource policy and shared
budget handle. It returns PacketBuf ownership plus header/ranges/offset, soft
violation and authorization evidence; borrowed views expose complete spans and
DATA ranges. Tail/error/NoCompletePacket rules remain typed and terminal. Builder
constructors select SeekChecked or StreamOptIn; existing bounded-prefix callers
keep their contract. Exact method names are deferred to the implementation review.

## Limits and admission (L2-ADM-008)

Let P be total Packet Length, S the edition/type soft threshold, H the caller's
absolute hard ceiling, B aggregate budget and K fixed I/O quantum. All include
explicit units; P includes headers/trailer, not just DATA. Built-in defaults are
**H = 128 MiB (134,217,728 bytes)** and **B = 256 MiB (268,435,456 bytes)**.
Both are independently caller-overridable. H matches the largest packet allowed
by the baseline standard (setup). **B fits one maximum 128 MiB setup packet
only with the default block-based PacketBuf representation**, plus bounded
workspace and a normal pool. Coalescing that packet into contiguous memory, or
using a contiguous-only Vec representation, requires a caller override with
**B >= 2 x 134,217,728 bytes + overhead** (more than 256 MiB); otherwise the
consumer must read the spans. Charge all retained storage/workspace and apply
the feasibility check below. Neither default causes up-front allocation or
derives from host RAM.

For baseline 106-24R1, S is 524,288 bytes except Computer-Generated Format 1 setup,
134,217,728 bytes. Time and Format 3 root index retain the ordinary maximum.
[Chapter 11](https://www.trmc.osd.mil/wiki/download/attachments/335389015/chapter11.pdf?api=v2),
§11.2.1(c)/Table 11-3, printed 11-4/11-5, states these sizes; §11.2.1.1(c),
printed 11-5/11-6, defines the complete, four-byte-aligned field. These are default
admission thresholds, not claims of payload validity. Exception routing uses
channel 0/type 0x01 by default; an unknown edition/type never earns the setup
allowance automatically. Use explicit edition policy, retain raw codes, never
infer it from type version.

The caller may explicitly enable **legacy_setup_any_channel**, default **false**,
for 106-07/09 distributed-multiplexer recordings (owner compatibility decision).
Only type 0x01 then gains S = 134,217,728 on any channel; other types retain their
ordinary threshold. Never infer this option from channel IDs, setup contents,
Data Type Version or a guessed edition. When the extended allowance is used,
record LegacySetupAnyChannel with the channel and explicit-policy provenance in
admission evidence. It never bypasses H/B/target limits or changes the 0.1 API.

Admission order:

1. Validate primary sync/checksum, structural lengths/ranges and checked offsets.
2. P > H: typed HardLimit before body allocation by default. Explicit caller
   drain policy may consume the declared remainder with fixed scratch and emit
   Rejected/Drained evidence; never a complete Packet or silent prefix. Record
   requested/consumed bytes and EOF/error; optional drain deadlines/cancellation
   prevent a caller-selected discard from waiting indefinitely on a live source.
3. A packet that cannot fit even the **entire** B with all other buffers returned
   and reclaimable capacity released, including unavoidable workspace/metadata,
   returns typed BudgetTooSmall (or explicit reported drain), never a wait.
   P <= S still needs H/B/target representability. Validate fixed per-reader
   scratch/metadata commitments against B at construction as well.
4. P <= S: automatically eligible; no oversize authorization or lookahead needed.
5. S < P <= H: SeekChecked accepts NextHeader or stable exact-EOF evidence below;
   StreamOptIn requires explicit caller authorization before scanning. No implicit
   fallback from failed seeking or validation to trust mode.
6. Acquire build admission, read incrementally, wait for reclaimable credits as
   needed; return a whole packet only at complete frame consumption.

**Soft/hard conflict:** H always wins. A 64 MiB ceiling refuses a 128 MiB setup
packet even though it is within S. To accept that record, caller must configure
H/B accordingly; being standard-compliant never bypasses a resource limit.
The built-in H admits the standard maximum; a lower caller override deliberately
reduces that eligibility. No soft allowance overrides the absolute ceiling.

SeekChecked computes offset+P with checked u64 arithmetic, seeks there through
an exclusive buffered reader, and reads a fixed 24-byte candidate. Require sync,
primary checksum and the same structural header/range checks as normal framing.
Return evidence NextHeader(offset), not a claim the following body is valid.
Restore the **logical** body position on success/failure, invalidate stale
read-ahead through BufReader::seek, and do not charge probe bytes to stream
position/recovery counters. Restore/seek/read errors are typed terminal errors;
never use File::try_clone assuming an independent file cursor. Interrupted is
retried; short probes before the known EOF are uncorroborated. No seeking for
non-seekable sources.

A final over-soft packet whose declared end equals a **known stable EOF** is
admitted by SeekChecked when P <= H and it fits B, with authorization evidence
**Unconfirmed(ExactEof)**. There is no next-header probe at that offset and no
stream opt-in requirement. This is an accepted admission case, not proof of
content validity or a fabricated NextHeader result. The packet must still arrive
completely before delivery; do not allocate from the declaration.

A stable-source adapter must expose a comparable identity and size snapshot;
Read + Seek alone cannot establish this. Recheck size/identity before exact-EOF
admission and again after body consumption, before complete-packet delivery.
Any observed identity/size mutation yields typed **SourceChanged**, no packet.
If stability information is unavailable, exact-EOF admission is unavailable;
never silently assume it or switch to StreamOptIn. The caller's stable-source
contract also excludes in-place content mutation. Unchanged identity/size cannot
prove immutability; use available modification evidence too, and document that
undetectable same-size concurrent writes remain a limitation.

For a stable seekable file whose declared end is *beyond* EOF (P <= H), retain
the **truncation-only** path: fixed-scratch consume to EOF and emit the normal
tail, never admit/deliver that packet. One byte less than declared is this path,
not ExactEof. Preserve actual consumed-byte evidence without storing a body known
incomplete. Recheck stability; observed source mutation is SourceChanged rather
than a misleading exact-EOF authorization or stable-file truncation finding.

A checksum is weak evidence: payload can contain a matching header deliberately
or by accident. Record the authorization method and soft violation on each
admitted packet, enforce H/B anyway, revalidate the next header on arrival, and
do not claim authenticity or full standard conformance. No recursive unbounded
lookahead. Headers found during recovery obey exactly the same admission rules.

## Receive-driven storage and budget (L2-BUD-008 / L2-WHOLE-008)

Default delivery is one owned complete packet; no partial body is dispatched.
Use fixed scratch to obtain up to K real bytes; request storage only after n>0
has arrived, append those n bytes fallibly, and repeat. Scratch, input buffering
and one bounded read-ahead quantum are included in B. Never resize/zero P bytes
or reserve P even if H would allow it. A short admitted input therefore grows
with actual received bytes plus bounded allocation granularity, then yields a
tail. Partial storage is released/recycled; preceding complete packets survive.

A naive Vec::try_reserve(delta) is insufficient: growth may be geometric and a
moving reallocation can transiently retain old and new allocations. Even
[try_reserve_exact](https://doc.rust-lang.org/std/vec/struct.Vec.html#method.try_reserve_exact)
permits allocator excess. Small exact contiguous increments can also repeatedly
copy a large prefix. **Default implementation representation:** opaque PacketBuf
with reusable bounded storage blocks, grown fallibly after bytes arrive, exposing
a complete packet view only at completion. Metadata growth is budgeted too. One
small packet normally occupies one reusable block; large packets occupy several.
Internal segmentation is not incremental delivery: workers receive one complete
packet, with validated DATA ranges over its spans. This avoids a hidden 2P copy.

If a consumer requires one contiguous slice, coalescing is explicit and fallible,
only after bytes exist, with old+new storage charged simultaneously. A maximum
setup packet needs B >= 2 x 128 MiB + overhead, including metadata, scratch and
other retained storage. At default B = 256 MiB, coalescing that packet returns
typed BudgetTooSmall **without waiting**, even with all other buffers returned;
read the complete spans or explicitly raise B. The same conservative minimum
applies to a contiguous-only Vec representation and its moving-growth workspace.
In-place reallocation is not a portable guarantee that permits a smaller budget.
A contiguous-only implementation still needs reviewed growth accounting and a
benchmark comparison; there is no unsafe/unaccounted realloc shortcut.

Credits cover live storage capacity (not just length), idle slots, metadata and
transient replacement/coalescing workspace. Charge bounded allocation quanta
before allocating; touch/initialize only received bytes plus fixed scratch.
Allocator-visible excess must be reconciled before retaining a block; if credits
are unavailable, drop it and return a typed allocation/budget error. Safe std Vec
cannot promise that an allocator never transiently supplies excess: request-credit
assertions and a measured allocator/platform margin are required, not a claim of
an exact instantaneous allocator footprint. B is an accounting bound, not an exact process-RSS
promise: allocator-private overhead, stacks and OS page/commit granularity need
external headroom. No infallible push/reserve/clone/shrink on untrusted paths. Policy, allocation
and synchronization failures return typed errors rather than panic/abort; this
cannot prevent an OS-initiated kill.

Use one active assembly lease per shared budget, including across readers, to
prevent two partial packets from each holding credits needed by the other.
Before consuming a body, establish that this build can eventually finish within
B after prior workers return storage; do not allocate/reserve P to do so. Idle
pool slots can be reclaimed down to the configured baseline. Workers must release
input capacity without waiting for a result merger that itself waits for this
packet. Return queues must accommodate all live slots. Fixed output/reorder
budgets cannot participate in a circular wait with input credits.

Temporary pressure from worker-held buffers blocks on a predicate/condition
variable with cancellation, not a lock held during I/O or worker work. Returning
capacity wakes the reader. Consumer-retained buffers can intentionally stall it;
caller may cancel or set a wait deadline. If credits can no longer be reclaimed,
return typed BudgetUnavailable rather than wait forever. Blocking I/O cancellation
itself depends on the input adapter; a condition-variable wake cannot interrupt
an arbitrary blocking Read. A caller choosing
nonblocking budget acquisition receives WouldBlockBudget with explicitly retained
assembly state; the first implementation may support only blocking/cancellable
acquisition. No busy polling or automatic switch to drain/chunk mode.

On return of a rare large packet, drop excess blocks beyond baseline and return
credits; pressure also trims idle slots. Free/drop is preferred to an infallible
shrink_to_fit; credits follow actual retained capacity. The allocator may keep
freed pages, so lower retained capacity need not immediately lower RSS. Repeated
large packets can trade reuse for footprint via explicit bounded retention policy.

Keep core safe/std-only and types as its sole ecosystem dependency. Budget leases
and PacketBuf are Send; borrowed views cannot survive owner reuse. Core exposes
resource/evidence primitives; pool scheduling, fixed workers, setup/time decoding
and transport adapters remain orchestration. One ordered reader/context stage
handles setup closure (owner INT-045), time epochs, offsets and global recovery
before bounded per-channel fan-out; workers get immutable context epochs and
return ownership without payload copies between pipeline stages.

Optional fixed-chunk delivery is explicitly selected for incremental consumers.
Begin/chunk/end records carry packet identity, offsets, limits and authorization;
chunks are provisional until complete consumption. On EOF/error, no successful
End is emitted. Sink errors are typed and terminal. Chunk consumers must bound
parser/output state; neither chunk mode nor drain is an automatic fallback.
Secondary/trailer evidence remains structural; data checksum contents stay
NotChecked until a separate validation assignment.

## Corner-case decisions

| Case | Required behavior / evidence |
| --- | --- |
| Bogus P mid-file | Never rescan inside an admitted incomplete frame. If lookahead fails, return Uncorroborated before allocation; if admitted, bytes through its declared end belong to it structurally, even if real packets are swallowed. Recovery cannot know payload truth. Preserve preceding packets. |
| Bogus P at EOF | Admissible stream input grows only by actual bytes then returns TruncatedPacket (or NoCompletePacket with tail before any complete frame). Known seekable EOF shortfall uses truncation-only drain above. P > H instead returns HardLimit/reported drain. |
| Approximate 4 GiB claim | u32's largest aligned P is 0xFFFFFFFC, not 2^32. With H=64 MiB/1 GiB this is rejected, not a tail. The receive-growth regression must explicitly authorize H/B/stream policy large enough, or exercise known-EOF truncation; no claim that rules A/B both override one another. |
| Legitimate oversize | S < P <= H plus NextHeader, Unconfirmed(ExactEof) or StreamOptIn, sufficient budget/workspace: deliver whole frame with soft-violation/authorization evidence. No cap inferred from actual file's type-version bytes. |
| 128 MiB setup | Channel0/type01 under this edition is within S. Accept without oversize corroboration only if H/B/target permit; grow as bytes arrive, then return/drop excess capacity. Nonzero-channel type01 needs explicit legacy_setup_any_channel to select this allowance, recorded as evidence; other types never inherit it. |
| Exact EOF / one byte short | Stable end == EOF and P <= H/fits B: admit, labelled Unconfirmed(ExactEof). End one byte beyond EOF: truncation-only path, no packet. Observed size/identity mutation: SourceChanged before delivery. |
| False corroboration | Does not bypass any resource bound or prove packet meaning; retain evidence and revalidate later. No silent recovery promotion of arbitrary data into trusted content. |
| Pipes/live streams | Oversoft packets require explicit StreamOptIn; no probe, seek emulation or whole-stream spool. Blocking read is supported. WouldBlock/timeout after partial input is terminal under 0.1 semantics; resumable nonblocking input needs its own state-machine contract. UDP requires bounded reassembly/loss policy outside core. |
| Recovery/padding | Sync/checksum failures retain scalar progress, byte counts and optional caller search cap; default has none. Checksum-qualified impossible lengths stay fatal. Policy refusals are not invalid sync and do not trigger unlimited rescanning. Trailing padding remains a finding with prior analysis. |
| Pool/backpressure | Budget charges workers and idle buffers; return/shrink releases credits. Inability to fit even with the entire budget reclaimed errors or explicitly drains immediately, never waits. Temporary pressure waits cancellably, never bypasses budget or deadlocks on pending ordered output. |
| Interrupted/short reads | Retry Interrupted without resetting received bytes or budget credit; positive short reads append only n bytes; Ok(0) means EOF. PermissionDenied and other errors remain typed, terminal, and distinct from truncation. |
| 32-bit | Keep offsets/length calculations checked in u64; checked conversions and isize/address-space limits before allocation. H/B/block counts must be representable. Segmentation does not justify unchecked usize arithmetic. Refuse unsupported configuration/packet with typed TargetLimit; no wrap or cast-based allocation. |
| Linux overcommit/OOM | Fallible allocation catches reported allocator failure, not process kills on later page faults. Only received bytes are touched, plus bounded scratch/copy overhead. A genuinely huge arriving stream can still hit a badly configured host/cgroup limit; H/B require headroom. Never promise immunity from OOM-killer/global pressure. |
| Windows commit charge | Heap allocation may consume commit before pages are resident. Incremental actual-byte allocation limits that request, but working-set RSS alone misses commit pressure. Measure private committed bytes too; failure may arrive earlier than on Linux. |
| Edition maximum | Explicit edition policy supplies S; unknown editions/types need a caller policy or conservative ordinary S, with the raw uncertainty reported. A standard size violation is an admission finding, not an inferred edition or permission to allocate. |

Platform sources: [Linux overcommit accounting](https://www.kernel.org/doc/html/latest/mm/overcommit-accounting.html)
and [Windows page states](https://learn.microsoft.com/en-us/windows/win32/memory/page-state).
No settings, overcommit policy or process limits are changed by this design.

## Required L3 controls and subsequent slices

| Control | Oracle / falsifying regression |
| --- | --- |
| L3-DEFAULT-008 | Whole-packet new()/Default H = 134,217,728 and B = 268,435,456; independent builder overrides preserve the other setting; defaults do not allocate H/B or alter 0.1 ReaderLimits. Feasibility still includes workspace/pool. |
| L3-EOF-008 | Stable exact EOF admitted with precisely Unconfirmed(ExactEof), whole bytes and no next-header/stream-opt-in requirement; one byte short takes truncation-only path; identity replacement or size mutation before admission/after body returns SourceChanged with no packet. Missing stability evidence cannot authorize ExactEof. |
| L3-LEGACY-008 | Default off: channel0/type01 selects setup S, nonzero/type01 retains ordinary S. Explicit opt-in selects setup S on nonzero/type01 and records LegacySetupAnyChannel evidence. Other types, arbitrary Data Type Versions and guessed editions never enable it. H/B overrides still apply. |
| L3-GROW-008 | Authorized huge aligned P on tiny short-read input: allocation requests and touched RSS follow received bytes + fixed quantum; exact tail/preceding results. Mutant preallocating/zeroing P must fail. Include a hard-refusal case separately so it cannot mask the growth test. |
| L3-ADM-008 | Real oversoft packet below/exactly/above H; valid/invalid/short next header; false checksum-valid payload header; seek restoration across buffering and recovery; final EOF and nonseek opt-in/refusal. Whole bytes and authorization evidence checked independently. |
| L3-SETUP-008 | At built-in H = 128 MiB and B = 256 MiB, a complete 128 MiB setup packet (P = H) is accepted with the default block-based representation; coalescing it under default B returns typed BudgetTooSmall without waiting, including with all other buffers returned. Contiguous coalescing/Vec requires B >= 2 x 128 MiB + overhead via caller override, or consume spans. Same size ordinary type needs NextHeader, Unconfirmed(ExactEof) or StreamOptIn; H=64 MiB refuses. Trimmed capacity returns after ownership is released. |
| L3-BUD-008 | Instrument all live credits/capacity and transient workspace. Gate a worker with barriers: reader blocks at budget, no new growth; releasing capacity wakes/completes. A packet that exceeds the entire reclaimed B (including scratch/workspace) errors or reports drain without waiting; exact-fit boundaries include overhead. Cancellation, retained idle slots, multiple readers and blocked merger cannot deadlock. |
| L3-RSS-008 | GNU time wraps prebuilt core-only probe, generation/compilation outside measurement. Tiny bogus-input test retains 32 MiB ceiling; true oversize acceptance gets an explicitly larger justified ceiling. Budget-pressure run bounds touched peak to configured budget plus measured fixed margin. No raising the old capture-path gate to hide a regression. |
| L3-PLAT-008 | Allocation counters plus Linux touched RSS and Windows commit/working-set evidence; 32-bit checked limits. Controls once with touched preallocation and unbudgeted-retention mutants, then discard mutants. Reserved-but-unwritten memory can evade RSS, so request/accounting assertions are mandatory. |

All rows are implementation obligations, including true oversize acceptance;
this docs-only update cannot pass them by measuring 0.1. Existing isolated RSS stays
unchanged. Absolute OS survival cannot be unit-tested or promised.

After this owner-alignment docs PR merges: (2) budget/storage primitives and accounting tests;
(3) admission/seek evidence and EOF tests; (4) incremental whole-packet reader;
(5) pool reuse/trim and metadata inspection; (6) ordered consumer examples and
optional streaming contract; (7) RSS/allocation/platform controls and performance
comparison. Keep each review around 300–600 lines; split rather than omit tests.
No SIMD/async/no_std/publishing work is authorized by this design.
