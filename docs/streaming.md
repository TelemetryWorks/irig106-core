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

## Optional incremental chunk delivery: contract, not an implemented API

Whole-packet delivery remains the default. L2-WHOLE-008's approved drain/chunk
options are explicit modes, never automatic fallbacks from H/B refusal, allocation
failure or deadline expiry. The following names describe protocol concepts, not
new chunk Rust types/methods. Explicit drain is implemented above; chunk delivery
remains outstanding.

- **Begin** follows primary structural validation and explicit admission. Carry
  packet identity/absolute offset, declared frame and DATA ranges, edition/limits,
  recovery and authorization evidence. It is a provisional record, not the
  complete-only PacketInfo exposed by PacketBuf today.
- **Chunk** carries identity, frame-relative range and received bytes; distinguish
  frame bytes from DATA intersections so secondary/filler/trailer are never
  silently decoded as DATA. Ranges are monotone, non-overlapping and byte-exact.
  A fixed, budgeted reusable quantum bounds live storage; borrowed bytes are valid
  only for the callback/lease lifetime. Asynchronous retention requires moving an
  accounted owner from a bounded pool, not an unbounded copy queue.
- **End** commits only after all declared bytes arrive and source-stability and
  structural checks complete. Carry final byte counts and integrity/admission
  evidence, including Unconfirmed(ExactEof) where applicable. It does not assert
  payload validity or upgrade packet-data checksum contents from NotChecked.
- **Abort** on truncation, source mutation, cancellation, deadline or operational
  error preserves received-byte/offset/recovery evidence and invalidates all
  provisional chunks. No complete Packet or successful End is emitted. Consumers
  must stage/rollback output or label it provisional; core cannot retract an
  irreversible decoder side effect. Interrupted retries preserve progress;
  WouldBlock remains terminal unless a future resumable API is separately agreed.
- **Rejected/Drained** reports policy reason, requested and actually consumed
  bytes, EOF/error/cancellation and disposition. It is an explicit omission, not a
  complete-packet inventory observation. A failed drain does not authorize sync
  recovery inside the refused body or pretend that a following frame was found.

Recovery scans precede Begin; garbage is never a payload chunk. No implicit
fallback from a failed seek probe to stream trust is permitted. All retained
chunks/leases and workspace remain budgeted; impossible requests fail rather than
wait, temporary pressure observes cancellation/deadlines. Output/context credits
must not form a cycle with input credits. The existing whole-packet API and 0.1
capture semantics remain unchanged.

Future executable chunk coverage must pin byte/range continuity, no commit
on every abort class, explicit disposition and early-stop accounting, source
mutation, bounded chunk retention, deadlines and merger backpressure. These are
outstanding implementation/qualification obligations, not passing checks here.


Q8 positioned refusal detail: `refused_header()` borrows the checked primary.
`position()` stays at frame offset + 24; body bytes are not logically consumed.
Fixed read-ahead may already hold payload bytes. Retrying whole delivery retries
the SAME frame and recovery evidence, never skips it. Only H/B/target size errors
are nonterminal; Uncorroborated, allocation, cancellation/deadline, source/I/O and
structural failures remain terminal. into_inner discards cached header/read-ahead;
stopping does not rewind the underlying transport. The cache is one fixed
metadata record charged through reader size at construction, never a body buffer.
