# Core requirements: implemented 0.1 and approved 0.2

Owner approved 2026-11-22; recovery correction from the coordinator is binding.
Slice 2 supplies primary-header/checksum/basic-length tests and shared `Rtc`.
Slice 4 adds typed completion, optional recovery caps and terminal evidence,
including checked offset-overflow unit coverage. See [slice 2](verification-s02.md),
[slice 3](verification-s03.md) and [slice 4](verification-s04.md) clause evidence.
Slice 5 adds secondary checksum/reserved evidence and trailer ranges; see
[clause evidence](verification-s05.md). Slice 6 adds bounded inventory/presence facts ([evidence](verification-s06.md));
slice 7 adds per-channel sequence observations and the consumer migration contract
([evidence](verification-s07.md)). Slice 8 adds CI-only isolated RSS regression
evidence, exact package inventory and archive verification
([evidence](verification-s08.md)). Allocation-failure
handling is typed but no process-OOM test is claimed. No whole-recording or
whole-standard conformance is asserted.

Slice 9 supplies the base release pipeline and unsigned Publish path dry run
([evidence](verification-s09.md)); this is adoption preparation, not publication.

Sources: **S** = 106-24R1 Chapter 11 §§11.2.1.1–11.2.1.2, as recorded in
[time's reviewed evidence](provenance.md); **O** = owner/coordinator policy
2026-11-22 and 2026-11-25 (whole-packet decisions); **T** = time L2/L3-CLI-104
and CLI-107 clause evidence, not wholesale adoption of their semantic/report requirements. Standard page inspection and
new checksum oracles must be recorded before claiming new wire-rule verification.

## Mode scope and owner approval

Owner decisions **2026-11-25** approve the whole-packet wording below and align
[design-0.2.md](design-0.2.md): built-in H = 128 MiB / B = 256 MiB, independently
overridable; stable exact-EOF admission labelled `Unconfirmed(ExactEof)`; and a
non-default explicit legacy setup allowance for type 0x01 on any channel.
These are approved requirements, **not implemented or verified features**.
Whole-packet slice 2 starts in a separate PR after this docs update merges.

**Owner sign-off, 2026-11-25, answer A:** L2-ADM-008, L2-BUD-008 and
L2-WHOLE-008 are approved in full, including reported opt-in drain/chunk modes
(never default), capacity trimming and cancellable waits. The default PacketBuf
is block-based; multi-block contiguous delivery is explicit coalescing under
raised B. Owner 2026-11-26 Option A updates L2-HOT-008: budgeted fixed hybrid read-ahead,
direct large remainders, no steady-state per-packet allocation, borrowed payloads.

The existing 0.1 bounded-capture mode and its independent-of-declared-size rule
are preserved, including the original mode-specific obligations:

| Clause (0.1 bounded capture) | Preserved wording |
| --- | --- |
| L1-CORE-002 | Stream with caller-bounded memory independent of recording and declared packet size. |
| L2-IO-002 | Use synchronous buffered Read, bounded DATA capture and fixed metadata; never mmap, unsafe, whole-file reads or Packet-Length allocations. |
| L2-CAP-002 | Clamp capture to Data Length, then enforce caller cap; never expose filler/checksum as DATA; caller owns retained packets. |

The corresponding primary rows below carry the approved whole-packet wording;
other clauses continue to apply to both modes where relevant. Historical 0.1
verification retains only its original clause/mode credit. Existing capture RSS
gates and uncapped default recovery are unchanged.

**Verification trace note:** retain the canonical IDs L1-CORE-002, L2-IO-002
and L2-CAP-002, but qualify each verification reference with `@capture-0.1` or
`@whole-0.2` (for example, `L2-IO-002@capture-0.1` versus
`L2-IO-002@whole-0.2`). These are mode-qualified trace keys, not new obligations.
Existing unqualified 0.1 evidence applies only to `@capture-0.1`; whole-packet
evidence must explicitly identify `@whole-0.2`. No test/RSS result transfers
credit across modes merely because the canonical ID is shared.

Default B = 256 MiB fits a maximum 128 MiB setup packet only with block-based
PacketBuf storage, bounded workspace and a normal pool. Coalescing that packet
or using contiguous-only Vec storage requires a caller override with
B >= 2 x 128 MiB + overhead, or the consumer must read the complete spans.
At default B, maximum-setup coalescing returns typed BudgetTooSmall without
waiting, even after all other buffers are returned. Default H/B are unchanged.

**Maintainer-derived design choice, confirmed by the coordinator:**
524,288 bytes (512 KiB) is the smallest block fitting every ordinary
standard-sized packet, including headers/trailer. The size is not itself an
owner requirement; L2-HOT-008 is owner direction.

Choose **524,288-byte (512 KiB) blocks**, including headers/trailer. Packets with
P <= 524,288 start at block offset zero, occupy one block and expose one contiguous
slice without coalescing. Large setup packets use spans (256 blocks at 128 MiB).
Only one fixed, budgeted provisional block may be provisioned for an upcoming
body read; only bytes returned by Read become valid packet data. No speculative
multi-block reservation from P is allowed. Cold initialized-block capacity is
bounded granularity; logical growth still follows actual received bytes.

The default pool uses **lazy warm-up**: construction allocates/pre-touches zero
payload blocks. Its shared idle target after a trim event is **N = 8 blocks = 4 MiB**
across the aggregate budget, not per reader/worker/channel. Demand populates only
needed blocks. Ordinary returns retain the warmed high-water set under B, even
above eight idle blocks. Trim toward eight only after storage from a large
assembly (more than eight blocks) returns or under explicit budget pressure;
pressure may release more. This hysteresis preserves allocation-free ordinary
multi-worker bursts whose in-flight count swings by more than eight. The coordinator
explicitly accepts this change to the approved L3-HOT/DEFAULT/RSS wording in #18,
implementing the owner's zero-steady-state-allocation direction.
Do not eagerly fill the idle baseline. Together
with measured process/control/allocator overhead, this small baseline must fit
under the isolated tiny-input 32 MiB RSS ceiling.

Default B admits at most **511 one-block packets**, including idle and worker-held
blocks, after pool control/lease charges. A maximum setup uses 256 blocks, leaving
at most 255 additional blocks; later reader/workspace charges may reduce this.
A tiny packet still occupies a full 512 KiB block, so deep worker/queue pipelines
encounter budget backpressure early. Size-class blocks are deferred unless later
benchmarks
justify them while preserving one-piece/direct-read behavior; not implemented now.

After a burst, idle storage can stay up to the caller's accounted budget B
(at default B, about **511 blocks / 256 MiB resident**) until a large packet returns
and its owner calls trim_after_return(), or trim_for_pressure() is called.
**Nothing trims automatically.** Retention never exceeds the caller's budget;
callers wanting lower residency choose a lower budget at construction or call
trim. Process/allocator overhead remains outside the accounted block budget.

## L1: responsibilities

| ID | Obligation | Source |
| --- | --- | --- |
| L1-CORE-001 | Find packets and expose checked structural/routing fields without interpreting payloads. | S, O |
| L1-CORE-002 | Stream with memory bounded by caller limits and a shared aggregate storage budget, independent of recording length. Grow packet storage only as bytes are actually received, never by reserving a declared Packet Length. Include all live packet/pool storage, retained capacity and allocation/growth workspace in budget accounting. | O |
| L1-CORE-003 | Recover sync with progress, explicit optional limits and byte-count evidence. | O, T |
| L1-CORE-004 | Distinguish complete input, malformed structure and EOF tails; preserve preceding packets. | O, T |
| L1-CORE-005 | Expose integrity evidence independently of payload/time meaning. | S, O |
| L1-CORE-006 | Supply bounded structural inspection facts for future `irig106 inspect`. | O |
| L1-CORE-007 | Provide one reusable reader, reproducible packaging and reviewed release preparation. | O |

## L2: contracts

Each L2 row names its L1 parent. L3 rows name the specific L2 parent; no
aggregate CLI requirement receives verification credit.

| ID | Parent | Required behavior |
| --- | --- | --- |
| L2-HDR-001 | L1-CORE-001 | Require sync and primary word checksum at every packet offset; preserve channel, type/version, sequence, flags and raw RTC. |
| L2-LEN-001 | L1-CORE-001 | Check 24/36-byte minimum, four-byte alignment, Data Length containment and overflow before trusting offsets or allocating. |
| L2-IO-002 | L1-CORE-002@whole-0.2 | Use safe, std-only synchronous buffered Read and fixed-size I/O scratch. For whole-packet delivery, acquire budget credits and fallibly extend storage only for received bytes in bounded quanta. Never preallocate from declared lengths, use mmap/unsafe, or retain a recording. Deliver a complete packet only after its entire declared frame has arrived. Preserve bounded capture as a separate compatible mode. |
| L2-CAP-002 | L1-CORE-002@whole-0.2 | Preserve the 0.1 DATA-prefix API and its explicit capture cap. The new default whole-packet API applies explicit edition soft thresholds, an absolute caller hard ceiling and aggregate budget; an over-soft packet requires corroboration or explicit stream opt-in. Refusal/drain/chunk disposition must be reported, never represented as an ordinary complete packet. |
| L2-REC-003 | L1-CORE-003 | Default recovery has no search cap and continues to EOF with bounded memory. Optional caller cap yields typed RecoveryLimit; report scanned bytes on every incident. |
| L2-END-004 | L1-CORE-004 | No complete valid packet yields NoCompletePacket (CLI input3); after one, cut-final headers/packets and EOF padding remain tail evidence/findings. |
| L2-ERR-004 | L1-CORE-004 | Propagate real I/O failures; reject impossible checksum-qualified lengths; preserve content-vs-structure distinction and prior events. |
| L2-SEC-005 | L1-CORE-005 | Bit 7 alone selects twelve raw secondary bytes; expose reserved fields and both checksum readings without decoding time or discarding primary RTC. |
| L2-CHK-005 | L1-CORE-005 | Check trailer checksum width/ranges; label packet-data checksum contents NotChecked in 0.1. |
| L2-EDN-005 | L1-CORE-005 | Baseline is 106-24R1; preserve unknown codes and never infer recording edition from data-type version. |
| L2-INV-006 | L1-CORE-006 | Bound channel/row state; count packets, DATA/packet bytes, lengths and raw data types with checked arithmetic and explicit limit failures. |
| L2-SEQ-006 | L1-CORE-006 | Track sequence continuity per channel, including wrap; expose gaps as observations, not proof of packet loss. |
| L2-PRES-006 | L1-CORE-006 | Observe setup/index/event packet presence without validating contents; absence after incomplete scans is inconclusive. |
| L2-PKG-007 | L1-CORE-007 | Use a virtual workspace at crates/irig106-core, edition 2024/MSRV 1.85, std-first, types 0.1 as sole ecosystem dependency. |
| L2-REL-007 | L1-CORE-007 | Independent exact-head review and passing CI precede owner merge. Adopt org Trusted Publishing in slice 9; never publish locally or bypass approval. |

### Whole-packet admission and resource contracts (0.2)

| ID | Parent | Required behavior |
| --- | --- | --- |
| L2-ADM-008 | L1-CORE-002@whole-0.2, L1-CORE-004 | Default H = 134,217,728 bytes, caller-overridable. Apply explicit edition/type S. Above S, admit through NextHeader, stable Unconfirmed(ExactEof), or explicit StreamOptIn; retain authorization evidence. Enforce H/B/target limits in all cases. Setup S = 134,217,728 defaults to channel0/type01 only; non-default legacy_setup_any_channel explicitly extends it to type01 on other channels with evidence, never by inference. |
| L2-BUD-008 | L1-CORE-002@whole-0.2, L1-CORE-004 | Default shared B = 268,435,456 bytes, caller-overridable independently of H. Account for live/idle storage, metadata and growth workspace; grow only after real bytes arrive. A packet unable to fit the entire reclaimed B returns typed BudgetTooSmall or explicit reported drain, never waits. Temporary reclaimable pressure waits cancellably with deadlock-free ownership return; trim rare large capacity toward baseline. |
| L2-WHOLE-008 | L1-CORE-002@whole-0.2, L1-CORE-004 | Deliver one complete packet only after the declared frame arrives. On a seekable stable source, exact EOF admits an oversoft P <= H fitting B with Unconfirmed(ExactEof); recheck source identity/size before admission and after body consumption, returning SourceChanged on observed mutation. Declared end beyond EOF retains truncation-only handling. Drain/chunk modes are explicit, with provisional streaming evidence until completion. |
| L2-HOT-008 | L1-CORE-002@whole-0.2 | Owner 2026-11-26 Option A: every ordinary standard-sized packet (P <= 524,288 including headers/trailer) stays one contiguous borrowed slice. Use one fixed read-ahead buffer (default 65,536 bytes), fallibly reserved at construction and charged to B. Copy already-buffered bytes (including a large-body prefix) into pooled storage; bodies strictly below a configurable cutoff (default 4,096 bytes after the primary header, including secondary/trailer) may refill through this buffer and copy. This is the owner-approved two-copy exception. Larger bodies drain pending bytes, then read the unbuffered remainder directly into final pooled blocks (one kernel-to-user copy on raw sources); select once by whole body size, never by a short read/final fragment. Reuse warmed blocks/descriptors for zero steady-state per-packet allocations; consumers borrow payloads. At most one provisional budgeted block; advance valid bytes only by arrivals; never reserve from declared P. As an implementation detail, header-phase refills after a completed body >= cutoff request only 24 bytes (within min(capacity, 24 + cutoff)); cold readers and readers after a smaller body allow full capacity. Consume pending bytes first. Small-body and recovery refills retain full capacity. This changes refill requests only, not capacity or the copy-once-per-body decision. |

**Refinement note:** L2-HOT-008 refines L2-IO-002@whole-0.2. Owner approval
2026-11-26 permits a bounded second copy for buffered/small bodies and buffered
large prefixes; the unbuffered large remainder stays direct. The fixed buffer is
chosen by caller policy, never P, and included in B. The approved L2-IO-002 wording
and the 0.1 capture-mode contract stay unchanged. See the hybrid design/evidence.

## L3: verification obligations

Coverage is clause-scoped in the slice evidence; planned probes are not passes.

| ID → L2 parent | Oracle and required probes |
| --- | --- |
| L3-HDR-001 → L2-HDR-001 | Hand-packed standard fields; known primary sum; checksum corruption at expected offset; arbitrary flag bytes. |
| L3-LEN-001 → L2-LEN-001 | Minimum/alignment/overflow boundaries; recompute checksum after structural mutations so checksum rejection cannot mask the test. |
| L3-IO-002 → L2-IO-002@capture-0.1 | Preserved 0.1 capture mode: actual small file, three-byte short reads, Interrupted and real errors; CI-only isolated RSS on generated input in slice 8. |
| L3-CAP-002 → L2-CAP-002@capture-0.1 | Preserved 0.1 capture mode: zero/exact/over cap, DATA clamp, nonzero filler, partial capture, zero buffer, allocation failure handling. |
| L3-REC-003 → L2-REC-003 | Cross-buffer/false sync; exact scanned counts; valid candidate at cap accepted, one byte beyond rejected; >1 MiB padding succeeds by default. |
| L3-END-004 → L2-END-004 | Empty/no-sync/lone truncated input; complete packet then every header/body cut and zero/FF padding; preserve preceding results. |
| L3-ERR-004 → L2-ERR-004 | Invalid structure fatal; short/invalid semantic body still delivered; terminal behavior and I/O errors during recovery retain evidence. |
| L3-SEC-005 → L2-SEC-005 | Bits 7/2 independence; twelve-byte truncations; byte-only/word-only/both/neither checksum vectors; primary RTC retained. |
| L3-CHK-005 → L2-CHK-005 | Every trailer width and insufficient space; checksum contents remain explicitly NotChecked, never reported valid. |
| L3-EDN-005 → L2-EDN-005 | Unknown type/version preserved; distinguish setup-record and data-type version mappings; no implicit edition. |
| L3-INV-006 → L2-INV-006 | Independent totals; interleaved types; row/channel cap edges and arithmetic overflow; no growing packet/diagnostic vectors. |
| L3-SEQ-006 → L2-SEQ-006 | 255→0, duplicate, discontinuity, interleaved channels and multiple types on one channel. |
| L3-PRES-006 → L2-PRES-006 | Setup/index/event observations; absent complete vs incomplete scan; no semantic validity claim. |
| L3-PKG-007 → L2-PKG-007 | Metadata, registry-only dependency/default-feature boundary and package file checks; CI builds archived source. Header integration tests use the shared Rtc directly. |
| L3-REL-007 → L2-REL-007 | Exact-head CI/review ledgers and unsigned publishing-path dry run in slice 9. Cryptographic/registry mutation paths have mocked controls; real release remains separately authorized. |

### Whole-packet L3 obligations (0.2; not yet executed)

These obligations include the owner-required default, exact-EOF/mutation and
legacy-allowance tests. They belong to the corresponding implementation slices;
this docs-only PR adds no runtime API or passing-test claims. Full behavior and
source references are in [the design](design-0.2.md).

| ID → L2 parent | Oracle and required probes |
| --- | --- |
| L3-BLOCK-008 → L2-HOT-008, L2-WHOLE-008 | Maintainer-derived, coordinator-confirmed default block size is 524,288 bytes including all headers/trailer. Independent frames at small P and exactly P = 524,288, with/without secondary headers and each trailer width, occupy one block and expose their exact full bytes as one contiguous borrowed slice. An admitted P = 524,292 uses two blocks; maximum setup uses 256. No automatic coalescing. |
| L3-HOT-008 → L2-HOT-008, L2-IO-002@whole-0.2, L2-BUD-008 | Instrument Read destinations/call sizes for the single budgeted read-ahead buffer, fully buffered bodies and strict cutoff boundaries, and direct large remainders including a tiny final block. Verify exact bytes/offsets with pending headers, recovery, short/Interrupted reads and seek probes; no large-body refill caused by a short read/final fragment. Constructor buffer accounting/failure and prefetch-time SourceChanged are typed. After pool/descriptor warm-up, allocation counting over mixed ordinary sizes is zero (oracles outside measurement). Four-worker bursts retain 16 live-to-idle blocks and allocate zero after warm-up; large-return/pressure trim remains separate. Borrow lifetimes prevent reuse while views live. Cold EOF has at most one budgeted provisional block plus fixed read-ahead, never a P-sized request. Assert capped header requests after large bodies and full requests after small bodies, including straddling/short headers; recovery and small-body refills stay full. Exercise capacity/cutoff/short-read/recovery combinations byte- and offset-exact. Both whole constructors reject 23-byte capacity with typed InvalidRequest and unchanged pool accounting, accept 24; directly test the private factory minimum. |
| L3-DEFAULT-008 → L2-ADM-008, L2-BUD-008 | Whole-packet new()/Default H = 134,217,728, B = 268,435,456 and shared idle N = 8 (4 MiB). Construction allocates/pre-touches zero payload blocks; demand-only warm-up does not fill to N. After large-return/pressure trim, at most eight idle blocks remain across readers/slots; ordinary returns can retain a larger warmed working set within B, and pressure may retain fewer. Independent H/B overrides preserve the other setting; defaults do not allocate H/B or alter 0.1 ReaderLimits. Feasibility still includes workspace/pool. |
| L3-EOF-008 → L2-ADM-008, L2-WHOLE-008 | Stable exact EOF admitted with precisely Unconfirmed(ExactEof), whole bytes and no next-header/stream-opt-in requirement; one byte short takes truncation-only path; identity replacement or size mutation before admission/after body returns SourceChanged with no packet. Missing stability evidence cannot authorize ExactEof. |
| L3-LEGACY-008 → L2-ADM-008 | Default off: channel0/type01 selects setup S, nonzero/type01 retains ordinary S. Explicit opt-in selects setup S on nonzero/type01 and records LegacySetupAnyChannel evidence. Other types, arbitrary Data Type Versions and guessed editions never enable it. H/B overrides still apply. |
| L3-GROW-008 → L2-IO-002@whole-0.2 | Authorized huge aligned P on tiny short-read input: allocation requests and touched RSS follow received bytes + fixed quantum; exact tail/preceding results. Mutant preallocating/zeroing P must fail. Include a hard-refusal case separately so it cannot mask the growth test. |
| L3-ADM-008 → L2-ADM-008 | Real oversoft packet below/exactly/above H; valid/invalid/short next header; false checksum-valid payload header; seek restoration across buffering and recovery; final EOF and nonseek opt-in/refusal. Whole bytes and authorization evidence checked independently. |
| L3-SETUP-008 → L2-ADM-008, L2-BUD-008 | At built-in H = 128 MiB and B = 256 MiB, a complete 128 MiB setup packet (P = H) is accepted with the default block-based representation; coalescing it under default B returns typed BudgetTooSmall without waiting, including with all other buffers returned. Contiguous coalescing/Vec requires B >= 2 x 128 MiB + overhead via caller override, or consume spans. Same size ordinary type needs NextHeader, Unconfirmed(ExactEof) or StreamOptIn; H=64 MiB refuses. Trimmed capacity returns after ownership is released. |
| L3-BUD-008 → L2-BUD-008 | Instrument all live credits/capacity and transient workspace. Gate a worker with bounded handshakes: explicit-deadline checkout waits at budget, no new growth; releasing capacity wakes/completes. A packet that exceeds the entire reclaimed B (including scratch/workspace) errors or reports drain without waiting; exact-fit boundaries include overhead. BudgetUnavailable requires provable absence of outstanding returners, never thread inference; explicit try-acquire returns WouldBlock on contention. Absolute-deadline waits test expiry with retained same-thread resources, spurious wakeups and reuse after timeout. Cancel with idle blocks must clear storage/credits. Test completion channels/joins are bounded independently of operation deadlines. Cancellation, retained idle slots, multiple readers and blocked merger cannot deadlock. |
| L3-RSS-008 → L2-IO-002@whole-0.2, L2-BUD-008 | GNU time wraps prebuilt core-only probe, generation/compilation outside measurement. Cold-construction and lazily warmed tiny bogus-input tests retain the 32 MiB ceiling: assert zero construction payload blocks and idle <= 8 after a large-return/pressure trim event, and measure process baseline plus at most 4 MiB idle payload, control storage and allocator margin; true oversize acceptance gets an explicitly larger justified ceiling. Budget-pressure run bounds touched peak to configured budget plus measured fixed margin. No raising the old capture-path gate to hide a regression. |
| L3-PLAT-008 → L2-IO-002@whole-0.2, L2-BUD-008 | Allocation counters plus Linux touched RSS and Windows commit/working-set evidence; 32-bit checked limits. Controls once with touched preallocation and unbudgeted-retention mutants, then discard mutants. Reserved-but-unwritten memory can evade RSS, so request/accounting assertions are mandatory. |

## Nine review slices (0.1 history)

Target 300–600 changed lines per PR, including tests and evidence. Freeze after
reporting each PR/head; independent reviewer reviews that exact head before owner
merge. Tests first; record compile failures separately from runtime failures.

1. Requirements, API contract, provenance, virtual workspace and minimal CI.
2. Primary header/checksum parser, types 0.1 dependency and independent fixtures.
3. Promote bounded reader and seed regressions without semantic/report code.
4. Typed completion/errors, optional recovery cap and adversarial tail tests.
5. Secondary integrity evidence and trailer-range checks.
6. Bounded inspection counts, raw data-type/presence facts.
7. Sequence continuity, public consumer examples and migration contract.
8. CI-only isolated RSS, package checks and documentation reconciliation.
9. One org-standard Trusted Publishing adoption PR with passing unsigned PR dry
   run. No publication/tag or environment-policy change is authorized here.

Tracked post-0.1: no_std/alloc support; streaming packet-data checksum validation
for all supported widths. SIMD, async, mmap, parallel scanning and throughput
claims need separate measured justification. Time remains paused; on resume it
adopts published core and removes its duplicate reader. Core does not depend on
TMATS 0.1; its parallel work must not block structural traversal.

Further conformance work observed during slice 2 standard inspection: packet
maximums/nonempty DATA (§11.2.1 c/e), reserved channel use and per-channel version
constancy are not checked by the primary primitive. Allocate explicit later
validator requirements before claiming full 106-24R1 conformance.
