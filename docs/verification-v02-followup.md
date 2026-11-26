# Core 0.2 follow-up: read-strategy benchmark and test watchdogs

Base: main 0bab0f1. The actual merged tree already contains benchmark commit
fb5ca5e; this follow-up isolates it into tests/read_strategy_bench.rs and reruns it
with a fresh exact-head CI record. It compares 28 B / 400 B / 4 KiB / 64 KiB /
512 KiB complete frames, direct File reads versus an external 64 KiB buffered-copy
prototype, using the same unchanged public whole reader. No production Rust,
read strategy, resource policy, manifest or dependency changes.

Release-mode Ubuntu CI uses about 16 MiB per size, median of three runs in
alternating mode order. Warm-cache file generation, opening, constructors and
initial pool-block zeroing are outside timing. Output gives source reads, actual
Linux per-thread read syscalls (calibrated /proc/thread-self/io), packets/s and
MB/s. EOF read included. The prototype buffer is extra memory outside B in this
harness; owner approval and budget/cursor/recovery integration are still required
before any production adoption. Results and exact-head run ID are in the PR.

All 22 tests in tests/whole.rs now enter a shared operation/termination watchdog:
ordinary cases have 5 s; the ignored file benchmark 30 s; the ignored maximum
setup 60 s. The isolated decision benchmark has 120 s. Parents wait on a bounded
completion channel rather than spin while operations run. Worker panics are
forwarded; thread termination is bounded before join. Existing bodies and test
names/filters stay intact. Watchdog workers preserve thread-local allocation and
kernel-counter measurement scopes.

S1 control removes scratch.used += n. On the unprotected baseline, the selected
recovery test exceeded 3 s and its owned process group was explicitly stopped.
With the watchdog it exits 101 with whole-reader operation exceeded watchdog in
5 s of test execution, rather than waiting for the CI job timeout. The temporary
mutant is discarded; production-source diff is empty.

Local qualification: a bounded 1 MiB-per-size debug matrix smoke validates all
ten mode/size outputs and kernel-counter calibration. Heavy release-mode matrix
and 128 MiB acceptance run in CI. The package inventory includes the isolated
test and shared test-only support module. No timing performance gate is added.
