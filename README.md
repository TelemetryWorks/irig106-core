# irig106-core

Chapter 10 packet finding and structural checking for the TelemetryWorks IRIG 106
ecosystem. Unreleased development toward 0.1 currently provides a primary
header/checksum parser and the promoted bounded reader under
`crates/irig106-core`, with typed completion/errors and optional caller recovery
caps (default None), secondary integrity evidence and checked trailer ranges.
Packet-data checksum contents remain NotChecked. The manifest is still 0.0.1;
no new version is published. Configure ReaderLimits with new()/Default and builders.
Bounded Inspection adds complete-frame totals, raw type/channel rows and
completion-qualified presence facts with explicit channel/row budgets.
Per-channel sequence comparisons include wrap and preserve recovery evidence;
discontinuities do not prove packet loss.
An expected successor after skipped input is Unverified; per-channel recovery
intervals include skips attached to other channels.

The virtual workspace uses edition 2024, MSRV 1.85 and Apache-2.0. It currently
has one library. Published `irig106-types` 0.1 (defaults disabled) is its sole
dependency, supplying the shared `Rtc`; no payload interpretation is performed.

- [Requirements and nine-slice plan](docs/requirements.md)
- [Development API and boundary behavior](docs/api.md)
- [Consumer migration contract and examples](docs/migration.md)
- [Seed provenance and slice 1 verification](docs/provenance.md)
- [Organization contributor guidance](https://github.com/TelemetryWorks/.github/blob/main/CONTRIBUTING.md)

Development checks (Python 3 and stable Rust; no Python dependencies):

```sh
python3 scripts/test_workspace.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

CI also checks Rust 1.85, Linux/macOS/Windows, docs, exact package file lists,
verified packaged-source builds, executed public examples and
`cargo deny check` (deny.toml: advisories, licences, bans and registry-only sources).
Use light focused local checks with `CARGO_BUILD_JOBS=2`. The CI-only
[isolated RSS gate](docs/verification-s08.md) measures a release reader/inventory
process on a 240 MiB fixture with a 32 MiB ceiling, including a complete 64 MiB
packet (4-byte capture) and a 48 MiB mid-file recovery gap. Separate one-off
controls check packet-sized draining and recovery retention. This is scoped regression evidence, not a universal memory
or throughput guarantee. no_std/alloc and packet-data checksum validation are
tracked follow-ups, not 0.1 features.

No release is authorized. Slice 9 must adopt the organization
[Trusted Publishing standard](https://github.com/TelemetryWorks/.github/blob/main/docs/TRUSTED-PUBLISHING.md)
before the first real release; publishing remains OIDC-only with owner approval.
