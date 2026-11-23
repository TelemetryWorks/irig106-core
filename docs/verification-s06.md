# Slice 6: bounded header inventory and API follow-ups

Base main 1d44d0a761ef1cd2abf4d1d52a7994f14f8744b7 (approved PR7).
Tests-first a3e14da: missing inspection APIs fail compilation; eight new guard
doctests FAIL because closed matches still compile; the tail/recovery Display
probe fails at runtime on missing text. Width4/FFFFFFFC range control already
passes base production; it additionally checks one-byte DATA overlap rejection.

L3-INV-006/PRES-006: independent packet/DATA/packet-byte totals, interleaved
channels/types, raw unknown codes, row minima/maxima, exact channel/row cap edges
and zero budgets. Rejected packets change no inventory facts. A 10000-observation
four-row fixture keeps two channels/four rows/two types; this is entry-cardinality
evidence, not process RSS. No packet payloads or diagnostics are retained.

Presence is accepted header routing, not content validity or enabled features:
setup channel0/type01, index03, events02. Absent requires caller-declared clean
EOF after at least one packet, no recovery and no failure; tails, errors and
early stops leave unseen records Unknown. Present facts survive failure. Empty cannot finish as
Complete. Callers must feed all events and mark read errors/omissions incomplete.
Sequence continuity remains slice7; no payload decoding/dependency is introduced.

All three u64 counters at global/type/row scope are forced to MAX privately in
nine small unit cases: checked overflow rejects atomically and keeps status
Incomplete. This control was added with implementation, not failing first.
Eight runtime mutants compile and fail named tests: ignore either entry cap;
count packet bytes as DATA; commit globals before later overflow; upgrade tail
status at finish; ignore setup channel; report unknown as absent; omit raw D9.
Source restored, focused tests rerun. No large recordings/OOM campaign locally.

Event/TailKind and all field-carrying ErrorKind variants are non-exhaustive.
Io/Header/Allocation become named source-field variants because Rust makes
non-exhaustive tuple variants inaccessible even to open external patterns.
Consumer migrations keep typed causes/downcasts; golden reason Display cases
move into a unit test since clients cannot construct these variants. New display
includes NoCompletePacket tail kind/bytes/offset and outer recovery start/count.
New inventory evidence is also non-exhaustive; four new constructor guards pass
(no failing-first claim). Twenty compile-fail doctests cover API extensibility.

Tests derive header facts from frozen 106-24R1 routing/layout evidence already
recorded in provenance; budget/completion rules are owner policy, not IRIG limits.
Local/CI commands and exact-head links belong in the PR. No version bump, release
workflow, publication, merge or next-slice work is authorized here.

PR8 review correction: tests-first 102464d reproduces a corrupt-checksum index
between valid packets: accepted offsets/recovery counts (0,0), (56,28), (84,0).
The index presence assertion fails at runtime (Absent instead of Unknown).
Inspection now retains one sticky recovery flag; clean EOF still completes the
scan, but unseen setup/index/events remain Unknown after any recovered bytes.
The later unrecovered packet checks that the flag is never cleared; previously
seen setup stays Present and accepted packet counts remain three. Separate
index-only and event-only complete scans verify independent routing and absence.
Two compiled mutation controls fail at runtime: replacing the sticky OR with
assignment, and swapping index/event routing. Production source restored; all
42 Rust tests and 20 compile-fail doctests pass, as do fmt and all-target clippy.
