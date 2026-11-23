# irig106-core

irig106-core 0.1.0 supplies structural packet traversal, typed
completion/errors, bounded inventory and per-channel sequence observations.
The sole dependency is irig106-types 0.1 (default features off), including
re-exported Rtc.

Header::parse checks primary sync/checksum/basic lengths on 24 bytes; it does
not establish a complete packet. PacketReader promotes irig106-time-cli's
reviewed streaming reader. Core owns packet
finding and basic framing; payload meaning stays with TMATS, time and decode.
Default recovery continues to EOF with bounded memory. The reader uses
typed errors: NoCompletePacket for EOF before any complete frame, original
I/O/structural causes and explicit optional caller caps (RecoveryLimit).
Secondary checksum/reserved evidence and DATA/filler/checksum ranges are now
exposed. Packet-data checksum contents are reported NotChecked. Use ReaderLimits::new()
or Default plus builder setters; evidence structs and Event::Tail are extensible.

See the [repository requirements and plan](https://github.com/TelemetryWorks/irig106-core/blob/main/docs/requirements.md)
and [development API](https://github.com/TelemetryWorks/irig106-core/blob/main/docs/api.md).

Inspection::new(InspectionLimits::new(channels, rows)) collects bounded header
facts without payload/history retention. Feed every Event, finish only at clean
EOF, and mark_incomplete on reader error/early stop. Missing records remain
Unknown on incomplete scans or after any recovery; Present means observed headers,
not valid content. Clean EOF after recovery does not establish absence.

observe_with_sequence returns one per-channel comparison without retaining a
history; 255-to-0 wraps, and discontinuities are observations rather than loss
estimates. See the [migration contract](https://github.com/TelemetryWorks/irig106-core/blob/main/docs/migration.md)
and `cargo run -p irig106-core --example inspect --locked < recording.ch10`.
Expected successors after skipped input are Unverified; skipped_since_previous
includes every stream recovery since that channel's prior packet.

CI verifies the packaged archive and file list, executes the inspect example,
and measures an isolated release process against a 32 MiB RSS ceiling on a
240 MiB generated fixture, including a 64 MiB packet and a 48 MiB recovery gap.
These probes are scoped regression evidence, not a general memory guarantee.
Scope and separate drain/recovery negative-control evidence are in
[slice 8](https://github.com/TelemetryWorks/irig106-core/blob/main/docs/verification-s08.md).
No payload meaning, recording-wide conformance, no_std/alloc or packet-data
checksum validation is supplied. Publishing preparation remains separately gated.
