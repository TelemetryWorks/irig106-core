# Known-good registry fixture

`irig106-core-0.0.1.crate` is the published placeholder, downloaded once on
2026-11-23 from the crates.io version download endpoint. Its SHA-256 was checked
against the independently fetched version metadata checksum:

`0d532861736df546570291e7d9494869f27cd4e9cd75448d74563373804e73b3`

Size: 6263 bytes. The fixture and digest are committed; the dry run compares
served registry bytes with this fixture, not two fresh downloads. This is NOT
the current development candidate and must never be regenerated from checkout.
The public crate API reported `crate.trustpub_only=true`; publisher registration
and environment protections were not inspected or changed.

Sources: [version metadata](https://crates.io/api/v1/crates/irig106-core/0.0.1),
[download](https://crates.io/api/v1/crates/irig106-core/0.0.1/download).
