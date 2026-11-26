# Whole-reader test watchdogs

Base: merged #19 at main 0bab0f1, including the deliberately approved fb5ca5e
benchmark. The benchmark stays in tests/whole.rs, with its corpus, measurement
body and CI command unchanged. No production Rust, small-body strategy, API,
requirements, manifest, dependency or publishing-policy changes.

All 23 whole-reader tests retain their names/bodies and enter a shared test-only
operation/termination watchdog. Ordinary cases have 5 s; the ignored file
benchmark 30 s; maximum setup 60 s; small-body matrix 120 s. Parents use a bounded
completion channel, then bound termination before joining. Original panic
payloads are forwarded. Operation-thread allocation/syscall measurement scopes
remain intact. Wrappers avoid reindenting or changing the test bodies.

S1 control removes scratch.used += n. Without watchdogs the selected recovery
test exceeded the external 3 s reproduction bound, requiring its owned process
group to be stopped. With watchdogs it exits 101 after 5.00 s test execution
(5.56 s including compilation), reporting whole-reader operation exceeded
watchdog. The temporary mutant is discarded; the production-source diff is empty.
The package inventory adds only tests/support/watchdog.rs. Exact-head CI evidence
is recorded in the PR. No performance threshold is introduced.
