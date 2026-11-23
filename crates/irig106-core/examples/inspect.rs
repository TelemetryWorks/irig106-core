//! Stream stdin with explicit inventory budgets; retain totals on read failure.
use irig106_core::{Inspection, InspectionLimits, PacketReader, ReaderLimits, SequenceKind};
use std::{error::Error, io, process::ExitCode};

fn main() -> ExitCode {
    let mut inventory = Inspection::new(InspectionLimits::new(1024, 4096));
    let limits = ReaderLimits::new().with_capture_bytes(0); // Header facts only.
    let mut reader = match PacketReader::new(io::stdin().lock(), limits) {
        Ok(reader) => reader,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(3);
        }
    };
    let failure: Option<Box<dyn Error>> = loop {
        match reader.next_packet(|_| 0) {
            Ok(Some(event)) => match inventory.observe_with_sequence(&event) {
                Ok(Some(s)) => match s.kind() {
                    SequenceKind::Duplicate | SequenceKind::Discontinuity => eprintln!(
                        "channel {} at {}: {:?}, previous {:?}, observed {}, recovered {} bytes",
                        s.channel,
                        s.offset,
                        s.kind(),
                        s.previous,
                        s.observed,
                        s.recovered_bytes
                    ),
                    _ => {}
                },
                Ok(None) => eprintln!("terminal tail: {event:?}"),
                Err(error) => break Some(Box::new(error)),
            },
            Ok(None) => {
                inventory.finish();
                break None;
            }
            Err(error) => {
                inventory.mark_incomplete();
                break Some(Box::new(error));
            }
        }
    };
    let counts = inventory.totals();
    println!(
        "packets={} data_bytes={} packet_bytes={} status={:?} presence={:?}",
        counts.packets,
        counts.data_bytes,
        counts.packet_bytes,
        inventory.status(),
        inventory.presence()
    );
    if let Some(error) = failure {
        eprintln!("{error}");
        ExitCode::from(3)
    } else {
        ExitCode::SUCCESS
    }
}
