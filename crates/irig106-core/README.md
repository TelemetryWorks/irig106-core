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
Secondary/trailer integrity checks remain slice 5.

See the [repository requirements and plan](https://github.com/TelemetryWorks/irig106-core/blob/main/docs/requirements.md)
and [proposed API](https://github.com/TelemetryWorks/irig106-core/blob/main/docs/api.md).
