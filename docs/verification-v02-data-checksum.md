# Core 0.2 packet-data checksum verification

Owner 2026-11-26 assignment; default ON in whole mode, explicit
`WholePolicy::with_data_checksum_verification(false)` for throughput-sensitive
consumers who accept NotChecked evidence. Detecting corruption by default is
preferable to quietly propagating unexamined content. No mismatch rejects a
packet, initiates recovery or validates payload meaning/authenticity.

106-24R1 §11.2.1.4, printed p. 11-11, states:
> Stated another way, the data checksum includes everything in the packet body plus all added filler.

[Chapter 11, p. 11-11](https://www.trmc.osd.mil/wiki/download/attachments/335389015/chapter11.pdf?api=v2#page=19)
selects arithmetic sums of bytes/words/longwords from flags bits 1–0. The preceding
sentence excludes primary header, optional secondary header and stored checksum;
the little-endian layout is Table 11-2. Computation covers
`header_length..ranges.checksum.start`, including filler, with width-modular sums.

Each arriving final pooled-block span contributes once during body acquisition,
before the next block is obtained. There is no assembled-packet re-traversal or
payload copy for checksumming. Fixed four-byte carry handles arbitrary span
boundaries; aligned frame/trailer lengths leave no partial word at completion.
Only the final 1/2/4 stored bytes are read at completion. Absent/disabled modes
skip accumulation. Fixed stack state fits existing charged scratch allowance;
hybrid I/O, pooling, source checks and cancellation/deadlines are unchanged.

`PacketInfo.ranges.data_checksum` reports Verified {width,value} or
Mismatch {width,stored,computed}; widths are bytes. Coalescing preserves it.
Capture retains 0.1 Absent/NotChecked behavior even with complete capture.
No complete packet means no verified outcome: truncation/error clears PacketBuf.
Inspection::data_checksum_counts partitions all accepted observations into absent,
not_checked, verified and mismatch, with checked atomic updates and no new rows.
Both inspect and ordered example summaries append `data_checksums=...`;
inspect is still capture-only and therefore reports no verified contents.

Independent evidence:
- Hand-known sums for every width; weighted-byte oracle independent of production
  word-chunk accumulation, patterned multi-block DATA, filler and odd lengths.
- Exclude both headers; mutate DATA/filler/stored checksum separately; mismatch
  delivers the frame, continues at the next offset, and never triggers recovery.
- Default-on, explicit opt-out, absence, capture coverage, coalesced evidence,
  short/Interrupted reads, direct/hybrid paths and fixed-carry quantum matrix.
- Truncation never yields verified metadata; four inventory-counter overflow
  paths retain all prior counts and mark incomplete.
- Existing warmed allocator test now includes widths 8/16/32 and still allocates
  zero per packet. Existing memory ceilings remain unchanged.

The release-mode benchmark shares the existing bounded file harness: 32 MiB per
size and width, median of three alternating on/off runs, same actual adaptive
hybrid, warm file cache, generation/construction/first block initialization
excluded. Five sizes (28/400/4096/65536/524288), all widths; source/kernel read
counts are recorded and checksum work is black-boxed. At P=28 width32 the body
coverage is empty, measuring per-packet overhead. This is informational scoped
cost evidence, not a timing gate or a claim about cold disk/network workloads.
Ten temporary controls fail with assertion panics/exit101: default-off,
ignore opt-out, omit filler, include secondary header, big-endian words, always
Verified, omit later blocks, omit width masking, misroute mismatch counter and
drop partial-word carry. All mutations were discarded. Exact-head CI run IDs
and benchmark table are recorded in the PR.
No merge, version bump, tag or publication is authorized.
