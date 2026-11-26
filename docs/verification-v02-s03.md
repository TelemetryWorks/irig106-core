# Core 0.2 slice 3: reader/admission integration

Base: main 1b86a4a (merged #18). Version remains 0.1.0; no release action.
Owner assignment requests reader/admission integration with the direct-read hot
path. This PR therefore includes the minimal whole-frame path originally listed
under slice 4 as well as slice-3 admission. Coalescing, optional reported drain/
chunk modes, broader platform/RSS controls and performance comparison remain
later work. The diff exceeds the 300–600-line guideline to keep these coupled
paths and their independent tests in the requested one PR.

Tests-first 053b264 adds pool watchdogs and the gated wake/expiry test. The waiter
is observed inside its condition-variable wait, then the test holds the mutex
through deadline expiry while publishing readiness. On reacquisition, readiness
must win even though the deadline expired. No sleep guesses wake ordering.
A readiness-after-wake expiry-first mutant fails (exit 101). Separate controls
ignoring the deadline and making try-acquire block fail within 5.64/5.40 s,
respectively, through the new watchdogs instead of a CI job timeout. All removed.
8e15668 records hand-packed integration fixtures before the new APIs were exported;
the missing StableSource/SourceStamp compile failures are distinct from runtime
regression evidence. A transient visibility typo during extraction was corrected
before observing those missing-API failures.

| Probe | Scoped evidence |
| --- | --- |
| final destination | Every normal body read points at the final contiguous block plus its frame-relative offset, with positive short reads and Interrupted; includes secondary and width-4 trailer bytes. No intermediate body staging. |
| allocation counter | Thread-local test-only GlobalAlloc counter covers successful reader/pool operations over 31 warmed mixed ordinary frames (28, 400, 4096, exactly 524288 bytes); zero allocations. Input/oracles/descriptors are prepared outside counting. Full one-copy/performance guarantees for arbitrary externally buffered Read are not claimed. |
| admission | Defaults; explicit edition including raw uncertainty; channel0/type01 scope; explicit nonzero-channel legacy allowance; no inference from 0xff type version; hard refusal before body; oversoft stream refusal/opt-in; next-header evidence; stable exact EOF admission and one-byte-short truncation; observed revision mutation is SourceChanged. |
| storage/growth | A short 8 MiB claim uses one provisional block on stream input, zero on known-short seekable input; no complete prefix. Two-block whole frame with exact spans/DATA; one-block standard maximum remains contiguous. Feasibility before body allocation; workspace charged/released, cannot expand during active assembly; feasibility rechecked after admission wake; refusal/known-short paths do not wait for unrelated builds; worker return wakes integrated reader without extra block allocation. |
| terminal behavior | Header/checksum recovery uses shared engine; one-byte cap boundary; tail offsets/recovery evidence; checksum-valid impossible lengths remain typed HeaderError with no search; PermissionDenied stays operational/terminal; post-error/tail None. |
| maximum setup | CI explicitly runs the ignored synthetic 128 MiB channel0/type01 frame at default H/B, no fixture-sized Vec; 256 spans, complete bytes count, used <= B, return trims to eight. Not an RSS or coalescing claim. |

The prior isolated 32 MiB capture-path RSS gate stays unchanged. Full
L3-GROW/ADM/SETUP/HOT/BUD/PLAT obligations are not all discharged: multi-block
DATA-boundary matrices, explicit coalescing/workspace controls, Windows commit
measurement, broad RSS mutants and throughput comparison remain to their slices.
StableSource is an adapter contract, not proof against undetectable same-size
writes. Library code remains safe/std-only; allocator instrumentation alone is
unsafe in tests. Existing 0.1 capture checks run unchanged. Package inventory
includes the new modules/test. Exact-head CI and publish-path dry-run IDs are
recorded in the PR after completion.

Final provenance regression: Unknown(7), channel0/type01, explicit legacy option
earns the setup allowance only through that option and must report it as used.
The added table case observed false instead of true before the evidence fix;
admission itself is unchanged. Known 24R1 channel0 still needs no legacy evidence.

Truncated large assembly cleanup includes its partial/provisional block: an
eight-full-block prefix plus partial ninth originally left nine idle blocks,
missing the large-return trim. The regression now returns all nine together and
trims to eight, with no complete packet exposed.


## Review corrections: recovery I/O and admission/source oracles

Tests-first bedd240 reproduces 100,013 Read calls for 100,000 garbage bytes,
two small frames and a 31-byte tail. Whole-mode recovery now uses an 8 KiB
fallibly allocated scratch chunk charged to B once at construction; the same
probe takes **15 calls**. Logical offsets, scanned-byte caps and tail counts
advance only on consumption. Unconsumed recovery read-ahead is preserved across
frames and seek probes. Tests cover sync straddling the chunk boundary, short/
Interrupted input, next-header and exact-EOF admission after recovery. Only bytes
already prefetched by recovery take the bounded extra copy. Normal body reads
remain direct and allocation-free after warm-up; L3-HOT's destination oracle is
explicitly scoped to that normal path, matching the design's recovery exception.
Capture-mode construction still has no recovery scratch allocation.

A2 is killed by two order checks: BudgetTooSmall must precede both corroboration
and known-short draining, with only the primary read and zero stamp calls.
A12 is killed by actual bytes exceeding a deliberately unchanged size stamp.
A5 is killed by a revision change between initial and immediate pre-admission
snapshots, requiring SourceChanged at byte 24 and zero block allocations.
All three one-off mutants exit 101 and were discarded. The immediate recheck is
kept for the approved admission contract; it does not guard the later read
interval. Post-body stamp/consumed-size validation remains mandatory.

Normal body reads now use remaining block/frame, at most 512 KiB, rather than
an artificial 4 KiB syscall quantum (known-short drain scratch stays 4 KiB).
A maximum ordinary frame takes **2 Read calls instead of 129**. The reproducible
ignored bounded_file_read_benchmark writes 8,389,056 bytes / 32 mixed frames
outside measurement, then reads a warm-cache File in a debug build. On this VM,
one bounded before/after run measured:

| Policy | Read calls | packets/s | MB/s (decimal) |
| --- | ---: | ---: | ---: |
| Previous 4 KiB body reads | 2,097 | 5,993.4 | 1,571.2 |
| Block/frame-bounded reads | 65 | 7,955.6 | 2,085.6 |

These single-run timings include first block initialization and are scoped
warm-cache evidence, not disk throughput, release-mode comparison or a stable
performance threshold. Read-call counts are the deterministic regression oracle.
Ubuntu CI explicitly runs this informational benchmark as well as maximum setup.
No timing threshold or RSS ceiling was changed.

API docs now state that reader DeadlineExceeded/Cancelled is terminal and cannot
resume a consumed frame; into_inner discards at most 8 KiB recovery read-ahead.
The low-level pool's resumable deadline contract is distinct. Error::kind maps
Cancelled and provably permanent BudgetUnavailable to Other, with a dedicated
test; only temporary WouldBlock requests the corresponding retry classification.
