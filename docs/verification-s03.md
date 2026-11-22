# Slice 3: bounded reader promotion and dependency policy

Base main 6eedf8d39a08a118d8ef6ff107ac1b99b26e5a91 (approved PR4).
Tests-first a876d83 imports five seed regressions; f7c7200 adds I/O/padding
controls before implementation. Missing reader API fails compilation (E0432),
not runtime. The new four-byte-length assertion already passes the base parser;
a compiled mutant dropping Data Length byte11 fails its value assertion.
Packet Length 0x04030228 replaces the owner's example 0x04030208 so DATA
0x04030201 fits after the 24-byte header. No packet-size conformance claim.

Seed: [time 5899806 input.rs](https://github.com/TelemetryWorks/irig106-time/blob/5899806ae684ed38d8cad8f422cabb3d47c6be0f/apps/irig106-time-cli/src/input.rs)
and [s16_reader.rs](https://github.com/TelemetryWorks/irig106-time/blob/5899806ae684ed38d8cad8f422cabb3d47c6be0f/apps/irig106-time-cli/tests/s16_reader.rs).
Preserves counted short reads/draining, capture clamp/cap, raw secondary bytes,
u64 offsets, terminal errors/tails and uncapped linear recovery. Replaces seed
Header/valid_header/length checks with the shared immutable Header::parse;
only sync/checksum errors search forward; checksum-qualified bad lengths stop.
No second header parser, payload interpretation, unsafe or new dependency.

Seven reader tests cover: 3200-byte real file/32-byte buffer/three-byte reads;
DATA-vs-filler; bit7/capture/zero-buffer bounds; recovery/cut tails/invalid
lengths; bad checksum at expected offset with exact28 recovered bytes; isolated
alignment; Interrupted in header/drain and terminal real I/O; >1MiB EOF padding.
These are partial L3-IO-002/CAP-002/REC-003/END-004/ERR-004 clauses only.
The seed's large ignored RSS worker is deferred to slice8's isolated CI probe;
no process-RSS or throughput result is claimed here.

Seven one-at-a-time mutants compiled and failed named seed tests: remove DATA
clamp; capture cap; no-stream guard; use bit2; remove DATA bound; skip checksum;
skip alignment. Sources restored, focused reader tests rerun green. The byte11
mutant is separate primary-field evidence (L3-HDR-001/L3-LEN-001), not a fix to
production extraction. Local/CI commands and exact-head results are in the PR.

The staged API retains seed io::Result, empty→None and first-cut→Tail behavior.
It cannot alone certify Chapter10 input: require a complete Packet event as the
time CLI does. Slice4 owns NoCompletePacket, optional caller recovery cap and
terminal scanned-byte evidence. Default recovery remains uncapped. Slice5 owns
secondary/trailer integrity; no whole-requirement verification is asserted.

cargo-deny policy matches time5899806 and tmats2400802, without TMATS prototype
advisory exemptions. No ignored advisories; permissive licence allowlist,
yanked/unmaintained checks, wildcard bans and crates.io-only sources. CI uses
the same SHA-pinned cargo-deny action and --locked --all-features arguments.
This tool gate is CI-only here; no local cargo-deny pass is claimed. No release,
publication, merge or next-slice work is authorized by this PR.
