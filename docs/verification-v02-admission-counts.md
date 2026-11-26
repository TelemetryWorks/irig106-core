# Core 0.2: admission-evidence inventory (owner decision 3A)

Base main fffc85a, containing reviewed #26. Tests-first f62f25e fails with E0599
for the missing admission_counts API. This PR implements counters only; explicit
coalescing, drain and chunk are approved but belong to subsequent separate PRs.
Version/dependencies/read policy and all memory ceilings remain unchanged.

Six independently packed whole-frame cases assert incremental vectors, not just
final equal totals: WithinSoft, oversoft NextHeader (peek is not observed), stable
Unconfirmed(ExactEof), explicit StreamOptIn, nonzero legacy setup and channel-0
setup with legacy enabled but unused. They prove the current authorization
partition and independent overlapping over-soft/legacy facts. A capture event
adds total packets without fabricating evidence coverage. Incomplete status keeps
accepted counts. Existing channel/row rejection tests assert unchanged admission.

The private arithmetic test seeds all seven u64 counters near MAX, accepts exact
MAX, then rejects the next addition. It snapshots all counts, row/channel state,
sequence checkpoints, skipped bytes and presence; overflow changes only terminal
status. A normal-total overflow also discards a proposed admission increment.
The public evidence struct has a non-exhaustive compile-fail doctest. Existing
four-worker zero-allocation tests and all-OS 12/24/80/144 MiB whole-memory gates,
capture 32 MiB, i686 and publish dry run remain enabled.

The public examples append admission=AdmissionCounts to their output. Executed
fixtures pin whole coverage/WithinSoft and capture-only coverage zero. Prior
fields/exit policy and capture reading behavior remain unchanged. Typed accessors,
not Debug text, are the consumer integration contract. Exact-head CI run IDs and
one-off mutant results are recorded in the PR after execution. Local controls
omitting over-soft, routing NextHeader as StreamOptIn, wrapping counter arithmetic
and inventing capture evidence all fail with assertion panics/exit 101. Each was
restored before final lint/diff checks; no mutant remains. No tag/publication.
