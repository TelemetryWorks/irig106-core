//! One ordered producer, channel-affine bounded workers, and returned owners.
//! Structural demonstration only: control notifications do not parse setup/time.
use irig106_core::{
    BlockPool, EditionPolicy, Inspection, InspectionLimits, PacketBuf, PacketInfo, WholeEvent,
    WholePacketReader, WholePolicy,
};
use std::{
    error::Error,
    io,
    process::ExitCode,
    sync::mpsc::{Receiver, SyncSender, sync_channel},
    thread,
    time::{Duration, Instant},
};
const WORKERS: usize = 2;
const SLOTS: usize = 4;
const WAIT: Duration = Duration::from_secs(5);
struct Work {
    packet: PacketBuf,
    revision: u64,
}
#[derive(Default)]
struct Summary {
    packets: u64,
    data_bytes: u64,
    revision: u64,
    revision_sum: u64,
}
#[derive(Debug, PartialEq, Eq)]
enum ControlKind {
    Control,
    Uncertain,
    None,
}
fn is_control(info: &PacketInfo) -> ControlKind {
    match info.header.data_type() {
        0x01 if info.header.channel() == 0 || info.admission.legacy_setup_any_channel => {
            ControlKind::Control
        }
        0x01 => ControlKind::Uncertain,
        0x11 => ControlKind::Control, // Time F1, any channel.
        // Time F2 exists from 106-17. 24R1 is our only known edition policy;
        // Unknown (including Unknown(17)) never earns this meaning by inference.
        0x12 if matches!(info.admission.edition, EditionPolicy::Irig106_24R1) => {
            ControlKind::Control
        }
        0x12 => ControlKind::Uncertain,
        _ => ControlKind::None,
    }
}
fn report_control(
    info: &PacketInfo,
    revision: &mut u64,
    out: &mut impl io::Write,
) -> io::Result<()> {
    match is_control(info) {
        ControlKind::Control => {
            *revision = revision
                .checked_add(1)
                .ok_or_else(|| io::Error::other("revision overflow"))?;
            writeln!(out, "control offset={} revision={revision}", info.offset)
        }
        ControlKind::Uncertain => writeln!(
            out,
            "uncertain-control offset={} kind={:#04x} edition={:?}",
            info.offset,
            info.header.data_type(),
            info.admission.edition
        ),
        ControlKind::None => Ok(()),
    }
}
fn worker(input: Receiver<Work>, returned: SyncSender<PacketBuf>) -> io::Result<Summary> {
    let mut summary = Summary::default();
    for Work {
        mut packet,
        revision,
    } in input
    {
        if revision < summary.revision {
            return Err(io::Error::other("control revision went backwards"));
        }
        // A real decoder borrows these spans; no payload Vec or result queue.
        let bytes = packet.data_spans().map(|span| span.len() as u64).sum();
        summary.data_bytes = summary
            .data_bytes
            .checked_add(bytes)
            .ok_or_else(|| io::Error::other("worker byte count overflow"))?;
        summary.packets = summary
            .packets
            .checked_add(1)
            .ok_or_else(|| io::Error::other("worker packet count overflow"))?;
        summary.revision_sum = summary
            .revision_sum
            .checked_add(revision)
            .ok_or_else(|| io::Error::other("revision sum overflow"))?;
        summary.revision = revision;
        packet.clear(); // Return input credits BEFORE publishing any result.
        returned
            .send(packet)
            .map_err(|_| io::Error::other("return receiver closed"))?;
    }
    Ok(summary)
}
fn run() -> Result<bool, Box<dyn Error>> {
    let pool = BlockPool::with_defaults()?;
    let mut free = Vec::with_capacity(SLOTS);
    for _ in 0..SLOTS {
        free.push(PacketBuf::new(pool.clone())?);
    }
    // The default 24R1 example assumes channel-0 setup. An explicit
    // with_legacy_setup_any_channel(true) also enables nonzero-channel notices
    // through admission evidence; never infer that option from packet contents.
    let mut reader = WholePacketReader::stream(io::stdin().lock(), WholePolicy::new(), pool)?;
    let mut inventory = Inspection::new(InspectionLimits::new(1024, 4096));
    // All four slots can be returned even while the producer is blocked reading.
    let (returned, returns) = sync_channel(SLOTS);
    let mut senders = Vec::with_capacity(WORKERS);
    let mut workers = Vec::with_capacity(WORKERS);
    for _ in 0..WORKERS {
        let (send, receive) = sync_channel(1);
        let returned = returned.clone();
        senders.push(send);
        workers.push(thread::spawn(move || worker(receive, returned)));
    }
    drop(returned);
    let mut revision = 0u64;
    let scan = (|| -> Result<(), Box<dyn Error>> {
        loop {
            let mut packet = match free.pop() {
                Some(packet) => packet,
                None => returns.recv_timeout(WAIT)?,
            };
            match reader.next_packet_into(&mut packet, Instant::now() + WAIT)? {
                Some(WholeEvent::Packet) => {
                    let info = packet.info().ok_or("missing complete metadata")?;
                    let sequence = inventory.observe_packet_info(info)?;
                    // Downstream setup closure (INT-045) and time-epoch updates
                    // belong HERE, before fan-out, not independently in workers.
                    report_control(info, &mut revision, &mut io::stdout().lock())?;
                    // Uncertain controls are visible, but retain the previous
                    // context revision and still follow ordinary data dispatch.
                    let route = info.header.channel() as usize % WORKERS;
                    println!(
                        "dispatch offset={} channel={} worker={route} revision={revision} sequence={:?}",
                        info.offset,
                        info.header.channel(),
                        sequence.kind()
                    );
                    // Same channel -> same FIFO. This scalar stands in for a
                    // bounded immutable context snapshot, never a mutable global.
                    senders[route].send(Work { packet, revision })?;
                }
                Some(WholeEvent::Tail { tail, recovery, .. }) => {
                    inventory.mark_incomplete();
                    eprintln!("terminal tail: {tail:?}, recovery={recovery:?}");
                    break;
                }
                Some(_) => return Err("unsupported whole event".into()),
                None => {
                    inventory.finish();
                    break;
                }
            }
        }
        Ok(())
    })();
    let mut failed = scan.is_err();
    if let Err(error) = scan {
        inventory.mark_incomplete();
        eprintln!("{error}");
    }
    // No more dispatch; workers drain accepted packets then exit. The return
    // queue fits ALL slots, so joining never depends on draining worker results.
    drop(senders);
    for (id, handle) in workers.into_iter().enumerate() {
        match handle.join() {
            Ok(Ok(summary)) => println!(
                "worker={id} packets={} data_bytes={} revision={} revision_sum={}",
                summary.packets, summary.data_bytes, summary.revision, summary.revision_sum
            ),
            _ => {
                inventory.mark_incomplete();
                failed = true;
                eprintln!("worker {id} failed");
            }
        }
    }
    println!(
        "packets={} status={:?} presence={:?}",
        inventory.totals().packets,
        inventory.status(),
        inventory.presence()
    );
    Ok(failed)
}
fn main() -> ExitCode {
    match run() {
        Ok(false) => ExitCode::SUCCESS,
        Ok(true) => ExitCode::from(3),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(3)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn control_notifications_obey_edition_and_explicit_legacy_policy() {
        use ControlKind::{Control, None, Uncertain};
        // INT-045: F1 on any channel; F2 from 106-17 (known 24R1 here).
        // The independent header fixture has P=28, DATA=4, no secondary/trailer.
        for (channel, kind, edition, legacy, expected) in [
            (0u16, 0x01, EditionPolicy::Irig106_24R1, false, Control),
            (7, 0x01, EditionPolicy::Irig106_24R1, false, Uncertain),
            (7, 0x01, EditionPolicy::Irig106_24R1, true, Control),
            (7, 0x09, EditionPolicy::Irig106_24R1, true, None),
            (7, 0x11, EditionPolicy::Unknown(7), false, Control),
            (0, 0x12, EditionPolicy::Irig106_24R1, false, Control),
            (7, 0x12, EditionPolicy::Irig106_24R1, false, Control),
            (7, 0x12, EditionPolicy::Unknown(17), false, Uncertain),
        ] {
            let mut bytes = [0u8; 28];
            bytes[..2].copy_from_slice(&[0x25, 0xeb]);
            bytes[2..4].copy_from_slice(&channel.to_le_bytes());
            bytes[4..8].copy_from_slice(&28u32.to_le_bytes());
            bytes[8..12].copy_from_slice(&4u32.to_le_bytes());
            bytes[12] = 0xff; // Data-type version must not select the edition.
            bytes[15] = kind;
            let sum = bytes[..22].chunks_exact(2).fold(0u16, |s, w| {
                s.wrapping_add(u16::from_le_bytes([w[0], w[1]]))
            });
            bytes[22..24].copy_from_slice(&sum.to_le_bytes());
            let policy = WholePolicy::new()
                .with_edition(edition)
                .with_legacy_setup_any_channel(legacy);
            let pool = BlockPool::with_defaults().unwrap();
            let mut packet = PacketBuf::new(pool.clone()).unwrap();
            let mut reader = WholePacketReader::stream(Cursor::new(bytes), policy, pool).unwrap();
            reader
                .next_packet_into(&mut packet, Instant::now() + WAIT)
                .unwrap();
            assert_eq!(
                is_control(packet.info().unwrap()),
                expected,
                "channel={channel} kind={kind:#x} edition={edition:?} legacy={legacy}"
            );
            let mut revision = 5;
            let mut output = Vec::new();
            report_control(packet.info().unwrap(), &mut revision, &mut output).unwrap();
            match expected {
                Control => {
                    assert_eq!(revision, 6);
                    assert_eq!(output, b"control offset=0 revision=6\n");
                }
                Uncertain => {
                    assert_eq!(revision, 5);
                    assert_eq!(
                        String::from_utf8(output).unwrap(),
                        format!(
                            "uncertain-control offset=0 kind={kind:#04x} edition={edition:?}\n"
                        )
                    );
                }
                None => {
                    assert_eq!(revision, 5);
                    assert!(output.is_empty());
                }
            }
        }
    }
}
