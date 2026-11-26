# Core 0.2 ordered consumers and optional streaming contract

## Executable whole-packet path

Run `cargo run -p irig106-core --example ordered --locked < recording.ch10`.
It uses the current whole-packet API, default H/B/hybrid policy, two fixed
workers, four reusable PacketBuf owners and one bounded FIFO per worker.
`channel % 2` keeps each channel on one FIFO. The producer observes every
PacketInfo exactly once, in recording order, before fan-out; metadata inspection
never waits for out-of-order worker results. No admission-evidence counters are
added to the inventory; that owner question remains pending.

The example notices channel0/type01 and type11 on the ordered path and attaches
an increasing control revision to each work item. **These are header routing
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

## Optional incremental delivery: contract, not an implemented API

Whole-packet delivery remains the default. L2-WHOLE-008's approved drain/chunk
options are explicit modes, never automatic fallbacks from H/B refusal, allocation
failure or deadline expiry. The following names describe protocol concepts, not
new public Rust types/methods; this slice implements no chunk/drain entry point.

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

Future executable chunk/drain coverage must pin byte/range continuity, no commit
on every abort class, explicit disposition and early-stop accounting, source
mutation, bounded chunk retention, deadlines and merger backpressure. These are
outstanding implementation/qualification obligations, not passing checks here.
