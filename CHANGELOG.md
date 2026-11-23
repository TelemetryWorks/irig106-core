# Changelog

## [Unreleased]

## [0.1.0] - 2026-11-23

- Add Chapter 10 primary-header/checksum parsing, structural length checks,
  secondary-header integrity evidence and DATA/filler/checksum ranges using
  the 106-24R1 Chapter 11 layout.
- Provide a streaming, bounded packet reader with caller-limited DATA capture,
  uncapped default sync recovery, optional recovery caps, typed errors and
  completion/tail evidence that preserves prior analysis.
- Add bounded counts, raw data-type and presence facts, and per-channel sequence
  observations with wrap and recovery-qualified continuity.
- Reuse irig106-types 0.1 and its Rtc type. Target std, Rust edition 2024 and
  MSRV 1.85. Packet-data checksum contents are reported NotChecked; payload
  interpretation, checksum-content validation and no_std/alloc support remain
  outside 0.1.
- Add reviewed Trusted Publishing preparation, verified package checks and
  CI-only isolated RSS probes as scoped regression evidence, not a general
  memory guarantee.
- 0.1.0 supersedes the 0.0.1 name placeholder.
