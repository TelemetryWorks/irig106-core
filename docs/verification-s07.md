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

One last u8 and recovery checkpoint are stored per channel in the existing
budgeted map, plus one checked stream recovery total. Each accepted
packet returns one non-exhaustive evidence struct; no history or diagnostics
are retained. Existing inventory cardinality tests still apply; no new RSS or
large-recording measurement is claimed (slice 8 remains planned).

## Controls and consumer checks

At initial review head edfc2cd, six compiled runtime mutants fail focused tests: recovery threshold >1,
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

## Review correction: skipped input makes continuity unverified

Tests-first 50eae0e reproduces both blocking probes at runtime: channel1 8→9
with 4096 skipped bytes, and channel1 5→6 when those skipped bytes attach to
channel2's packet. Both previously report Contiguous. The new stdin runner also
fails against that head because the recovery example emits no Unverified finding.

SequenceObservation now exposes skipped_since_previous; an expected successor
after nonzero skipped input is Unverified. A checked cumulative recovery total
and bounded per-channel checkpoints accumulate ALL incident bytes. First has no
prior interval; only the observed channel's checkpoint advances on acceptance.
An additional three-channel test checks exact interval totals 0/0/0/7/14/0/6/0
across multiple recoveries, first observations and checkpoint resets. Private
controls accept u64::MAX exactly, reject its successor, and preserve counters,
channel checkpoints and the cumulative total on failure. These were added with
implementation, not failing first.

CI now builds AND executes the public example using Python subprocess stdin
pipes: wrap requires exit0/Complete, recovery exit0/Unknown/Unverified, empty
input exit3/incomplete. The same script passes locally. Migration documents the
concrete header accessor/RTC changes and open Event/TailKind/Tail patterns.
Five compiled mutation controls fail at runtime: track only the current incident,
never advance recovery checkpoints, report expected successors as Contiguous
despite skips, commit the recovery total before counters, and wrap its sum.
Production is restored before final verification.
