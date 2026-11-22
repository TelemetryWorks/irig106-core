# Slice 5: secondary evidence, trailer ranges and extensible API

Base main 2bbe127f14f30824c3c054c1b1e0fad7b080e611 (approved PR6).
Tests-first ea97994: five API guard doctests FAIL because construction/exhaustive
matching still compiles, not because of a Rust compilation error. New integrity
integration tests separately fail compilation for missing evidence/builders and
ErrorKind Display. No production-runtime failing-first claim for new APIs.

Oracle: archived 106-24R1 Ch11 PDF SHA-256 remains
0f63792dcac39f6010df371ee0afc46a396017918f4155cba9ebab6c2d6d263e;
re-read §11.2.1.2 c and §§11.2.1.4/11.2.1.1 g (printed11-9..12/PDF17..20).
Literal bytes1..10 yield byte0037 and LE-word1E19; five FFFF words yield FFFB.
RDG-008's suspect byte reading remains selected; alternative word-only matches
and raw reserved word8..9 remain evidence. No time conversion/format inference.

Tests cover byte-only/word-only/both/neither matches, reserved evidence, word
wrapping, raw bytes/primary RTC retention, widths0/1/2/4 with/without secondary,
disjoint DATA/filler/checksum ranges, DATA-only capture, physical trailer EOF,
insufficient space stopping before capture, and u32 range arithmetic using only
a 24-byte allocation. Variable filler is intentional: §11.2.1.4 also allows it
to preserve channel packet size; filler content validation is outside this slice.
Packet-data checksum contents remain explicitly NotChecked/Absent, never Valid.
L3-SEC-005/CHK-005 clause evidence only; no semantic/full-recording conformance.

Coordinator API decision: non_exhaustive on Error/Recovery/Tail/ReaderLimits,
Packet, Event::Tail and both new evidence structs; Header/PacketReader already
have private fields. Eight compile-fail doctests exercise external construction
or exhaustive Tail matching. Existing regressions migrate to builders/field
reads/`..` without deleting prior assertions. Builders keep defaults8192/65536/
None and defer zero-buffer rejection to the reader. Human-readable Display tests
cover every ErrorKind and the outer Error, with deterministic injected I/O text.

The largest-range control, physical trailer-EOF probe and three new-evidence
API guards were added during implementation; no failing-first claim for them.
Six compiled runtime mutants fail named tests: include stored bytes in sum;
wrong word endian; wrong reserved offset; select word instead of byte; wrong
checksum width; allow DATA/checksum overlap. Source restored; focused tests rerun.
Local commands/results and exact-head CI belong in the PR. No process-OOM/RSS
campaign, dependency, version bump, publication, merge or next-slice work.
