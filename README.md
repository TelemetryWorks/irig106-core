# irig106-core

Version 0.2.0 provides structural Chapter 10 packet traversal using the
106-24R1 Chapter 11 layout. It retains the bounded DATA-prefix reader and adds
whole-packet ownership, explicit admission evidence and optional coalescing,
drain and callback-scoped chunks. Payload interpretation stays downstream.

Whole mode uses reusable 512 KiB blocks, a default 128 MiB hard ceiling H and
256 MiB shared storage budget B, both caller-overridable. Storage grows on receipt
with one bounded provisional block, never by reserving the declared length.
One fixed 64 KiB read-ahead buffer serves headers/recovery and small bodies;
large remainders read directly into pooled storage. The configurable small-body
cutoff defaults to 4 KiB. Warmed ordinary reuse is allocation-free in scoped tests;
new inventory entries and caller-owned queues/output have separate bounds.

Packet-data checksums default ON for whole/chunk delivery, with an explicit speed
opt-out. Verified/Mismatch are integrity evidence; mismatch does not reject by
default. Capture remains Absent/NotChecked. Recovery has no default search cap;
optional caller caps return typed errors. Presence and per-channel sequence facts
retain recovery uncertainty. Observe complete PacketInfo once on the ordered
path; chunk/drain results are distinct, never complete-packet inventory input.

The virtual workspace has one library at crates/irig106-core, Rust edition 2024,
MSRV 1.85 and Apache-2.0. Published `irig106-types ^0.1` (default features off,
locked to 0.1.0) supplies `Rtc` and is the sole dependency. **Core 0.2 does not
require types 0.2.** No no_std/alloc, payload decoding or full conformance claim.

- [Requirements](docs/requirements.md) and [0.2 coverage audit / open limits](docs/coverage-0.2.md)
- [API contract](docs/api.md), [migration](docs/migration.md), [ordered/streaming consumers](docs/streaming.md)
- [Design](docs/design-0.2.md), [seed provenance](docs/provenance.md), [release notes](docs/releases/v0.2.0.md)
- [Organization contributor guidance](https://github.com/TelemetryWorks/.github/blob/main/CONTRIBUTING.md)

Light local checks (Python 3, Rust; no Python dependencies):

```sh
CARGO_BUILD_JOBS=2 python3 scripts/test_workspace.py
cargo fmt --all -- --check
CARGO_BUILD_JOBS=2 cargo clippy --workspace --all-targets --locked -- -D warnings
CARGO_BUILD_JOBS=2 cargo test --workspace --locked
python3 -m unittest discover -s scripts/release -p 'test_*.py'
```

CI covers MSRV, Linux/macOS/Windows, executed Windows i686 tests, public examples,
package inventory/archived-source verification, dependency policy and workflow
audits. Heavy memory qualification stays in CI: capture retains its 32 MiB RSS
gate; whole-mode small cases use 12 MiB, with separately justified 24/80/144/272
MiB pressure/large/coalesced ceilings. These probes are scoped regression evidence,
not a general memory guarantee; RSS sees touched resident pages, not unwritten
reserved capacity. See the [coverage audit](docs/coverage-0.2.md).

The [Trusted Publishing pipeline](docs/RELEASING.md) qualifies and verifies the
0.2.0 candidate and unsigned publish path, retaining the pinned published 0.0.1
known-good fixture. Preparing this version does not authorize publication.
Tagging, signing, crates-io approval and publication require the owner's release
decision; no local token upload or settings change is part of this PR.
