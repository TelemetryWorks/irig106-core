# Core 0.2 slice 5: pooled reuse and whole-packet inventory

Base: main e7523d1, following reviewed adaptive-refill head bf97621 (#22).
Tests-first commit 5119740 specifies the metadata bridge (initial E0599 for the
missing method) and the exact-cutoff previous_large boundary. Only inspection
production code changes: capture and whole metadata share the same count,
presence, limit and sequence implementation. No reader/pool/refill policy change,
dependency, version or publishing change.

| Evidence | Scope |
| --- | --- |
| Whole/capture parity | Five hand-packed packets, one skipped byte attached to another channel, exact offsets/counts/type rows/presence and 254→255→0 sequence wrap. Both APIs return identical observations; unseen setup remains Unknown after recovery even at clean EOF. |
| Limits/completion | New-channel and new-row refusals preserve prior counts/entries and stop observation; finish cannot erase failure. A whole-reader tail requires the consumer to mark incomplete and preserves unseen Unknown facts. |
| Ordered producer/four workers | Five bursts of 16 complete packets, bounded ownership channels, borrowed DATA checks, buffer return/reuse. Warm rounds retain 16 blocks despite an eight-block trim baseline. A real nine-block setup return trims to eight; one re-warm adds eight blocks, subsequent bursts reuse them. |
| Scoped allocations | Thread-local allocation counters surround core reader/inspection calls and worker span/clear operations. Three warmed bursts allocate zero; construction, first-entry/block growth, rare setup, re-warm and channel bookkeeping are excluded. Logical totals and sequence wrap are checked. |
| Exact-cutoff boundary | A short initial read ends precisely after a 4,096-byte body; the next empty-boundary header request is 24 bytes. Changing previous_large from >= to > fails the request assertion (N3, exit 101). |
| Additional controls | Dropping recovery forwarding fails whole/capture parity; trimming every ordinary return fails the worker reuse test (both exit 101). All temporary mutations were discarded. |

Worker receives and joins are deadline-bounded, and whole tests have the existing
process watchdog. Counts distinguish cold/trim-induced allocation from the warm
claim. No payload copies are added by the inspection bridge. Capture API behavior
and its isolated 32 MiB RSS gate remain unchanged.

Input pool accounting includes its existing workspace and buffers. Inspection's
existing consumer BTreeMaps use explicit entry limits and cold-allocate first
entries outside pool B; this PR does not introduce a process-wide memory budget
or fallible-map guarantee. Explicit coalescing, chunk/drain delivery and broader
resource/platform qualification remain outstanding. Exact-head CI and read-only
publish dry-run run IDs are recorded in the PR after completion.
