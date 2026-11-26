# Adaptive header refill requests and minimum-capacity regression

Base: main ef6cad8, merged #21 at 6221116. Owner policy unchanged: one fixed
64 KiB read-ahead allocation, default 4 KiB body cutoff, approved two-copy path.
Only header-phase refill REQUEST size adapts. Cold/full-after-small requests use
capacity; after body >= cutoff, use the allowed header-only alternative: 24 bytes, within the
min(capacity, 24 + cutoff) bound and requiring no overflowing cutoff addition. Pending bytes are consumed before any refill, including
straddling headers. Small-body and recovery refills remain full capacity. The
copy-once-per-body predicate is unchanged. Capacity/workspace remains charged to B.
The capture-mode wrapper still requests its original buffered framing behavior.

Tests-first 0f927b3 fails the original full-refill behavior: the recorded request
following a 64 KiB frame is 65,536 instead of the tested bounded request (initial candidate 4,120; final
header-only choice 24). New traces assert large/small
transitions, small-body refill after a limited short header, recovery refill after
a limited invalid header, and 65,536 / 65,560 / 524,288-byte repeated frames.
The cold first large frame intentionally retains its initial full-capacity read.

A bounded matrix checks 196 configurations (seven capacities, seven cutoffs,
four short-read limits including one-byte input), 1,960 complete packet paths.
Ten mixed sizes include cutoff boundaries, 64 KiB and 64 KiB + 24, secondary/
checksum flags, straddling headers and inserted recovery gaps. Every packet's
bytes, offset and recovery count, plus terminal position/EOF, are checked. Each
input also injects Interrupted. The local matrix completes in about 2.5 s under
a 30 s test watchdog. The reviewer's separate 1,919-case probe was not attached to
GitHub; this reproduces its requested cross-product shape, not its exact script.

H12: both public constructors reject 23 with PoolError::InvalidRequest, leaving
used_bytes unchanged; 24 is accepted and returns all workspace on drop. A direct
private-factory unit test prevents the public guard masking a mutated private
minimum. One-off always-full-refill and private-minimum-1 controls both fail
with exit 101 and assertion panics; both discarded. H5 (>= versus > when exactly
the body is buffered) remains equivalent: both paths drain without another fill.
No extra H5 oracle or policy change is introduced.

The unchanged release-mode small_body_read_strategy_benchmark runs in CI against
the actual adaptive implementation, direct-body baseline versus default hybrid,
for 28 B / 400 B / 4 KiB / 64 KiB / 512 KiB, median of three alternating-order
warm-cache runs, about 16 MiB per size. Syscall counters calibrate against source
reads. Results and exact-head run IDs are in the PR. Informational only: no timing
gate. Existing zero-allocation, SourceChanged, probe/cursor, truncation, max setup,
0.1 capture, MSRV and 32 MiB RSS checks still apply. No unsafe/dependency/manifest/
version/publishing changes. No allocation is derived from declared Packet Length.

The permitted header-only alternative was selected to eliminate the large-body
prefix at empty boundaries rather than merely reduce it to 4 KiB. Small-to-large
transitions can still use a full fill, and straddling headers may prefetch up to
23 body bytes. Small-body refills retain full capacity; no timing advantage is
assumed from noisy local smoke measurements. Final release CI is the evidence.
