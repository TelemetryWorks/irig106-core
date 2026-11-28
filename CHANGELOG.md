# Changelog

## [Unreleased]

## [0.2.0] - 2026-11-28

- Add opt-in whole-packet delivery with reusable 512 KiB pooled blocks, borrowed
  spans, a caller-overridable 128 MiB hard ceiling and 256 MiB aggregate budget.
  Grow by received bytes with a bounded provisional block, never reserve from a
  declared Packet Length; preserve the 0.1 bounded-capture API and memory rule.
- Add explicit edition/size admission evidence, stable-source next-header probes,
  Unconfirmed(ExactEof), non-seekable oversoft opt-in and reported legacy setup
  allowance. Use one budgeted 64 KiB hybrid read-ahead buffer with configurable
  4 KiB small-body cutoff and direct large remainders.
- Add deadline/cancellation-aware pool backpressure, lazy warm-up and reuse,
  hysteresis/explicit trimming, ordered ownership-fan-out examples and aggregate
  admission-evidence inspection counters with explicit coverage.
- Verify 8/16/32-bit packet-data checksums incrementally over body plus filler by
  default in whole/chunk delivery, with an explicit speed opt-out. Report
  Verified/Mismatch evidence and inspection counts; mismatch never rejects by
  default. Capture retains Absent/NotChecked behavior.
- Add explicit budgeted coalescing, positioned size refusals followed by caller
  drain/stop, reported fixed-memory drain, and callback-scoped provisional chunks
  with Abort and a returned End commit. No automatic fallback, silent truncation
  or complete-packet inventory credit for drain/chunk reports.
- Qualify allocation reuse, 64-bit Linux/macOS/Windows memory and Windows i686
  functional limits, package verification and the unsigned publish dry run.
  RSS probes are scoped regression evidence, not a general memory guarantee;
  generated warm-cache benchmarks are informational, not real-recording claims.
- Keep Rust 2024/MSRV 1.85, safe std-only code and irig106-types 0.1 as the sole
  dependency; types 0.2 is not required. Payload interpretation, recording-wide
  conformance, async transports and no_std/alloc remain outside this release.

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
