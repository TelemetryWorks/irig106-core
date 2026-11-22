# irig106-core

Chapter 10 packet finding and structural checking for the TelemetryWorks IRIG 106
ecosystem. Unreleased development toward 0.1 currently provides a primary
header/checksum parser and the promoted bounded reader under
`crates/irig106-core`. Typed completion/recovery-cap refinements remain slice 4;
the manifest remains 0.0.1 and no new version is published.

The virtual workspace uses edition 2024, MSRV 1.85 and Apache-2.0. It currently
has one library. Published `irig106-types` 0.1 (defaults disabled) is its sole
dependency, supplying the shared `Rtc`; no payload interpretation is performed.

- [Requirements and nine-slice plan](docs/requirements.md)
- [Proposed API and boundary behavior](docs/api.md)
- [Seed provenance and slice 1 verification](docs/provenance.md)
- [Organization contributor guidance](https://github.com/TelemetryWorks/.github/blob/main/CONTRIBUTING.md)

Development checks (Python 3 and stable Rust; no Python dependencies):

```sh
python3 scripts/test_workspace.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

CI also checks Rust 1.85, Linux/macOS/Windows, docs, packaged-source builds and
`cargo deny check` (deny.toml: advisories, licences, bans and registry-only sources).
Use light focused local checks with `CARGO_BUILD_JOBS=2`; large generated-input
RSS checks belong to CI in slice 8. No throughput or memory measurement is
claimed by this scaffold. no_std/alloc and packet-data checksum validation are
tracked follow-ups, not 0.1 features.

No release is authorized. Slice 9 must adopt the organization
[Trusted Publishing standard](https://github.com/TelemetryWorks/.github/blob/main/docs/TRUSTED-PUBLISHING.md)
before the first real release; publishing remains OIDC-only with owner approval.
