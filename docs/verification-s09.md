# Slice 9: base Trusted Publishing adoption

This is the final planned implementation slice, as ONE adoption PR. It does not
authorize tagging, publication, registry changes or environment-policy changes.
The version remains 0.0.1; 0.1.0 is the owner's later decision. The only changelog
entry added is Unreleased; future dated release records are intentionally absent.
Base main: c8db7fab62a7465444c21e32132ee32ef3b5173e (approved PR10).

## Pinned source and adaptations

- [Org standard 2adedc5](https://github.com/TelemetryWorks/.github/blob/2adedc5aaf27b7bafb54ad05a51bc17d89b65d3e/docs/TRUSTED-PUBLISHING.md)
  retains fcceb02's base requirements; the later diff only clarifies the archived
  ch10-reader rollout. Read in full, including owner-verification boundaries.
- [Types #21, head 1e81bd7](https://github.com/TelemetryWorks/irig106-types/pull/21)
  supplies reviewed workflow/control code and 49 Python tests. Core adapts names,
  the fixed one-library virtual workspace, package manifest paths, qualification
  commands and the published 0.0.1 fixture. No other repository was modified.
- Tests-first f7a794a cannot import the missing pipeline.py (FileNotFoundError).
  This is a harness-import failure, not a runtime signature/publishing failure.
  Core also adds event/gate/checkout identity checks and workflow-contract tests;
  those extensions were added with implementation, not failing first.

## Contract and evidence boundary: L3-REL-007

The six base ids/names/edges/permissions and the seventh read-only dry-run job
match the standard. Every privileged job independently requires tag PUSH plus
publish=true. Checkouts use github.sha. Source identity mismatches fail before
signing or provenance checks. No gate output supplies a checkout ref.

Payload and attestation sidecar upload lists are explicit and disjoint; immutable
payloads are never overwritten. Downstream jobs merge exactly the two artifact
names. Signature validation binds each crate/SBOM to repo, workflow, tag, source
digest, hosted runner and SLSA predicate. It runs before authentication/upload
decisions and before Release creation/attachment. Latest uses fresh paginated
semantic versions; older stable/prerelease uploads cannot take Latest. Matching
existing assets/registry bytes are retained; mismatches fail without replacement.

55 stdlib Python controls cover checksums/unsafe manifests/symlinks, CI identity,
remote annotation, tag movement, notes, semver, missing/invalid provenance,
verify-before-repackage and verify-before-attach, existing assets/registry
comparison, docs.rs pending failure, workspace membership, immutable checkouts,
permissions and dry-run guards. Privileged APIs/signature CLI are mocked in
these tests; no successful real signing or publish-path mutation is claimed.

The candidate dry run packages/builds source and generates a CycloneDX SBOM with
Rust1.100.0 and cargo-cyclonedx0.5.9, then rechecks exact Rust before non-compiling
repackaging and byte comparison. Separate known-good registry comparison uses
the committed [0.0.1 fixture](../scripts/release/fixtures/PROVENANCE.md), pinned
SHA256 0d532861736df546570291e7d9494869f27cd4e9cd75448d74563373804e73b3.
It is not rebuilt from development source or compared with another fresh
download. Public only-mode was observed true; private publisher/environment
protections were not inspected. Owner checks remain in [RELEASING](RELEASING.md).

The PR route must pass Release gate, Build and Publish path (dry run), with all
four privileged jobs skipped. Exact-head runs/results belong in the adoption PR.
Unsigned comparisons are not signature validation. No manual dispatch/tag was
performed to obtain evidence. Known-good public HTTP comparison passed locally.

Local checks use cached light Rust checks and the Python controls. Large RSS,
SBOM compilation and archive/candidate dry runs run in CI. actionlint1.7.12 and
zizmor1.33.1 are exact upstream binaries with pinned SHA-256, installed outside
the crate; CI audits all workflows and additionally audits trusted-publishing.yml
with auditor persona. New publishing workflow has no auditor findings; baseline
CI's toolchain-action informational suggestions are not suppressed by repo config.
The RSS doc now states that untouched reserved memory is invisible to resident
page measurements. No production library, dependency or release version change.
