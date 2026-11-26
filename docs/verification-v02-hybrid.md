# Whole-packet hybrid reads: owner Option A (2026-11-26)

Base: main 6d6ffda. Owner decision references benchmark runs 45740882675 and
45741995400. L2-HOT-008 / L3-HOT-008 now permit the explicitly bounded two-copy
path; H/B defaults and all capture-mode requirements remain unchanged.

One fixed read-ahead Vec (64 KiB default) replaces the recovery-only 8 KiB Vec.
Construction reserves capacity fallibly, rejects target/size errors and charges
capacity plus reader/drain/control workspace to B before reservation. No size is
derived from a packet header. Headers, recovery and small bodies share it.
Buffered bytes are consumed before direct reads; logical offsets/caps count
consumption. Probe bookmarks restore the physical cursor without discarding
pending bytes. into_inner may discard up to the configured capacity of read-ahead.

Cutoff is 4,096 **body bytes after the primary header**, strict less-than, including
secondary/filler/trailer. This covers the winning 4 KiB frames (body 4,072 bytes),
without claiming a win for unmeasured intermediate sizes; 64 KiB lost in both
reference runs. A fully buffered body copies independently of cutoff. Large
bodies drain their buffered prefix then read directly for the remainder. The
choice is made once by whole body size, never by an individual short read or a
tiny final block. with_small_body_cutoff is configurable; zero disables only
size-triggered refills. with_read_ahead_bytes is independently configurable,
at least 24 and within target/B; buffer=24/cutoff=0 is a direct-body baseline.

Tests-first 2df7779 records missing policy getters/builders before implementation.
New pointer/call-size tests check below/equal-cutoff and override boundaries,
fully buffered copies with cutoff zero, a large prefetched prefix plus direct
remainder, and a direct 4-byte final block. Buffer accounting and invalid/over-B
construction are checked without allocating a declared P. Existing mixed-size
warm-path allocation counter still yields zero. Short/Interrupted reads retain
their destination/offset contract. Existing recovery/cap/EOF, truncation-only,
permission/fatal-header, pool return/deadline and complete-span tests still run.

SeekChecked captures a size/identity/revision baseline before **any** read-ahead,
then compares admission and delivery stamps to it. This catches mutations while
the first header fill may already prefetch a small body. The post-body guard is
still required and tested with a mutation only at EOF on a frame larger than the
read-ahead. Snapshot calls are constructor/admission/delivery work, not body
allocation; the pre-feasibility tests count only the constructor snapshot.
The adapter still excludes undetectable same-size concurrent writes.

One-off controls removing the post-body guard or read-ahead budget charge,
or reclassifying a small last fragment of a large frame, all fail with exit 101.
All discarded. Existing test watchdogs remain. Library code remains safe/std-only;
no dependency, manifest, version, publishing or settings changes.

The informational benchmark now uses the **actual hybrid implementation** on
both sides, configured as direct-body baseline (24-byte header buffer, cutoff=0)
or default hybrid (64 KiB, cutoff=4 KiB). No external buffered-copy prototype is
in the comparison. Same five frame sizes, about 16 MiB per size, alternating
order, median of three, release-mode warm-cache File input; construction/first
block initialization outside timing. Linux per-thread kernel read syscall counts
are calibrated against source reads, including EOF and excluding the counter read.
A bounded local 1 MiB debug smoke passes all ten outputs/calibrations; full
release-mode numbers and exact-head CI run IDs are recorded in the PR.
The 128 MiB setup acceptance and unchanged isolated 32 MiB capture RSS gate run
in CI. No timing threshold or blanket memory/performance guarantee is introduced.
