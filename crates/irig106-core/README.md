# irig106-core

This package is still the **0.0.1 placeholder**. Its `add` function is template
code, not a telemetry API. No packet parser is implemented yet.

The approved 0.1 design promotes irig106-time-cli's bounded streaming reader.
Core will own packet finding and structural checks; payload meaning stays with
TMATS, time and decode consumers. Default recovery continues to EOF with bounded
memory; only an explicit caller cap can cause `RecoveryLimit`.

See the [repository requirements and plan](https://github.com/TelemetryWorks/irig106-core/blob/main/docs/requirements.md)
and [proposed API](https://github.com/TelemetryWorks/irig106-core/blob/main/docs/api.md).
