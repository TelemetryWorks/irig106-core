# Core 0.1 requirements and delivery plan

Owner approved 2026-11-22; recovery correction from the coordinator is binding.
Slice 2 supplies primary-header/checksum/basic-length tests and shared `Rtc`.
Slice 4 adds typed completion, optional recovery caps and terminal evidence,
including checked offset-overflow unit coverage. See [slice 2](verification-s02.md),
[slice 3](verification-s03.md) and [slice 4](verification-s04.md) clause evidence.
Slice 5 adds secondary checksum/reserved evidence and trailer ranges; see
[clause evidence](verification-s05.md). Slice 6 adds bounded inventory/presence facts ([evidence](verification-s06.md));
slice 7 adds per-channel sequence observations and the consumer migration contract
([evidence](verification-s07.md)). Allocation-failure
handling is typed but no process-OOM test is claimed. No whole-recording or
whole-standard conformance is asserted.

Sources: **S** = 106-24R1 Chapter 11 §§11.2.1.1–11.2.1.2, as recorded in
[time's reviewed evidence](provenance.md); **O** = owner/coordinator policy
2026-11-22; **T** = time L2/L3-CLI-104 and CLI-107 clause evidence, not wholesale
adoption of their semantic/report requirements. Standard page inspection and
new checksum oracles must be recorded before claiming new wire-rule verification.

## L1: responsibilities

| ID | Obligation | Source |
| --- | --- | --- |
| L1-CORE-001 | Find packets and expose checked structural/routing fields without interpreting payloads. | S, O |
| L1-CORE-002 | Stream with caller-bounded memory independent of recording and declared packet size. | O, T |
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
| L2-IO-002 | L1-CORE-002 | Use synchronous buffered Read, bounded DATA capture and fixed metadata; never mmap, unsafe, whole-file reads or Packet-Length allocations. |
| L2-CAP-002 | L1-CORE-002 | Clamp capture to Data Length, then enforce caller cap; never expose filler/checksum as DATA; caller owns retained packets. |
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

## L3: verification obligations

Coverage is clause-scoped in the slice evidence; planned probes are not passes.

| ID → L2 parent | Oracle and required probes |
| --- | --- |
| L3-HDR-001 → L2-HDR-001 | Hand-packed standard fields; known primary sum; checksum corruption at expected offset; arbitrary flag bytes. |
| L3-LEN-001 → L2-LEN-001 | Minimum/alignment/overflow boundaries; recompute checksum after structural mutations so checksum rejection cannot mask the test. |
| L3-IO-002 → L2-IO-002 | Actual small file, three-byte short reads, Interrupted and real errors; CI-only isolated RSS on generated input in slice 8. |
| L3-CAP-002 → L2-CAP-002 | Zero/exact/over cap, DATA clamp, nonzero filler, partial capture, zero buffer, allocation failure handling. |
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
| L3-REL-007 → L2-REL-007 | Future: exact-head CI/review ledger and unsigned publishing-path dry run; release remains separately authorized. |

## Nine review slices

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
