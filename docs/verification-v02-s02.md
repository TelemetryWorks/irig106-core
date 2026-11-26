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
forbids unsafe. Block geometry/default B are approved design inputs, not an encoder.

| Check | Scoped requirement evidence |
| --- | --- |
| tests/pool.rs burst | Partial L3-HOT-008: four workers, 16-to-zero swings, zero **all** allocations over two warmed bursts; ordinary returns retain 16, explicit pressure trims to eight. No whole-reader throughput claim. |
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
