# Core 0.2 ordered consumers and optional streaming contract

## Executable whole-packet path

Run `cargo run -p irig106-core --example ordered --locked < recording.ch10`.
It uses the current whole-packet API, default H/B/hybrid policy, two fixed
workers, four reusable PacketBuf owners and one bounded FIFO per worker.
`channel % 2` keeps each channel on one FIFO. The producer observes every
PacketInfo exactly once, in recording order, before fan-out; metadata inspection
never waits for out-of-order worker results. Owner decision 3A (2026-11-26) adds
aggregate admission evidence through Inspection::admission_counts(); ordered
prints it after successful whole-packet observations. Capture events carry no
such evidence; evaluated_packets explicitly reports coverage.

The default 106-24R1 example notices channel0/type01 setup and time types 0x11
or 0x12 on **any channel**, attaching an increasing control revision to each
work item. INT-045 includes Time F2 / Network Time (0x12) from 106-17 onward;
24R1 is the only known EditionPolicy today, so Unknown never authorizes that
interpretation, even if its raw code or Data Type Version resembles an edition.
If the caller explicitly selects with_legacy_setup_any_channel(true), notices
also cover type01 on nonzero channels using the admission evidence. The default
does not infer legacy mode. Inventory setup_record presence remains **channel-0
only**, even under the legacy allowance; a nonzero-channel setup notification
does not change that structural presence rule. Classification is three-way:
Control advances the revision; Uncertain visibly reports offset, raw kind and
edition but keeps the prior revision and dispatches ordinary data; None dispatches
without a control notice. Uncertain covers 0x12 without an established 106-17+
edition and nonzero-channel type01 without the explicit legacy option. **These are header routing
notifications, not setup parsing, setup closure or interpreted time epochs.**
Downstream orchestration must perform setup closure (INT-045), validate/apply
setup changes and resolve time references/epochs here before publishing a bounded
immutable context snapshot to workers. All channels must see the applicable
snapshot, even when their own channel has no control packet. A mutable shared
"current context" looked up later in a worker can apply a future setup/time state
to earlier packets. Channel FIFO alone does not prevent that mistake.

Workers borrow DATA spans, retain scalar summaries, clear input storage and
return the same owners. They do not clone payloads or infer packet meaning.
Single-block packets are contiguous; consumers of multi-block packets use spans.
No automatic coalescing, filtering or reordered inspection is demonstrated.
Inspection retains its existing caller-set entry limits and cold allocations.
The example's worker/channel/context/output storage is orchestrator state outside
input pool B and must stay separately bounded; core does not supply a scheduler.

The return queue fits **all four slots**, including during a blocked source read
or shutdown. A worker clears input credits before publishing a result, and never
waits for a merger to release input. Ordinary returns preserve warm pool capacity;
rare large returns/explicit pressure trim can require re-warm. Queue count bounds
alone cannot replace B when packet sizes differ. Pool admission has a five-second
absolute wait deadline here, and free-owner receive has a five-second timeout.
Neither interrupts an arbitrary blocking Read; a live transport needs its own
bounded-read/cancellation policy. Workers do no external blocking work in this
demonstration. Production orchestrators need their own cancellation and join policy.

Only complete packets are dispatched. On a reader/inspection error, retained
analysis is incomplete and exit is 3; after an established stream a tail is
reported with retained packets, incomplete status and exit 0 (illustrative CLI
policy). Clean EOF alone calls finish. Worker failure also marks incomplete.
Shutdown closes input queues and drains accepted work before joining; the return
queue capacity prevents a join/return cycle. The example has no result-reordering
buffer; applications needing global output order need a separate bounded merger.

Pipes use `WholePacketReader::stream`; standard-size packets work with defaults,
oversoft packets require explicit caller stream opt-in. This example deliberately
does not enable it. WouldBlock/errors after partial input are terminal, not an
invitation to restart at the physical cursor. StableSource seek probes and
ExactEof authorization require the adapter's stability contract. UDP needs bounded
transport reassembly, ordering and loss policy outside core; a datagram boundary
is not a core packet boundary.

## Explicit drain implementation

`reader.drain_next(absolute_deadline)` explicitly omits the next structurally
framed packet. It never happens as a fallback from next_packet_into. Owner Q8
(2026-11-28) retains the validated header on HardLimit/BudgetTooSmall/TargetLimit
refusals so drain_next can explicitly consume that frame or the caller can stop. DrainReport records the validated header/declared ranges,
requested/actually consumed frame bytes (including primary, excluding recovery),
reason and Drained/Truncated disposition. DrainError retains typed Error and an
Aborted progress report when a valid header was available. Framing tails stay
separate DrainEvent::Tail events. All evidence types are non-exhaustive.

Drain uses fixed 4 KiB scratch already charged at reader construction, with no
packet blocks or assembly permit. It can discard above H/B and on 32-bit targets
without sizing storage from P. It reports the whole-delivery refusal reason but
performs no whole-admission/corroboration or checksum-content verification.
Cancellation/deadlines cover recovery and Interrupted retries cooperatively;
blocking transport I/O still needs adapter-level bounds. Source mutation aborts.
A completed drain resumes only at the declared boundary; errors/EOF terminate
without searching the body for sync. Completed drain establishes framing only.

Every report is an explicit omission, never PacketInfo or an ordinary inventory
observation. Stop/mark incomplete any unfiltered Inspection; later packets belong
to a separately declared retained scope, not a complete-recording inventory.
Omitted setup/time may invalidate orchestrator context; no setup closure or time
continuity may be inferred from draining their headers. Drained payload bytes
cannot enter worker queues. See [drain verification](verification-v02-drain.md).

## Explicit callback-scoped chunk delivery

`reader.next_chunks(absolute_deadline, sink)` is opt-in, never a fallback from
whole delivery. The sink accepts `ChunkEvent::Begin`, `Chunk` and `Abort` and
returns `Result<(), E>`. `ChunkOutcome::End` is returned separately and is the
**only commit point**; no callback follows commit. Empty/terminal input returns
None; framing tails use ChunkOutcome::Tail (or NoCompletePacket before any
completed transfer). These reports are distinct from PacketInfo and cannot feed
complete-packet Inspection counters. Mark an unfiltered inventory incomplete or
keep explicitly separate scopes when mixing modes.

Begin follows a checked primary and H/edition soft authorization. NextHeader,
Unconfirmed(ExactEof), explicit StreamOptIn and legacy evidence retain their
whole-mode meanings. Whole-frame B/isize storage feasibility is unnecessary in
this fixed-memory mode, including after a positioned whole BudgetTooSmall or
32-bit TargetLimit refusal. H and absolute-offset overflow still refuse before
Begin and retain the header; explicit drain can omit such a frame instead.
There is no fallback from a failed probe to stream trust. Known-stable-short
input keeps the approved truncation-only path: fixed drain, tail, **no Begin**.

Each PacketChunk borrows at most **4,096 bytes** for this callback only. The
primary is the first 24-byte chunk. Frame-relative ranges partition the received
frame exactly; `data_range`/`data` contain only their DATA intersection, excluding
secondary/filler/trailer. Recovery precedes Begin and never becomes payload.
The fixed scratch plus 1,024 bytes of bounded transfer/control metadata are
charged at reader construction, alongside existing read-ahead. There are no
packet blocks, assembly leases, credit waits or per-transfer allocations. The
callback and source own their own bounded state outside B; async retention would
need a separately bounded accounted copy, never a retained callback borrow.

End means all declared bytes arrived and were acknowledged by callbacks, with
final source/cancellation/deadline checks. It carries final ranges, secondary
integrity and admission evidence. Q7 checksums verify incrementally as chunks
arrive (default ON; opt-out NotChecked; absent Absent). A mismatch is final
integrity evidence, not automatic refusal. End does not claim payload validity,
produce PacketInfo or advance the complete-packet inventory. Consumers may apply
staged setup/time state only after End and their own semantic validation.

Any failure after an attempted Begin attempts Abort and returns ChunkError.
Truncation, I/O (including WouldBlock), mutation, cancellation, deadline and sink
rejection invalidate **all** provisional output; no End exists. Before the first
completed transfer truncation retains typed NoCompletePacket; after establishment
it is ChunkFailure::Truncated. Received progress includes primary and actual
logical reads, offered progress includes a rejected chunk, and delivered progress
counts successful callback acknowledgements, not semantic consumption. A read
error can leave received > offered. All counts exclude recovery. Original sink E
is preserved even if Abort notification fails (abort_sink_error). Begin rejection
also attempts Abort with zero offered bytes. Panics propagate with the reader
terminal; Rust unwinding cannot guarantee an Abort callback.

Stage/rollback output or visibly label it provisional: core cannot undo external
side effects. Cancellation/deadlines are checked around recovery, reads,
Interrupted retries and callbacks; they cannot preempt a blocking Read/Seek or
callback. Operational errors are terminal; only pre-Begin positioned size
refusals are resumable. No body rescan follows abort. Output/context ownership
must never depend on returning input credits; the callback API itself holds no
packet-credit lease. Whole delivery, coalescing, drain and 0.1 capture semantics
remain unchanged. See [chunk verification](verification-v02-chunks.md).


Q8 positioned refusal detail: `refused_header()` borrows the checked primary.
`position()` stays at frame offset + 24; body bytes are not logically consumed.
Fixed read-ahead may already hold payload bytes. Retrying whole delivery retries
the SAME frame and recovery evidence, never skips it. A log-and-continue
error loop will spin on that refusal unless it explicitly calls drain_next or
stops (or selects an authorized explicit chunk transfer). Only H/B/target size errors
are nonterminal; Uncorroborated, allocation, cancellation/deadline, source/I/O and
structural failures remain terminal. into_inner discards cached header/read-ahead;
stopping does not rewind the underlying transport. The cache is one fixed
metadata record charged through reader size at construction, never a body buffer.
