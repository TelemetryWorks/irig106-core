# Slice 2: primary header/checksum primitive

Base: main 11ea305bfb027350ab9688ff91fafdb7e0b8d513 (PR3 merged).
Tests-first ea6f7ae: six Rust tests cannot compile on the base because the public
header API and types dependency are absent (E0432/E0433); this is **not** a
runtime parser regression. The updated workspace dependency assertion also errors
because no dependency exists yet; the other two workspace checks pass.

## Independent source and oracle

Inspected 106-24R1 Chapter11, January2025, §11.1 little-endian order
(printed11-1/PDF9), Table11-2 (printed11-3/PDF11),
§11.2.1.1 a/c/d/e/g/i/j (printed11-5..11-10/PDF13..18) and §11.2.1.2's
12-byte secondary size. PDF13/16/18 were also rendered and visually inspected.
[Archived chapter](https://github.com/TelemetryWorks/rcc-106-standards/releases/download/106-24R1/chapter11.pdf),
[manifest at 1071999](https://github.com/TelemetryWorks/rcc-106-standards/blob/10719995d63426c2fe62d1548acb21ae0238b44c/manifest.toml).
Downloaded PDF SHA-256 matched the manifest:
`0f63792dcac39f6010df371ee0afc46a396017918f4155cba9ebab6c2d6d263e`.

The literal DISTINCT fixture's eleven words sum to 0x28A91, hence checksum
0x8A91. Eleven FFFF words sum to 0xAFFF5, hence 0xFFF5. Neither expectation
uses production code or an encoder. Structural mutations are hand-packed with
an independent wider scalar checksum expression, so checksum rejection cannot
hide length tests. Unknown version/type values deliberately test raw retention,
not compliant content. Zero-DATA and arbitrary-flag vectors isolate framing;
they are not claims of fully conforming packets.

## Test/requirement ledger

| Test in tests/primary_header.rs | Clause evidence |
| --- | --- |
| literal_header_preserves_fields_and_shared_rtc | L3-HDR-001 field offsets/LE values; raw-code clause L3-EDN-005; direct irig106_types::Rtc type identity |
| checksum_wraps_excludes_stored_word_and_covers_every_primary_word | L3-HDR-001 literal modulo sums, stored-word exclusion, every non-sync covered byte/checksum byte |
| short_sync_and_checksum_failures_are_typed | L3-HDR-001 lengths0..23, reversed sync and exact stored/computed evidence |
| all_flags_use_bit7_alone_for_secondary_presence | L3-HDR-001 and primary-presence clause L3-SEC-005; all256 flag values, no semantic dispatch |
| checksum_qualified_lengths_isolate_minimum_alignment_and_data_bounds | L3-LEN-001 24/36 minimum, upper length words, zero/alignment/data edges, u32 extremes; no offset arithmetic claim |
| primary_parse_does_not_require_body_or_read_following_bytes | Primary-only/unaligned input and following bytes ignored; not stream/EOF verification |

After implementation all six focused tests and three workspace/package checks
pass. Four one-at-a-time mutants compiled and failed their named runtime tests:
skip checksum comparison; use bit2 for secondary presence; skip alignment;
saturate checksum addition. Source restored and six focused tests rerun green.
The old template add API/test is removed. Dependency is registry types0.1.0
locked, defaults disabled; no other dependency, I/O, unsafe or parser allocation.

Local: stable1.100.0/aarch64, focused tests, workspace contracts, fmt and touched
Clippy; final workspace test command/result is recorded in the PR. CI supplies
MSRV1.85, platform tests, rustdoc and archived-source packaging at the exact head.
No streamed packet, recovery cap, secondary checksum, trailer validation, packet
maximum/nonempty-DATA rule or full-edition conformance is claimed. Those remain
outside this primitive. No throughput/RSS result, publication or release workflow.

Final inspection added positive nonzero upper length-word assertions within the
ordinary packet maximum. They pass the implementation; no failing-first claim
for this added control.
