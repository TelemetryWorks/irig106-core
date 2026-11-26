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
| final destination | Every body read points at the final contiguous block plus its frame-relative offset, with positive short reads and Interrupted; includes secondary and width-4 trailer bytes. No intermediate body staging. |
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
