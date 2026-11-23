# Core release pipeline

The repository uses the **base** [org standard at 2adedc5](https://github.com/TelemetryWorks/.github/blob/2adedc5aaf27b7bafb54ad05a51bc17d89b65d3e/docs/TRUSTED-PUBLISHING.md),
which retains the fcceb02 requirements, adopted in reviewed PR12.
The manifest selects 0.1.0. Release preparation requires independent exact-head
review, a passing PR dry run and owner merge. After the owner confirms Trusted
Publisher settings, the coordinator pushes the annotated tag and the owner
approves crates-io. This document does not authorize tagging, publication,
re-tagging or replacement of existing registry bytes.

Identity remains **TelemetryWorks/irig106-core**, **trusted-publishing.yml**,
environment **crates-io**, workflow name **Trusted publishing**. The virtual
workspace releases its single library at crates/irig106-core; changing that set
fails closed until ordering/membership are reviewed. Types 0.1 stays a registry
dependency, not part of core's release set.

The six base jobs are Release gate, Build, Checksums and attestations, Publish
to crates.io, GitHub release, Verify publication. A seventh read-only job is
Publish path (dry run). PR/manual routes emit publish=false, compile/package/
qualify in Build, then reproduce candidate bytes without compiling and compare
the pinned known-good published core 0.0.1. They do not sign, authenticate,
enter crates-io or publish, even for a manual dispatch from a tag. Missing future
release records are findings on these routes, fatal on tag pushes.

Build pins Rust 1.100.0, MSRV 1.85.0 and cargo-cyclonedx 0.5.9 --locked. Core
tests, examples, RSS, archive verification and release controls run before upload
approval, also on PRs. The actual build Rust version is an output used and
rechecked by non-compiling repackaging in Publish and Publish path (dry run).
The frozen payload contains the crate, CycloneDX SBOM and strict SHA256SUMS.

The signer rechecks checksums and event/gate/checkout identity, then uploads
provenance as a disjoint immutable `release-<version>-attestations` sidecar.
Publish, GitHub release and Verify download exactly payload plus sidecar.
Both crate and SBOM require `gh attestation verify --bundle`, binding repo,
workflow, tag, source digest, hosted runner and SLSA predicate. Verification
precedes repackaging, any upload decision/authentication, and any Release
creation/attachment. No payload overwrite or asset replacement is allowed.

Every privileged job independently requires a v* tag PUSH and publish=true;
all checkouts use github.sha, never a gate-supplied checkout ref. Gate/event/
checkout identity mismatches fail. The gate requires remote API annotation,
tag/manifest equality, main ancestry and successful intended CI on that exact
main commit. Release notes must match a dated changelog entry and be committed.

Registry upload gets a fresh OIDC credential for the single-crate wave, exposed
only on cargo publish --locked --no-verify. Existing versions are not uploaded;
their served bytes/checksum must still match. An existing placeholder cannot be
used for token bootstrap. Later verification compares actual attached assets,
cryptographic evidence, served registry bytes/checksum and successful docs.rs
builds; pending docs at the deadline fail. Latest is selected from a fresh
paginated semver release list: highest stable true; older stable and prerelease
false, with prerelease metadata set. No creation-time ordering or --clobber.

Before any separately authorized release, the owner verifies Trusted Publisher
registration for this exact filename/environment, trustpub_only=true, required
owner approval, v* TAG deployment eligibility and disabled admin bypass. YAML
guards are defense in depth; they do not create those server-side barriers.
Only public registry only-mode was observed here; private registration and
environment protection are not claimed verified. Prepare the version/lockfile,
dated changelog and docs/releases/vX.Y.Z.md in a reviewed release-preparation PR.
No tag, signing, upload, dispatch or settings change was executed by adoption.

Implementation lineage: [types #21 at 1e81bd7](https://github.com/TelemetryWorks/irig106-types/pull/21)
for pipeline, 49 controls, sidecar verification and semver policy, adapted to the
core workspace/placeholder and extended with source-identity/workflow contracts.
See [slice 9 evidence](verification-s09.md). This PR's unsigned candidate
comparisons are not a successful signature verification or a real release test.
