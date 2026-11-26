# Core 0.2 slice 6: ordered consumer example and optional streaming contract

Base: main 64810f9, containing approved #23 head 5749b81. Tests-first commit
56a3274 adds independent stdin oracles; the initial build fails because example
`ordered` does not exist. No library code, inventory counters, read strategy,
dependency or version changes. No tag/publication is authorized or performed.

The new executable uses existing WholePacketReader/PacketBuf/Inspection APIs,
two workers, four reusable owners, FIFO channel routing and ordered control
revision notifications. It borrows DATA spans without payload conversion. These
notifications are not semantic setup closure or time decoding; INT-045 remains
an integration duty on the ordered path. The streaming document specifies the
approved optional provisional protocol without claiming its implementation.

`scripts/test_ordered_example.py` runs the built binary with independent
hand-packed/checksummed fixtures and a 15-second subprocess watchdog:

- Setup and time notifications precede dispatch; a channel spanning a control
  update keeps FIFO routing and carries the newer revision on subsequent work.
  Worker summaries confirm last revision, per-packet revision sums and borrowed
  DATA counts. A two-block setup frame is consumed through complete DATA spans.
- 64 packets exceed the four-slot working set repeatedly and complete with exact
  worker counts, sequence wrap is contiguous, and clean EOF finishes inventory.
- Recovery makes successor continuity Unverified and unseen presence Unknown.
- Padding/truncation after a complete packet retain accepted analysis, mark it
  incomplete and dispatch no partial packet. Empty input exits 3.
- An oversoft ordinary frame on stdin is refused without implicit stream opt-in;
  prior work is drained and retained, incomplete, with exit 3.

CI builds/runs the fixtures, checks the updated exact package file inventory,
formats/lints/tests/docs/packages, and retains the capture RSS gate and release
path dry run. Exact-head run IDs and executed local checks are recorded in the
PR after completion. No aggregate admission-evidence counter decision is assumed.

Temporary controls: replacing dispatched revisions with zero fails the independent
summary oracle; reducing the return queue to one reproduces the circular wait
and is killed by the 15-second subprocess watchdog. Both mutations were discarded
and the production example rebuilt before the final fixture run.


## Review correction: Time F2 and legacy setup notifications

The new setup → type0x12 time → data fixture fails at the original head: both
later packets still carry revision 1 and the time notification is absent. After
the fix, channels 0 and 3 both advance to revision 2 before dispatch and worker
revision sums prove that data received the updated snapshot. INT-045 recognizes
0x11 on any channel and 0x12 from 106-17; the example enables F2 for its known
24R1 edition policy, never for Unknown based on raw/version inference.

An example unit matrix exercises known/unknown edition routing, channel-0 setup,
nonzero-channel setup with explicit legacy policy on/off, and ordinary data with
legacy enabled. CI explicitly runs this example unit test before the subprocess
fixtures. Legacy notices use admission evidence; inventory setup_record remains
channel-0 only. Library code, admission counters and read strategy are unchanged.
