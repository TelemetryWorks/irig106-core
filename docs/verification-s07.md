# Slice 7: per-channel sequence and consumer migration

Base main bd6d2120b543f621884f483d60f2380888c42f4d (approved PR8).
Tests-first cfd3d8d: sequence tests fail compilation with E0432/E0599 (missing
public types/method). This is not a runtime failure. InspectionError Display
already passes production; no Display implementation change is claimed.

## Source and scoped requirements

Re-inspected the frozen 106-24R1 Chapter11 PDF from [slice 2](verification-s02.md),
SHA256 0f63792dcac39f6010df371ee0afc46a396017918f4155cba9ebab6c2d6d263e.
PDF16/printed11-8, §11.2.1.1 f specifies separate channel sequence counters,
incrementing per packet, an arbitrary initial value, and rollover after FF.
The comparisons implement L2/L3-SEQ-006; duplicate/discontinuity names and
recovery handling are observation policy, not proof of loss or reset semantics.

Interleaved channel fixtures cover nonzero first values, mixed known/unknown
types on a channel, 254→255→0, duplicate, forward/backward discontinuities and
the next reference after a discontinuity. Independent headers are hand-packed
with a scalar wrapping-word checksum; no production encoder supplies the oracle.
One-byte recovery reports offset29/skipped1 then offset57/skipped0: comparison
continues across recovery and unseen presence stays Unknown at clean EOF.
This also checks ordinary observe and observe_with_sequence share the reference.
Tails return no sequence; stopped/zero-budget observations reject without state.
The existing nine counter-overflow cases now check last-sequence atomicity with
a different rejected sequence byte (independent F445→F545 checksum control).

One last u8 is stored per channel in the existing budgeted map. Each accepted
packet returns one non-exhaustive evidence struct; no history or diagnostics
are retained. Existing inventory cardinality tests still apply; no new RSS or
large-recording measurement is claimed (slice 8 remains planned).

## Controls and consumer checks

Six compiled runtime mutants fail focused tests: recovery threshold >1,
saturating wrap, global channel reference, never advancing the reference,
discarding recovery evidence, advancing before checked-counter failure. Source
restored before final checks. Six ErrorKind variant compile-fail blocks now
require E0638; removing their field non-exhaustiveness makes all six guards fail
because their examples unexpectedly compile. New sequence evidence/type guards
were added with implementation, not failing first.

InspectionError Display covers both typed limits, Stopped and CountOverflow.
The runnable DATA-capture doctest checks four independent payload bytes. The
stdin example was run on small independent fixtures: complete wrap (exit0),
one-byte recovery (exit0, Unknown), terminal tail (exit0, retained totals), and
empty input (exit3, incomplete). No payload interpretation is introduced.

[Migration](migration.md) records time's future transport replacement and
downstream command/content checks; no other repository was modified or tested.
Exact-head CI links and local command results belong in the PR. No dependency,
version, publishing workflow, release, merge or next-slice work is included.
