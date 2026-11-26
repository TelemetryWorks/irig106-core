# Core 0.2 slice 2: shared budget and lazy block storage

Base: merged #17, b6aa008a8126d53f7acb66c3aea47c5db98e3eb3. Tests-first
c636efc cannot compile on that base (missing pool APIs, E0432/E0425); this is
separate from the runtime reproduction below. Version remains 0.1.0; no release.

The four-worker, 16-block burst oracle failed an uncommitted eager-trim policy:
after warm-up, the next two bursts made **16 allocations instead of zero**.
Ordinary-return hysteresis makes the same allocation-counter check pass. This
reproduces a proposed-pool tension, not a bug in the released reader. Worker
mailboxes, barriers and oracle storage are allocated before measurement.
The unsafe System allocator shim is test instrumentation only; the library still
forbids unsafe. Block geometry/default B are reviewed design inputs, not an encoder.

Initial macOS CI at 643f8a7 saw two process-wide allocations during the counter
window, which also covered the synchronization oracle. Counting now scopes all
producer acquisition/permit-drop and worker block-drop operations with per-thread
markers, excludes mailbox/barrier code, and pre-warms every worker's admission
wait. The scoped counter still kills the eager-trim mutant with 16 allocations;
the mutant was removed. No macOS-specific allowance or nonzero threshold is used.

| Check | Scoped requirement evidence |
| --- | --- |
| tests/pool.rs burst | Partial L3-HOT-008: four workers, 16-to-zero swings, zero heap allocations inside producer/worker **pool operations** over two warmed bursts; ordinary returns retain 16, explicit pressure trims to eight. No whole-reader throughput claim. |
| tests/pool.rs budget/lifecycle | Partial L3-BUD/DEFAULT-008: lazy zero-block construction, B=256 MiB and block=512 KiB; idle/live credits; impossible request before waiting; exclusive admission; actual return wakes checkout; terminal cancel wakes both waits; nine-block return plus explicit trim yields eight. |
| pool unit probes + doctest | Failed reservation and capacity mismatch roll back credits; poison is typed and Drop does not panic; borrow cannot outlive returned block; owned storage/handles are Send. |

Control descriptor capacity is reserved fallibly from the **caller budget**, not
P, and charged alongside fixed control/assembly allowance and per-block lease
space. Returning storage cannot grow that vector. Each cold block claims credits
before fallible reservation/initialization; allocator-visible capacity mismatch
fails closed. Fixed small Arc control allocation and allocator-private overhead
are not an OS-OOM guarantee; B is accounting, not exact RSS.

Only fixed-block storage is implemented: no PacketBuf valid lengths or reader
integration, no H/edition/legacy admission, no EOF corroboration, no arbitrary
coalescing/control-workspace reservation. Those require later slices. Full L3-HOT,
GROW, BLOCK, SETUP, RSS and platform evidence is not earned by this pool test.
Heavy qualification and the unchanged isolated RSS gate run in CI; exact-head
run IDs are recorded in the PR, including the publishing-path dry run.

## Review fixes: bounded waits and idle cancellation

Tests-first 3a72ebd records the new deadline/unavailable contract (missing API
compile failures are distinct from runtime evidence). A temporary probe on
3edab6b reproduced same-thread retention entering an indefinite default wait;
only an external cancel released it. The idle-cancel mutant now fails with two
allocated/idle blocks instead of zero. Both probes/mutants were removed.

Default acquisition is now nonwaiting BudgetUnavailable. Explicit absolute-deadline
variants use wait_timeout and predicate loops; unit checks cover positive expiry,
spurious notification without readiness, unchanged slot/credits after timeout,
and reuse. Send does not expose transfer-thread identity: nonwaiting defaults
handle all no-returner cases without guessing ownership. Bounded waiting is an
explicit caller choice when another owner may release storage.

Cold allocation is outside the mutex; a deterministic callback verifies try_lock
and cancels during allocation, then checks claim rollback. Idle cancellation is
idempotent and returns used bytes to fixed control cost. Integration workers use
bounded acknowledgements and joins (5 s watchdog, 30 s operation deadlines), so
missed notification/release mutants fail without waiting for a CI job timeout.
Current-default admission accepts 511 blocks and rejects 512 without payload
allocation; this is a storage-only bound, not a whole-reader capacity claim.

One-off controls removed the block-return notification and assembly-release
assignment separately. The integration test failed in 5.06 s and immediately,
respectively (both exit 101); healthy code was restored. These are bounded test
failures, not CI job timeouts.
