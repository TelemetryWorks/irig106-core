# Core 0.2.0 coverage audit and release-candidate boundary

Audited 2026-11-28 from main 5d92706 (reviewed #31 bcd251b merged), plus this
release-preparation PR. This is a source/test/evidence trace, **not** a measured
line/branch coverage percentage or blanket IRIG-conformance certificate. Candidate
CI/CodeQL/publish-dry-run IDs, archived README inspection and package digest are
recorded in the PR at its exact head. Prior feature evidence is labelled below;
prior runs do not count as new-version execution. No tag/publication is authorized.

## Modes, defaults and dependency closure

| Path | Storage / outcome | Checksum / inventory |
| --- | --- | --- |
| Retained PacketReader capture | Caller DATA cap, fixed framing metadata/read buffer; independent of declared P. No complete-frame payload retention required. | Absent/NotChecked; complete Event can be observed once. |
| WholePacketReader + PacketBuf | Received bytes in 512 KiB pooled blocks; one bounded provisional block; complete frame only. | ON by default, opt-out NotChecked, mismatch evidence; complete PacketInfo observed once. |
| coalesce_until | Explicit old blocks + independent complete-frame copy/metadata charged simultaneously. Never automatic; check contiguous() first. | Preserves original info; original/copy are one observation. |
| drain_next | Explicit omission through fixed 4 KiB scratch, even above whole H/B/target storage limits. | Absent/NotChecked; never PacketInfo/inventory credit. |
| next_chunks | Explicit fixed 4 KiB callback borrows; Begin/Chunk provisional, Abort invalidates, returned End commits. | Incremental checksum ON/opt-out; End remains distinct from complete PacketInfo/inventory. |

Whole defaults: H = 134,217,728 bytes, B = 268,435,456 bytes, read-ahead = 65,536,
strict small-body cutoff = 4,096 after primary; no default recovery cap, legacy
setup extension or non-seekable oversoft opt-in. Ordinary standard-sized packets
(P <= 524,288) are contiguous; max setup uses 256 blocks. Default B admits at most
511 blocks before additional workspace charges, not 512. Coalescing max setup
under default B returns BudgetTooSmall; consume spans or explicitly raise B.

Pools warm lazily and ordinary returns retain the warmed high-water set within B;
large-return/explicit pressure trim targets eight idle blocks, not every return.
B accounts core storage/control, not allocator-private overhead, source adapters,
thread stacks, inventory maps or caller queues/context/output. Those need bounds.
Checksums are arithmetic integrity evidence, not authenticity or payload meaning.

`Cargo.toml`, Cargo.lock and `src/lib.rs` establish the dependency closure:
**irig106-types ^0.1**, defaults off, locked to published **0.1.0** (checksum
2f5ddd5baf4fbc4fa5579bd739561274bc5fc9cc36ba50d610b0442cc0635420), is the only
dependency. Core uses/re-exports Rtc; no types 0.2 symbol/version is required.
**Types 0.2 need not be published first.** Packaged-source verification resolves
this registry dependency; core is the only member of its release set.

## L1 and L2 trace

C below means code plus scoped executable oracles exist; it does not mean all
possible inputs/failures are verified. P calls out residual verification below.
Canonical 0.1 capture IDs retain their original scope; whole-mode credit is
qualified @whole-0.2. The full wording remains in [requirements](requirements.md).

| L1 | Scope and implementing L2 rows |
| --- | --- |
| L1-CORE-001 | Header/framing without payload semantics: L2-HDR-001, L2-LEN-001. |
| L1-CORE-002 | @capture-0.1 and @whole-0.2 have different storage rules: L2-IO-002, L2-CAP-002, L2-ADM-008, L2-BUD-008, L2-WHOLE-008, L2-HOT-008, L2-COA-010. |
| L1-CORE-003 | Linear bounded recovery / optional cap: L2-REC-003. |
| L1-CORE-004 | Completion, structural/I/O failure and explicit partial-mode reports: L2-END-004, L2-ERR-004 plus whole/budget/coalescing contracts. |
| L1-CORE-005 | Raw secondary/edition and checksum evidence: L2-SEC-005, L2-CHK-005, L2-EDN-005, L2-CHK-012. |
| L1-CORE-006 | Entry-bounded inventory, continuity, presence and scoped admission counters: L2-INV-006, L2-SEQ-006, L2-PRES-006, L2-INV-009. |
| L1-CORE-007 | One workspace/library, registry types and reviewed release controls: L2-PKG-007, L2-REL-007. |

| L2 | Implementation → L3 / evidence | Scope |
| --- | --- | --- |
| L2-HDR-001 | src/header.rs → L3-HDR-001; [slice 2](verification-s02.md). | C; Header alone is not a packet. |
| L2-LEN-001 | header.rs + integrity.rs ranges → L3-LEN-001. | C; standard maxima/nonempty DATA are not primary-primitive checks. |
| L2-IO-002 / L2-CAP-002 | reader.rs @capture-0.1; whole.rs + pool.rs @whole-0.2 → L3-IO/CAP-002, BLOCK/HOT/GROW-008, DRAIN-013, CHUNK-014. | C/P; fixed quantum and external-memory limits below. |
| L2-REC-003 | Shared reader.rs framing → L3-REC-003. | C; no default search cap, EOF padding preserved. |
| L2-END-004 / L2-ERR-004 | error.rs + shared framing + mode wrappers → L3-END/ERR-004, DRAIN-013, CHUNK-014. | C/P; size refusals alone positioned; budget-race oracle missing. |
| L2-SEC-005 | integrity.rs + capture/whole/chunks → L3-SEC-005, CHUNK-014. | C; RDG-008 byte/word evidence, no time conversion. |
| L2-CHK-005 | PacketRanges + capture → L3-CHK-005. | C; capture contents remain NotChecked/Absent. |
| L2-EDN-005 | Header raw codes + admission.rs EditionPolicy → L3-EDN-005, LEGACY-008. | C; no inferred edition. |
| L2-INV-006 / L2-SEQ-006 / L2-PRES-006 | inspection.rs + sequence.rs → L3-INV/SEQ/PRES-006. | C; bounded entries, not fallible BTree allocation or semantic facts. |
| L2-INV-009 | inspection.rs AdmissionCounts → L3-INV-009. | C; complete whole observations only. |
| L2-CHK-012 | integrity.rs accumulator + whole/chunks + inspection.rs → L3-CHK-012. | C; default ON accepted Q7, <10% benchmark target not met. |
| L2-ADM-008 | admission.rs + whole.rs/chunks.rs → L3-ADM/DEFAULT/EOF/LEGACY/SETUP-008. | C/P; corroboration is structural; false-positive fixture gap below. |
| L2-BUD-008 | pool.rs + whole.rs workspaces → L3-BUD/DEFAULT/RSS/PLAT-008. | C/P; allocator/OS limits and race branch below. |
| L2-WHOLE-008 | whole.rs complete path, positioned refusal, drain.rs/chunks.rs explicit modes → L3-EOF/SETUP-008, DRAIN-013, CHUNK-014. | C; no implicit fallback or partial PacketInfo. |
| L2-HOT-008 | hybrid/adaptive reader.rs + whole.rs, borrowed PacketBuf → L3-BLOCK/HOT-008. | C; small/buffered second-copy exception, scoped warmed allocations. |
| L2-COA-010 | whole.rs CoalescedPacket + pool.rs returnable credits → coalescing suite, L3-SETUP/BUD-008. | C/P; free-before-credit ordering inspected, not allocator-hook traced. |
| L2-PKG-007 | Virtual manifests, tests/workspace checks, archive verification → L3-PKG-007. | C; Rust 2024/MSRV 1.85, types 0.1 only. |
| L2-REL-007 | pipeline/workflow, independent review and owner gate → L3-REL-007. | C for dry controls; real 0.2 signatures/registry/docs.rs and private settings unverified. |

## Complete L3 inventory

Paths below are relative to crates/irig106-core unless scripts/.github is stated.
Test names select independent wire/behavior oracles; full suites run in CI.
Historical [provenance](provenance.md) preserves the promoted time-reader lineage.
External L3-CLI-104/CLI-107 are provenance only, not core verification credit.

| L3 | Executable evidence / independent oracle | Credit / limits |
| --- | --- | --- |
| L3-HDR-001 | tests/primary_header.rs: literal_header_preserves_fields_and_shared_rtc, checksum_wraps_excludes_stored_word_and_covers_every_primary_word, all_four_length_bytes_survive_field_access. | C, independent literals/upper bytes. |
| L3-LEN-001 | primary_header: checksum_qualified_lengths_isolate_minimum_alignment_and_data_bounds; integrity: width_four_range_at_largest_aligned_packet_length. | C, recomputed primary checksums isolate lengths. |
| L3-IO-002@capture-0.1 | tests/reader.rs: bounded_reader_crosses_small_buffers_without_exposing_filler, interrupted_reads_retry_but_real_errors_terminate; scripts/check_rss.py. | C, retained capture path only. |
| L3-CAP-002@capture-0.1 | reader capture-cap/secondary tests; completion: caller_limit_errors_are_typed_and_do_not_become_no_complete_packet; scripts/check_rss.py 64 MiB packet / 4-byte capture. | C; real allocator failure not forced. |
| L3-REC-003 | completion: recovery_cap_is_inclusive_and_optional, recovery_budget_resets_per_incident_and_eof_keeps_evidence; whole: recovery_chunk_boundaries_and_short_interrupted_input_keep_exact_bytes; RSS 48 MiB gaps. | C, exact candidate-displacement caps, no default cap. |
| L3-END-004 | completion: empty_and_every_first_packet_cut_are_no_complete_packet, every_established_packet_cut_remains_a_tail; whole/drain/chunk truncation suites. | C; complete inventory distinct from framing establishment by explicit drain. |
| L3-ERR-004 | reader impossible-length/typed-downcast/filler PermissionDenied regressions; completion: structural_and_io_failures_keep_typed_causes_and_recovery; whole fatal/I/O and Q8 tests. | C/P; race-only cache branch below. |
| L3-SEC-005 | integrity: secondary_byte_word_both_and_neither_are_distinct_evidence; chunks: ranges_bytes_checksums_and_commit_are_exact (literal 106/0x2644/0x1234 and None). | C, selected byte interpretation plus alternative word evidence. |
| L3-CHK-005 | integrity: every_trailer_width_keeps_data_filler_checksum_disjoint, insufficient_trailer_space_stops_before_capture, width_four_range_at_largest_aligned_packet_length. | C, capture NotChecked/Absent only. |
| L3-EDN-005 | primary_header raw flags/version tests; whole: setup_scope_unknown_edition_and_header_versions_are_explicit. | C, arbitrary versions never select edition. |
| L3-INV-006 | tests/inspection.rs interleaved rows, transactional entry limits, repeated_packets_do_not_grow_inventory; src/inspection.rs every_counter_overflow_preserves_inventory_atomically. | C, checked u64 counts; entry limits do not guarantee OS allocation success. |
| L3-SEQ-006 | tests/sequence.rs interleaved channels/wrap, successor_after_four_kib_recovery_is_not_contiguous, recovery_attached_to_other_channel_prevents_contiguous, all_recoveries_accumulate_until_each_channels_next_packet. | C, no lost-packet inference. |
| L3-PRES-006 | inspection: mid_file_recovery_keeps_unseen_presence_unknown_after_clean_eof, separate index_only/events_only fixtures; sequence one-byte sticky recovery. | C, setup presence channel0 only; absence requires unfiltered clean/recovery-free scope. |
| L3-INV-009 | inspection: whole_admission_counts_partition_authorizations_and_report_coverage; unit admission_overflow_is_atomic_for_every_field_and_other_counts. | C, exact max/overflow atomicity, capture coverage zero. |
| L3-CHK-012 | tests/data_checksum.rs independent weighted-byte/literal sums for all widths, filler, odd/short/multi-block cases, corruption, on/off/capture/counters; src/integrity.rs randomized_differential_over_unaligned_spans_and_odd_tails (900 cases), maximum-lane/carry probes. | C, no extra payload pass/copy; cost target not met, Q7 accepts ON with opt-out. |
| L3-PKG-007 | scripts/test_workspace.py metadata/default-feature/exact archive inventory; cargo package --locked (verification ON); extracted archive README/manifest comparison in candidate PR. | C on candidate CI; no path/git dependency or new types version. |
| L3-REL-007 | scripts/release/test_pipeline.py + test_workflow.py, actionlint/zizmor; gate/build/read-only publish-path job. | C dry controls only; mutation jobs skipped on PR; owner settings and real tag execution unverified. |
| L3-BLOCK-008 | whole one-block maximum / two-block boundaries, data_checksum all-width/secondary matrix; maximum_setup_fits_default_h_and_b (CI explicitly --ignored). | C, 524,288 one block; max setup 256; no automatic coalescing. |
| L3-HOT-008 | whole mixed_ordinary_frames_allocate_zero_after_warmup, whole_reader_workers_reuse_buffers_and_inspect_without_warm_allocations, hybrid pointer/call-size and adaptive straddle matrix, previous_large_includes_body_exactly_at_cutoff; pool four-worker 16-block bursts. | C, warmed pool/descriptor operations only; caller/first-entry allocations excluded. |
| L3-DEFAULT-008 | pool bounded_pool_ownership_pressure_cancellation_and_burst_hysteresis; whole policy/default/buffer configuration tests; maximum setup CI. | C, lazy zero blocks, H/B defaults, idle eight after trim, ordinary returns retain more. |
| L3-EOF-008 | whole direct_blocks_admission_eof_and_truncation, exact_eof_mutation_between_snapshots_is_refused_before_body_storage, growth_beyond_an_unchanged_stamp_is_source_changed; chunks exact-EOF/known-short and final-source fixtures. | C, no proof against undetectable same-size writes. |
| L3-LEGACY-008 | whole refusals_legacy_and_no_speculative_growth, setup_scope_unknown_edition_and_header_versions_are_explicit; ordered example routing tests. | C, explicit default-off option and evidence, not inferred. |
| L3-GROW-008 | whole small_huge_claim_is_one_provisional_block_or_known_eof_drain; whole_resources bogus case; prior touched-P-allocation control. | C, fixed provisional block/read-ahead, known-short zero blocks; not exact per-byte RSS. |
| L3-ADM-008 | whole failed_probe_restores_cursor_and_never_falls_back_to_stream_trust, frame_feasibility_precedes_both_probe_and_known_short_drain, recovered_seekable_oversoft_frame_preserves_prefetch_during_probe_or_exact_eof; chunk exact-H tests. | C/P; no separately isolated false-positive-in-payload corroboration oracle located; see gap list. |
| L3-SETUP-008 | CI explicitly runs maximum_setup_fits_default_h_and_b: 128 MiB accepted at default H/B, coalescing BudgetTooSmall; whole_resources setup/coalesce raised B=272 MiB. | C, actual touched 128/256 MiB data; default B unchanged. |
| L3-BUD-008 | pool units readiness_after_wake_wins_over_expired_deadline, deadlines_expire_without_cancelling_or_consuming_resources, no_returner_is_provable_only_without_outstanding_resources, spurious wake/cancel/allocation rollback; whole worker-return; coalescing exact fit/two copies/concurrent build/queued-reader fairness. | C/P; bounded joins, used<=B; begin_until race refusal cache not isolated. |
| L3-DRAIN-013 | tests/drain.rs live_size_refusal_then_explicit_drain_then_next_packet, refusal_then_stop_leaves_live_body_unread, hard_ceiling_is_inclusive_for_explicit_drain_reason, source_mutation_after_first_quantum_aborts_before_reading_remainder, typed error/tail/cache/recovery cases; whole allocator oracle. | C; i686 explicitly executes synthetic 4 GiB drain; 64-bit all-OS 64 MiB drain gate. |
| L3-CHUNK-014 | tests/chunks.rs range/checksum/secondary/provisional-error/callback-lifetime cases, source_mutation_after_first_chunk_quantum_aborts_before_reading_remainder, chunk_hard_limit_accepts_exact_h_and_positions_h_plus_four, active full pool and huge-short-claim; whole allocator oracle; compile-fail borrow/Inspection doctests. | C; new C10/C16/C21/C24 mutants killed, 4 KiB callbacks, returned End only; 64-bit all-OS 64 MiB/B=4 MiB gate; i686 functional only. |
| L3-RSS-008 | scripts/check_whole_resources.py cold/bogus/refuse/recovery/drain/chunk 12 MiB, pressure baseline 12, active B=16 +8 MiB overhead, admit/setup/coalesce 80/144/272; capture scripts/check_rss.py 32 MiB unchanged. | C/P; trim-to-eight accounting/checkpoints covered, no separate lifetime-peak <=12 MiB process after a prior large burst. |
| L3-PLAT-008 | Same core-only GNU time/macOS time/Windows commit+working-set probes; Windows i686 executes workspace and full synthetic drain; allocation/request/accounting tests and prior touched-allocation/retention controls. | C/P; no i686 memory gate, no all-architecture/allocator/OOM guarantee. |

## Evidence and execution boundaries

Release-preparation oracles first failed against 0.1.0 metadata and the old Build
without whole-mode qualification, then passed after the version/workflow updates.
A temporary control omitting the whole-memory dry-run command fails the new
workflow contract; it was restored. The new borrowed whole-packet rustdoc example
executes alongside retained capture and compile-fail API examples.


- Prior full feature qualification at reviewed bcd251b: [CI 46103591726](https://github.com/TelemetryWorks/irig106-core/actions/runs/46103591726),
  [CodeQL 46103588003](https://github.com/TelemetryWorks/irig106-core/actions/runs/46103588003),
  [unsigned TP 46103591808](https://github.com/TelemetryWorks/irig106-core/actions/runs/46103591808).
  Chunk peaks Linux 2,972 / macOS 2,256 / Windows 4,724 KiB vs 12,288; i686
  source-check/boundary/huge-short tests and synthetic 4 GiB drain actually ran.
  The four review mutants are killed independently; not every possible mutant.
- [Historical whole memory controls](verification-v02-s07.md) cite run 45847406093:
  touched P preallocation 133,856 KiB vs 32,768; unbudgeted retention 133,892 vs
  81,920. These are one-off removed controls, not part of candidate code.
- Capture [slice 8](verification-s08.md) records independent packet-sized-drain
  and recovery-retention negative controls on its 240 MiB fixture. Source/input
  generation and compilation stay outside the measured core process.
- Linux/macOS RSS sees resident touched pages only. Windows also samples private
  committed/peak commit memory. Ceilings are scoped regression evidence, not B
  itself and not a general memory guarantee. i686 runs functional tests only.
- The current candidate repeats full Rust/doctest/OS/MSRV checks, package
  verification, examples, memory gates and workflow audits in CI. The unsigned
  Trusted Publishing Build also executes Linux max-setup/coalescing, ordered
  examples and both memory suites before artifact upload; the read-only path
  reproduces candidate bytes with the same pinned compiler and checks published
  core 0.0.1 against its unchanged trusted fixture. No signatures are made.

## Still unverified, partial or deliberately outside scope

1. **Concrete oracle gaps:** the race-only BudgetTooSmall from begin_until after
   an intervening workspace constructor (review #30 P5) lacks a deterministic
   cache/resume test. Upfront BudgetTooSmall/cache and workspace rechecks are
   tested. No dedicated fixture distinguishing a checksum-valid lookalike *inside
   payload* at the corroboration offset was located for L3-ADM-008; positive probe,
   hard/budget ordering and failed-probe paths are tested. There is no isolated
   small-process RSS test after a large burst proving eight-idle-block residency;
   trim credits/counts are checked but large cases retain their lifetime peak.
   These rows are partial, not silently marked fully verified.
2. **Allocator/OS limits:** injected reservation/capacity failures and rollback
   exist, but actual global OOM/commit exhaustion, allocator-private overhead and
   Linux OOM-killer survival are not proven. Unwritten virtual reservations can
   evade RSS. Coalesced Vec-before-credit Drop order is inspected and Rust field
   order is relied upon; no allocator-hook free/credit timestamp oracle exists.
   There is no exhaustive concurrency/model-checking/fuzz campaign or claimed
   100% branch/mutation coverage. BTree inspection allocation is entry-bounded,
   not fallible try-reserve storage.
3. **Source/transport contracts:** StableSource cannot detect same-size mutation
   hidden by its adapter's stamps; no platform File identity adapter is supplied.
   Cancellation/deadlines cannot preempt arbitrary blocking Read/Seek, allocator
   faults or callbacks. Nonblocking WouldBlock is terminal. Actual UDP transport,
   reassembly/loss/backpressure adapters and semantic INT-045 setup/time closure
   are downstream work, not established by generated stdin/Read fixtures.
4. **Performance:** generated warm-cache direct/hybrid and checksum benchmarks
   are informational, not real recorder/storage/network performance. The <10%
   warm-cache checksum cost target was **not met**: reviewed #29's optimized
   run 45879792260 showed roughly 19–23% at 64 KiB and 26–30% at 512 KiB; owner Q7
   nevertheless chooses ON with opt-out. Current candidate timings are in CI;
   no timing gate, geometric-refill/size-class optimization or universal claim.
5. **Platform/standard scope:** Windows i686 functional coverage is not a memory
   gate; no 32-bit Linux/macOS or all-target qualification. Local aarch64 checks
   do not substitute for other targets. Core does not validate payload meaning,
   full edition/recording constraints, nonempty DATA or per-channel type-version
   constancy; secondary RDG-008 ambiguity remains evidence, not time validity.
   no_std/alloc, async and downstream tmats/time/decode integration are not part
   of this release. No downstream repository was modified or certified here.
6. **Publication:** real v0.2.0 remote annotation/main-CI ancestry, signature
   verification, crates.io served-byte/checksum identity and docs.rs success await
   the separately authorized tag run. PR artifacts are unsigned. Server-side
   Trusted Publisher registration/only-mode, tag eligibility, required owner
   approval and disabled bypass need owner confirmation; dry controls cannot
   prove them. The successful 0.1.0 release and incident recovery are historical,
   not evidence of 0.2.0 publication. Future tag-pinned README links resolve only
   after that authorized tag exists.

These limits do not authorize relaxing requirements, raising gates, publication
or hiding failures. The owner/reviewer decides candidate acceptance with this
explicit partial-verification record; publication needs a separate go-ahead.
