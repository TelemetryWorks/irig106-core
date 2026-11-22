# irig106-core

Unreleased development toward 0.1 provides `Header::parse`, `primary_checksum`,
`HeaderError` and the shared `Rtc` type. Parsing checks the first 24 bytes only:
sync, primary checksum and basic framing lengths. It does not establish a
complete packet. The manifest remains 0.0.1; no new version is published.

The approved 0.1 design promotes irig106-time-cli's bounded streaming reader.
Core will own streaming packet finding and structural checks; payload meaning stays with
TMATS, time and decode consumers. Default recovery continues to EOF with bounded
memory; only an explicit caller cap can cause `RecoveryLimit`. These streaming
APIs and trailer/secondary checks are not implemented yet.

See the [repository requirements and plan](https://github.com/TelemetryWorks/irig106-core/blob/main/docs/requirements.md)
and [proposed API](https://github.com/TelemetryWorks/irig106-core/blob/main/docs/api.md).
