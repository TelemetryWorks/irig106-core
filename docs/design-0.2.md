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

Verification uses mode-qualified trace keys for the shared IDs:
L1-CORE-002, L2-IO-002 and L2-CAP-002 each have `@capture-0.1` and
`@whole-0.2` scopes. Existing unqualified 0.1 evidence means capture scope; it
never verifies whole-packet behavior. See the requirements trace note.

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

**Owner sign-off, 2026-11-25 (answer A to #17 item 7):** L2-ADM-008,
L2-BUD-008 and L2-WHOLE-008 are approved **in full**, including explicit reported
opt-in drain/chunk modes, capacity trimming and cancellable waits. Block-based
PacketBuf is the approved default. Multi-block contiguous delivery requires
explicit coalescing under a raised B; ordinary one-block packets are already
contiguous. The direct-read/allocation/borrowing contract below is also an owner
requirement, not a measured 0.2 performance claim.

Planned API boundary: the shared PacketReader framing engine gains an explicit
whole-packet mode configured with a non_exhaustive resource policy and shared
budget handle. It returns PacketBuf ownership plus header/ranges/offset, soft
violation and authorization evidence; borrowed views expose complete spans and
DATA ranges. Tail/error/NoCompletePacket rules remain typed and terminal. Builder
constructors select SeekChecked or StreamOptIn; existing bounded-prefix callers
keep their contract. Exact method names are deferred to the implementation review.

## Limits and admission (L2-ADM-008)

Let P be total Packet Length, S the edition/type soft threshold, H the caller's
absolute hard ceiling, B aggregate budget and K bounded direct-read quantum.
All include explicit units; P includes headers/trailer, not just DATA. Built-in defaults are
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
an exclusive source cursor, and reads a fixed 24-byte candidate. Require sync,
primary checksum and the same structural header/range checks as normal framing.
Return evidence NextHeader(offset), not a claim the following body is valid.
Restore the **logical** body position on success/failure, invalidate stale
header/probe read-ahead, and do not charge probe bytes to stream
position/recovery counters. Whole-packet mode seeks through the exclusive raw
source; it must not add an input BufReader that stages body bytes. The existing
0.1 buffered-capture path is unchanged. Restore/seek/read errors are typed
terminal errors; never use File::try_clone assuming an independent file cursor. Interrupted is
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
**Default block size is 524,288 bytes (512 KiB), including packet headers and
trailer**, independent of P. Every packet with P <= 524,288 starts at block offset
zero and occupies exactly one block; its valid frame is one contiguous slice.
Headers, secondary bytes, DATA, filler and checksum all fit that same block.
Larger packets use multiple blocks; a maximum 128 MiB setup occupies 256 blocks.
There is no automatic contiguous-only representation or multi-block coalescing.
This block size is the maintainer's derivation of the owner's one-piece requirement,
confirmed by the owner during #17 review.

**Pool warm-up is lazy:** construction allocates or pre-touches **zero payload
blocks**. The default idle retention baseline is **N = 8 blocks**, or
8 x 512 KiB = **4 MiB**, shared across the entire aggregate budget, not per
reader, worker or channel. Acquire/initialize blocks only on actual demand under
the provisional-block rule below; never populate eight blocks merely to reach
the baseline. After returned storage is trimmed, retain at most eight idle
blocks; pressure may release more, down to zero. Live worker-owned blocks are
not idle and remain charged until returned. Keep metadata/control overhead
bounded and measured so the 4 MiB idle payload footprint plus process baseline
fits comfortably below the isolated 32 MiB tiny-input RSS ceiling.

At default B, block bytes alone allow **at most 512 one-block packets** live;
metadata, control scratch and idle capacity reduce the usable count. A maximum
setup holds 256 blocks, leaving at most about 256 additional ordinary-packet
slots before that overhead. Even a tiny packet occupies a full 512 KiB block,
so worker/queue depth can reach backpressure while valid payload bytes remain
small. These are upper bounds, not promises that all 512 slots are available.
Size-class blocks are a possible later optimization only if benchmarks justify
it and the one-piece/direct-read requirements remain satisfied; not this slice.

### Direct-read hot path (L2-HOT-008)

Read each body directly from the unbuffered source into its final pooled block.
For raw file/pipe input this targets one kernel-to-user payload copy: never read
normal body bytes into input BufReader/scratch and then append/copy them into
PacketBuf. The small fixed primary header may be copied into the first block;
header/probe/recovery and explicit drain scratch are separate from the normal
body path. Bound each read by K, the block's remaining space and remaining frame
bytes, so it cannot prefetch another packet's body. Probes read only header bytes.
Secondary/trailer reads also land at their final frame offsets.

Acquire an existing initialized pool block (or fallibly provision one fixed block
on a cold/exceptional path), charge its capacity before reading, and read into
its final writable slice. Safe Read needs initialized destination storage; fixed
block initialization is a bounded cold-path cost, never zeroing/reserving P.
Only the returned n>0 bytes advance valid length. Acquire another block only
when the current one is filled and more frame bytes are needed. At most one
provisional block is available to the next read; EOF/error releases any unused
block, and Interrupted retries without allocating another. This fixed-granularity
allowance, including the initial block before the first body byte, is charged to
B; a bogus large P cannot cause multiple blocks to be allocated speculatively.
Logical packet growth follows actual arrivals, with at most one block of slack.

Reuse blocks **and packet/segment descriptors**: after the bounded pool and
metadata are warmed, the successful hot path has **zero per-packet allocations**.
Consumers borrow DATA/frame slices or spans from the packet owner; do not clone
payload vectors to hand them off. Borrow lifetimes prevent reuse while a view
is live; moving packet ownership to workers does not copy its payload.
Trimming a rare large record can make later growth a cold path again; do not
claim allocation-free first use or arbitrary sustained large-record churn.

L2-HOT-008 refines L2-IO-002@whole-0.2 without changing its approved wording.
The source adapter must actually support direct reads; an externally supplied
Read may hide buffering/copies. No one-copy claim is made for such an adapter or
UDP reassembly outside core. In L2-IO-002's whole-packet scope, buffering means
final pooled storage and fixed scratch means framing/control/drain scratch,
not a second body staging buffer. This does not alter 0.1 bounded capture.
Small direct file reads may increase syscall overhead relative to buffered 0.1;
benchmark calls, small-packet throughput and large-body bandwidth separately.
The copy/allocation contract is mandatory, not a guaranteed speedup.

A naive Vec::try_reserve(delta) is insufficient: growth may be geometric and a
moving reallocation can transiently retain old and new allocations. Even
[try_reserve_exact](https://doc.rust-lang.org/std/vec/struct.Vec.html#method.try_reserve_exact)
permits allocator excess. Small exact contiguous increments can also repeatedly
copy a large prefix. **Default implementation representation:** opaque PacketBuf
with reusable fixed-size blocks, provisioned under the direct-read rules above, exposing
a complete packet view only at completion. Metadata growth is budgeted too. One
packet up to 524,288 bytes occupies one block; larger packets occupy several.
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
No contiguous-only reader mode is introduced: this Vec calculation is a cost
comparison, not an alternative default or automatic fallback. Multi-block
contiguous delivery is only explicit coalescing under raised B, outside the
direct-read hot path; there is no unsafe/unaccounted realloc shortcut.

Credits cover live storage capacity (not just length), idle slots, metadata and
transient replacement/coalescing workspace. Charge bounded allocation quanta
before allocating; account for received bytes, the one provisional initialized
block and fixed control scratch. Do not initialize a declared-length region.
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

On return of a rare large packet, drop excess idle blocks beyond the shared
N = 8 baseline and return credits; pressure also trims idle slots. The baseline
is a retention ceiling, not an eager allocation target or a guaranteed reserve. Free/drop is preferred to an infallible
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
| Approximate 4 GiB claim | u32's largest aligned P is 0xFFFFFFFC, not 2^32. This is rejected under default H = 128 MiB or any caller override below 0xFFFFFFFC, not reported as a tail. The receive-growth regression must explicitly authorize H/B/stream policy large enough, or exercise known-EOF truncation; no claim that rules A/B both override one another. |
| Legitimate oversize | S < P <= H plus NextHeader, Unconfirmed(ExactEof) or StreamOptIn, sufficient budget/workspace: deliver whole frame with soft-violation/authorization evidence. No cap inferred from actual file's type-version bytes. |
| 128 MiB setup | Channel0/type01 under this edition is within S. Accept without oversize corroboration only if H/B/target permit; grow as bytes arrive, then return/drop excess capacity. Nonzero-channel type01 needs explicit legacy_setup_any_channel to select this allowance, recorded as evidence; other types never inherit it. |
| Exact EOF / one byte short | Stable end == EOF and P <= H/fits B: admit, labelled Unconfirmed(ExactEof). End one byte beyond EOF: truncation-only path, no packet. Observed size/identity mutation: SourceChanged before delivery. |
| False corroboration | Does not bypass any resource bound or prove packet meaning; retain evidence and revalidate later. No silent recovery promotion of arbitrary data into trusted content. |
| Pipes/live streams | Oversoft packets require explicit StreamOptIn; no probe, seek emulation or whole-stream spool. Blocking read is supported. WouldBlock/timeout after partial input is terminal under 0.1 semantics; resumable nonblocking input needs its own state-machine contract. UDP requires bounded reassembly/loss policy outside core. |
| Recovery/padding | Sync/checksum failures retain scalar progress, byte counts and optional caller search cap; default has none. Checksum-qualified impossible lengths stay fatal. Policy refusals are not invalid sync and do not trigger unlimited rescanning. Trailing padding remains a finding with prior analysis. |
| Pool/backpressure | Budget charges workers and idle buffers; return/shrink releases credits. Inability to fit even with the entire budget reclaimed errors or explicitly drains immediately, never waits. Temporary pressure waits cancellably, never bypasses budget or deadlocks on pending ordered output. |
| Interrupted/short reads | Retry Interrupted without resetting received bytes or budget credit; positive short reads append only n bytes; Ok(0) means EOF. PermissionDenied and other errors remain typed, terminal, and distinct from truncation. |
| 32-bit | Keep offsets/length calculations checked in u64; checked conversions and isize/address-space limits before allocation. H/B/block counts must be representable. Segmentation does not justify unchecked usize arithmetic. Refuse unsupported configuration/packet with typed TargetLimit; no wrap or cast-based allocation. |
| Linux overcommit/OOM | Fallible allocation catches reported allocator failure, not process kills on later page faults. Touch received bytes plus bounded fixed-block initialization and control/copy overhead, never a declared-length region. A genuinely huge arriving stream can still hit a badly configured host/cgroup limit; H/B require headroom. Never promise immunity from OOM-killer/global pressure. |
| Windows commit charge | Heap allocation may consume commit before pages are resident. Incremental actual-byte allocation limits that request, but working-set RSS alone misses commit pressure. Measure private committed bytes too; failure may arrive earlier than on Linux. |
| Edition maximum | Explicit edition policy supplies S; unknown editions/types need a caller policy or conservative ordinary S, with the raw uncertainty reported. A standard size violation is an admission finding, not an inferred edition or permission to allocate. |

Platform sources: [Linux overcommit accounting](https://www.kernel.org/doc/html/latest/mm/overcommit-accounting.html)
and [Windows page states](https://learn.microsoft.com/en-us/windows/win32/memory/page-state).
No settings, overcommit policy or process limits are changed by this design.

## Required L3 controls and subsequent slices

| Control | Oracle / falsifying regression |
| --- | --- |
| L3-BLOCK-008 | Default block size is 524,288 bytes including all headers/trailer. Independent frames at small P and exactly P = 524,288, with/without secondary headers and each trailer width, occupy one block and expose their exact full bytes as one contiguous borrowed slice. An admitted P = 524,292 uses two blocks; maximum setup uses 256. No automatic coalescing. |
| L3-HOT-008 | Instrument Read destinations to prove every body read targets the final pool block at its frame offset, never scratch/BufReader; short/Interrupted reads preserve this and never overread the frame. After pool/descriptor warm-up, allocation counting over many mixed ordinary packet sizes is zero (oracle storage preallocated). Borrow/lifetime checks reject owner reuse while views live. Cold EOF has at most one budgeted provisional block and no speculative P-sized request. |
| L3-DEFAULT-008 | Whole-packet new()/Default H = 134,217,728, B = 268,435,456 and shared idle N = 8 (4 MiB). Construction allocates/pre-touches zero payload blocks; demand-only warm-up does not fill to N. After returns/trim, at most eight idle blocks remain across readers/slots; pressure may retain fewer. Independent H/B overrides preserve the other setting; defaults do not allocate H/B or alter 0.1 ReaderLimits. Feasibility still includes workspace/pool. |
| L3-EOF-008 | Stable exact EOF admitted with precisely Unconfirmed(ExactEof), whole bytes and no next-header/stream-opt-in requirement; one byte short takes truncation-only path; identity replacement or size mutation before admission/after body returns SourceChanged with no packet. Missing stability evidence cannot authorize ExactEof. |
| L3-LEGACY-008 | Default off: channel0/type01 selects setup S, nonzero/type01 retains ordinary S. Explicit opt-in selects setup S on nonzero/type01 and records LegacySetupAnyChannel evidence. Other types, arbitrary Data Type Versions and guessed editions never enable it. H/B overrides still apply. |
| L3-GROW-008 | Authorized huge aligned P on tiny short-read input: allocation requests and touched RSS follow received bytes + fixed quantum; exact tail/preceding results. Mutant preallocating/zeroing P must fail. Include a hard-refusal case separately so it cannot mask the growth test. |
| L3-ADM-008 | Real oversoft packet below/exactly/above H; valid/invalid/short next header; false checksum-valid payload header; seek restoration across buffering and recovery; final EOF and nonseek opt-in/refusal. Whole bytes and authorization evidence checked independently. |
| L3-SETUP-008 | At built-in H = 128 MiB and B = 256 MiB, a complete 128 MiB setup packet (P = H) is accepted with the default block-based representation; coalescing it under default B returns typed BudgetTooSmall without waiting, including with all other buffers returned. Contiguous coalescing/Vec requires B >= 2 x 128 MiB + overhead via caller override, or consume spans. Same size ordinary type needs NextHeader, Unconfirmed(ExactEof) or StreamOptIn; H=64 MiB refuses. Trimmed capacity returns after ownership is released. |
| L3-BUD-008 | Instrument all live credits/capacity and transient workspace. Gate a worker with barriers: reader blocks at budget, no new growth; releasing capacity wakes/completes. A packet that exceeds the entire reclaimed B (including scratch/workspace) errors or reports drain without waiting; exact-fit boundaries include overhead. Cancellation, retained idle slots, multiple readers and blocked merger cannot deadlock. |
| L3-RSS-008 | GNU time wraps prebuilt core-only probe, generation/compilation outside measurement. Cold-construction and lazily warmed tiny bogus-input tests retain the 32 MiB ceiling: assert zero construction payload blocks and post-trim idle <= 8, and measure process baseline plus at most 4 MiB idle payload, control storage and allocator margin; true oversize acceptance gets an explicitly larger justified ceiling. Budget-pressure run bounds touched peak to configured budget plus measured fixed margin. No raising the old capture-path gate to hide a regression. |
| L3-PLAT-008 | Allocation counters plus Linux touched RSS and Windows commit/working-set evidence; 32-bit checked limits. Controls once with touched preallocation and unbudgeted-retention mutants, then discard mutants. Reserved-but-unwritten memory can evade RSS, so request/accounting assertions are mandatory. |

All rows are implementation obligations, including true oversize acceptance;
this docs-only update cannot pass them by measuring 0.1. Existing isolated RSS stays
unchanged. Absolute OS survival cannot be unit-tested or promised.

After this owner-alignment docs PR merges: (2) budget/storage primitives, 512 KiB blocks and accounting/default tests;
(3) admission/seek evidence and EOF tests; (4) incremental whole-packet reader;
(5) pool reuse/trim and metadata inspection; (6) ordered consumer examples and
optional streaming contract; (7) RSS/allocation/platform controls and performance
comparison. Keep each review around 300–600 lines; split rather than omit tests.
No SIMD/async/no_std/publishing work is authorized by this design.
