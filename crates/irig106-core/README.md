# irig106-core

Unreleased development toward 0.1 provides `Header::parse`, `primary_checksum`,
`HeaderError` and the shared `Rtc` type. Parsing checks the first 24 bytes only:
sync, primary checksum and basic framing lengths. It does not establish a
complete packet. The manifest remains 0.0.1; no new version is published.

Slice 3 promotes irig106-time-cli's bounded streaming reader. Core owns packet
finding and basic framing; payload meaning stays with TMATS, time and decode.
Default recovery continues to EOF with bounded memory. The reader uses
typed errors: NoCompletePacket for EOF before any complete frame, original
I/O/structural causes and explicit optional caller caps (RecoveryLimit).
Secondary checksum/reserved evidence and DATA/filler/checksum ranges are now
exposed. Packet-data checksum contents remain NotChecked. Use ReaderLimits::new()
or Default plus builder setters; evidence structs and Event::Tail are extensible.

See the [repository requirements and plan](https://github.com/TelemetryWorks/irig106-core/blob/main/docs/requirements.md)
and [proposed API](https://github.com/TelemetryWorks/irig106-core/blob/main/docs/api.md).

Inspection::new(InspectionLimits::new(channels, rows)) collects bounded header
facts without payload/history retention. Feed every Event, finish only at clean
EOF, and mark_incomplete on reader error/early stop. Missing records remain
Unknown on incomplete scans or after any recovery; Present means observed headers,
not valid content. Clean EOF after recovery does not establish absence.
