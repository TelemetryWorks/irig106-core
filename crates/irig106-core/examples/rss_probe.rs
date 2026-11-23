//! CI memory-probe driver; GNU time measures this process, never Cargo or Python.
use irig106_core::{Event, Inspection, InspectionLimits, PacketReader, ReaderLimits, SequenceKind};
use std::{error::Error, fs::File};

fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::args_os().nth(1).ok_or("expected fixture path")?;
    let limits = ReaderLimits::new()
        .with_buffer_bytes(4096)
        .with_capture_bytes(4);
    let mut reader = PacketReader::new(File::open(path)?, limits)?;
    let mut inventory = Inspection::new(InspectionLimits::new(1, 1));
    let (mut recoveries, mut recovered, mut tail, mut tail_scanned, mut unverified) =
        (0u64, 0u64, 0u64, 0u64, 0u64);
    while let Some(event) = reader.next_packet(|_| 4)? {
        if let Event::Packet(p) = &event {
            assert_eq!(p.body_prefix, [1, 2, 3, 4]);
        }
        if let Some(sequence) = inventory.observe_with_sequence(&event)? {
            if sequence.recovered_bytes > 0 {
                recoveries += 1;
                recovered += sequence.recovered_bytes;
            }
            if sequence.kind() == SequenceKind::Unverified {
                unverified += 1;
            }
        } else if let Event::Tail {
            bytes, recovery, ..
        } = event
        {
            tail += bytes;
            tail_scanned += recovery
                .ok_or("expected tail recovery evidence")?
                .scanned_bytes;
        }
    }
    inventory.finish();
    let c = inventory.totals();
    println!(
        "packets={} data_bytes={} packet_bytes={} recoveries={recoveries} recovered={recovered} tail={tail} tail_scanned={tail_scanned} unverified={unverified} status={:?}",
        c.packets,
        c.data_bytes,
        c.packet_bytes,
        inventory.status()
    );
    Ok(())
}
