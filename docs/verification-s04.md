# Slice 4: typed completion and optional recovery budget

Base main 4122bf69ea790df0d6acd23beaaba7aac699398b (approved PR5).
Tests-first 14a48ce: six completion tests fail compilation for missing typed
API/fields, not runtime failures. Source/capture identity remains the promoted
S16b reader; no new wire interpretation or dependency. Version remains0.0.1.

New tests trace to L3-END-004/REC-003/ERR-004: every byte cut of primary-only
and secondary-bearing first frames, with capture0/4, yields NoCompletePacket;
after a complete frame, every cut remains a tail. Empty/junk-only input is not
success. Complete EOF returns None. Short semantic DATA is still a frame.
Caps accept candidates at N, reject N+1, support zero, default to None, and
reset per incident. Fixed24-byte lookahead is excluded: 27 junk bytes gives
scanned3/tail27. Recovery evidence survives EOF, malformed recovered headers,
real read failures and recovered packet tails. Claimed truncated bodies are
not rescanned for embedded valid sync. Existing >1MiB padding test remains.

Error.reason distinguishes I/O, HeaderError, NoCompletePacket, RecoveryLimit,
InvalidBuffer, CaptureLimit, Allocation and OffsetOverflow. Error.position is
consumed input, excluding read-ahead; Error.recovery records the current incident.
NoCompletePacket retains optional Tail metadata. HeaderError downcasts survive;
standard source() also preserves I/O/header causes. Other failures take priority
over no-complete classification. All errors/tails are terminal then None.

The extra caller-limit/source-chain controls and private offset-overflow unit
arrived with implementation; no failing-first claim. Offset test sets private
position to u64::MAX without reading a huge file. Actual allocation failure is
not forced. Seed first-cut fixtures now prepend a complete frame deliberately;
new exhaustive first-cut tests verify the corresponding fatal classification.
No old typed structural-stop, filler-I/O or recovered-tail assertions are removed.

Six compiled runtime mutants fail named completion tests: accept empty input;
return first cuts as tails; cap off-by-one; ignore cap; fail to reset incident;
omit scanned count. Source restored; focused tests rerun green. Local commands,
exact-head CI and independent review belong in the PR. CI owns platform/MSRV,
package and cargo-deny checks; no large RSS/fuzz campaign is run locally.

An explicit cap exhausted on a rejected candidate returns RecoveryLimit before
another read, even if that read would observe EOF. No default cap is added;
uncapped recovery remains linear and memory-bounded. Every terminal tail/error
now retains recovery evidence; secondary/trailer checking remains slice5.
No merge, publication, release workflow or next-slice work is authorized here.
