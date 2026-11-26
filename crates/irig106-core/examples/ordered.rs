//! One ordered producer, channel-affine bounded workers, and returned owners.
//! Structural demonstration only: control notifications do not parse setup/time.
use irig106_core::{
    BlockPool, Inspection, InspectionLimits, PacketBuf, WholeEvent, WholePacketReader, WholePolicy,
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
                    if (info.header.channel() == 0 && info.header.data_type() == 1)
                        || info.header.data_type() == 0x11
                    {
                        revision = revision.checked_add(1).ok_or("revision overflow")?;
                        println!("control offset={} revision={revision}", info.offset);
                    }
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
