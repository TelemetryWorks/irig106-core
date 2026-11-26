# Core 0.2 slice 7: resource, platform and performance qualification

Base main 2be781b, containing approved #24 aa576d6. No library, policy, dependency,
version or publishing changes. Chunk/drain/coalescing and admission counters are
excluded from this assignment; their outstanding owner decisions are unchanged.
Tests-first 6d3e3e8 records the missing resource-validator failure and target-limit
oracles. Exact-head CI results and one-off control run IDs are recorded in the PR.

## Isolated whole-reader controls

CI builds `whole_resources` in release mode BEFORE measurement. Synthetic Read/
Seek input holds only scalar state and a 24-byte generated header; body bytes
really arrive and are touched in final pooled blocks, without a file-sized source
Vec masking reader memory. This is not a storage throughput benchmark. Every
checkpoint flushes PID/accounting then waits for host acknowledgement; Windows
samples the still-live process. Collection, acknowledgements and joins are bounded.

| Case | Independent oracle | Ceiling |
| --- | --- | --- |
| Cold/tiny bogus | Whole reader construction has zero payload blocks. Ordinary P=128 MiB, H/B defaults and explicit stream opt-in, only 28 physical bytes: typed NoCompletePacket/TruncatedPacket, no delivered packet, at most one retained block. | 32 MiB |
| Oversoft admitted | 64 MiB ordinary frame, valid next primary header at declared end: NextHeader evidence, 128 complete spans, exact DATA bytes, next frame offset and EOF. | 80 MiB |
| Maximum setup | 128 MiB channel0/type01, default H/B, WithinSoft, 256 complete spans and exact DATA; next frame and EOF. | 144 MiB |
| Oversoft refused | Same declared 64 MiB frame but invalid following header; Uncorroborated, zero payload blocks. | 32 MiB |
| Budget pressure | B=16 MiB, three held 4 MiB packets; fourth build waits at 31 allocated blocks/zero idle, used <= B. Returning one wakes completion; explicit trim retains eight. | 32 MiB |

Large gates allow payload plus a fixed 16 MiB process/allocator/stack margin;
small cases retain the 32 MiB ceiling. These are scoped falsification thresholds,
not a general memory guarantee. Pool accounting is independently asserted and
is not equated to process memory. Ordinary-return hysteresis remains unchanged.
Linux GNU time wraps the prebuilt core process alone and records lifetime peak
resident pages. Reserved but untouched capacity is invisible to RSS.

Windows PowerShell runs outside the measured process. At each held checkpoint it
refreshes private committed bytes/working set plus lifetime peak paged bytes and
peak working set; all values must be positive and within the same case ceiling.
PeakPagedMemorySize64 corresponds to Page File Bytes Peak, i.e. peak process
commit, not only physical residency: [Microsoft property documentation](https://learn.microsoft.com/en-us/dotnet/api/system.diagnostics.process.peakpagedmemorysize64),
[process memory counters](https://learn.microsoft.com/en-us/windows/win32/api/psapi/ns-psapi-process_memory_counters_ex).
Allocator/OS headroom is still necessary; no protection from external OOM kills
is claimed. No OOM/commit settings are changed.

## Coverage and remaining scope

- L3-GROW/RSS/PLAT: new whole-mode gates supplement, never replace or raise, the
  existing capture-mode 32 MiB/240 MiB-file gate. Cold/bogus, real oversize, refusal,
  max setup and pressure are separate processes, so one path cannot mask another.
- L3-BUD/HOT: new pressure measurement complements existing deterministic deadline,
  wake/cancel, four-worker zero-warm-allocation, trimming and recovery controls.
- 32-bit Windows CI executes the workspace under i686-pc-windows-msvc. A new
  target-sized-limit test refuses isize-exceeding pool/read-ahead configurations
  without changing credits; on 32-bit, aligned P=0xFFFFFFFC returns TargetLimit
  before payload allocation despite explicit H/stream authorization.
- Performance uses the existing release-mode real-file direct/default-hybrid
  comparison, 28/400/4096/65536/524288-byte frames, now 32 MiB per case and median
  of three alternating-order warm-cache samples. Source reads are calibrated to
  Linux kernel read syscalls. Report packets/s and MB/s; no timing gate or strategy
  change, and no real-recording/geometric-refill claim.

API/requirements coverage is partial until owner-held chunk/drain/coalescing
scope is resolved; admission-evidence aggregation remains pending. Coalescing's
L3-SETUP obligation is not discharged by the accepted 128 MiB segmented case.
No release candidate, version bump, tag or publication is authorized by this PR.
