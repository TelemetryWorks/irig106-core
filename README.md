# irig106-core

Chapter 10 packet finding and structural checking for the TelemetryWorks IRIG 106
ecosystem. **Current code is still the 0.0.1 placeholder**, moved unchanged into
`crates/irig106-core`; the approved 0.1 reader is not implemented yet.

The virtual workspace uses edition 2024, MSRV 1.85 and Apache-2.0. It currently
has one library and no dependencies. Published `irig106-types` 0.1 is the only
approved ecosystem dependency, to be introduced with the header API.

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

CI also checks Rust 1.85, Linux/macOS/Windows, docs and packaged-source builds.
Use light focused local checks with `CARGO_BUILD_JOBS=2`; large generated-input
RSS checks belong to CI in slice 8. No throughput or memory measurement is
claimed by this scaffold. no_std/alloc and packet-data checksum validation are
tracked follow-ups, not 0.1 features.

No release is authorized. Slice 9 must adopt the organization
[Trusted Publishing standard](https://github.com/TelemetryWorks/.github/blob/main/docs/TRUSTED-PUBLISHING.md)
before the first real release; publishing remains OIDC-only with owner approval.
