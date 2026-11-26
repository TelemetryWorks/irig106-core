//! CI-only whole-reader memory probe. Synthetic input holds no recording buffer.
use irig106_core::*;
use std::{
    error::Error as StdError,
    io::{self, Cursor, Read, Seek, SeekFrom, Write},
    sync::mpsc::sync_channel,
    thread,
    time::{Duration, Instant},
};
const MIB: usize = 1024 * 1024;
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(30)
}
fn header(length: u32, kind: u8) -> [u8; 24] {
    let mut b = [0u8; 24];
    b[..2].copy_from_slice(&[0x25, 0xeb]);
    b[4..8].copy_from_slice(&length.to_le_bytes());
    b[8..12].copy_from_slice(&(length - 24).to_le_bytes());
    b[15] = kind;
    let sum = b[..22].chunks_exact(2).fold(0u16, |s, w| {
        s.wrapping_add(u16::from_le_bytes([w[0], w[1]]))
    });
    b[22..24].copy_from_slice(&sum.to_le_bytes());
    b
}
struct Synthetic {
    p: u32,
    count: u32,
    kind: u8,
    next: Option<bool>,
    pos: u64,
}
impl Synthetic {
    fn size(&self) -> u64 {
        u64::from(self.p) * u64::from(self.count) + if self.next.is_some() { 28 } else { 0 }
    }
}
impl Read for Synthetic {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.size() {
            return Ok(0);
        }
        let main = u64::from(self.p) * u64::from(self.count);
        let (relative, p, kind) = if self.pos < main {
            (self.pos % u64::from(self.p), self.p, self.kind)
        } else {
            (self.pos - main, 28, 9)
        };
        let boundary = if relative < 24 { 24 } else { u64::from(p) };
        let n = out.len().min((boundary - relative) as usize);
        if relative < 24 {
            let mut b = header(p, kind);
            if self.pos >= main && self.next == Some(false) {
                b[0] = 0;
            }
            out[..n].copy_from_slice(&b[relative as usize..relative as usize + n]);
        } else {
            out[..n].fill(0x5a);
        }
        self.pos += n as u64;
        Ok(n)
    }
}
impl Seek for Synthetic {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let pos = match from {
            SeekFrom::Start(n) => i128::from(n),
            SeekFrom::Current(n) => i128::from(self.pos) + i128::from(n),
            SeekFrom::End(n) => i128::from(self.size()) + i128::from(n),
        };
        self.pos = u64::try_from(pos).map_err(|_| io::Error::other("invalid seek"))?;
        Ok(self.pos)
    }
}
impl StableSource for Synthetic {
    fn source_stamp(&mut self) -> io::Result<SourceStamp> {
        Ok(SourceStamp::new(self.size(), 1, 0))
    }
}
fn checkpoint(phase: &str, pool: &BlockPool) -> Result<(), Box<dyn StdError>> {
    let s = pool.stats()?;
    assert!(s.used_bytes <= s.budget_bytes);
    println!(
        "phase={phase} pid={} budget={} used={} blocks={} idle={} coalesced={}",
        std::process::id(),
        s.budget_bytes,
        s.used_bytes,
        s.allocated_blocks,
        s.idle_blocks,
        s.coalesced_bytes
    );
    io::stdout().flush()?;
    let mut ack = [0]; // Host samples the STILL-LIVE process, then releases it.
    io::stdin().read_exact(&mut ack)?;
    assert_eq!(ack, [b'+']);
    Ok(())
}
struct Fill(u64);
impl Read for Fill {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let n = out.len().min(self.0 as usize);
        out[..n].fill(0xff);
        self.0 -= n as u64;
        Ok(n)
    }
}
fn main() -> Result<(), Box<dyn StdError>> {
    let case = std::env::args().nth(1).ok_or("expected case")?;
    let pool = BlockPool::new(
        if matches!(case.as_str(), "pressure" | "pressure-baseline") {
            16 * MIB
        } else if case == "coalesce" {
            272 * MIB
        } else {
            DEFAULT_POOL_BUDGET
        },
    )?;
    let mut buf = PacketBuf::new(pool.clone())?;
    if case == "drain" {
        let p = 64 * MIB as u32;
        let source = Synthetic {
            p,
            count: 1,
            kind: 9,
            next: Some(true),
            pos: 0,
        };
        let mut reader = WholePacketReader::stream(
            source,
            WholePolicy::new().with_hard_limit(MIB as u32),
            pool.clone(),
        )?;
        checkpoint("cold", &pool)?;
        let Some(DrainEvent::Report(d)) = reader.drain_next(deadline())? else {
            panic!("drain report")
        };
        assert_eq!(
            (d.requested_bytes, d.consumed_bytes),
            (u64::from(p), u64::from(p))
        );
        assert_eq!(d.disposition, DrainDisposition::Drained);
        assert!(matches!(d.reason, DrainReason::HardLimit { .. }));
        assert_eq!(pool.stats()?.allocated_blocks, 0);
        checkpoint("drained", &pool)?;
        assert!(matches!(
            reader.next_packet_into(&mut buf, deadline())?,
            Some(WholeEvent::Packet)
        ));
        assert_eq!(buf.info().unwrap().offset, u64::from(p));
        checkpoint("resumed", &pool)?;
        assert!(reader.next_packet_into(&mut buf, deadline())?.is_none());
    } else if case == "recovery" {
        let gap = 48 * MIB as u64;
        let mut small = [0x5a; 28];
        small[..24].copy_from_slice(&header(28, 9));
        let input = Cursor::new(small)
            .chain(Fill(gap))
            .chain(Cursor::new(small))
            .chain(Fill(gap));
        let mut reader = WholePacketReader::stream(input, WholePolicy::new(), pool.clone())?;
        checkpoint("cold", &pool)?;
        for recovered in [0, gap] {
            assert!(matches!(
                reader.next_packet_into(&mut buf, deadline())?,
                Some(WholeEvent::Packet)
            ));
            let info = buf.info().unwrap();
            assert_eq!(info.recovered_bytes, recovered);
            assert_eq!(info.offset, if recovered == 0 { 0 } else { gap + 28 });
        }
        checkpoint("recovered", &pool)?;
        assert!(matches!(reader.next_packet_into(&mut buf, deadline())?,
            Some(WholeEvent::Tail {tail, recovery: Some(r), ..}) if tail.bytes == gap && r.scanned_bytes == gap - 24));
        assert!(buf.info().is_none());
        checkpoint("tail", &pool)?;
        assert!(reader.next_packet_into(&mut buf, deadline())?.is_none());
    } else if case == "bogus" {
        let mut bytes = [0x5a; 28];
        bytes[..24].copy_from_slice(&header(128 * MIB as u32, 9));
        let mut reader = WholePacketReader::stream(
            Cursor::new(bytes),
            WholePolicy::new().with_stream_opt_in(true),
            pool.clone(),
        )?;
        checkpoint("cold", &pool)?;
        let err = reader.next_packet_into(&mut buf, deadline()).unwrap_err();
        assert!(
            matches!(err.reason, ErrorKind::NoCompletePacket { tail: Some(t), .. }
            if t.kind == TailKind::TruncatedPacket && t.bytes == 28)
        );
        assert!(buf.info().is_none());
        assert!(pool.stats()?.allocated_blocks <= 1);
        checkpoint("short", &pool)?;
    } else if matches!(case.as_str(), "pressure" | "pressure-baseline") {
        let source = Synthetic {
            p: 4 * MIB as u32,
            count: 4,
            kind: 9,
            next: None,
            pos: 0,
        };
        let mut reader = WholePacketReader::stream(
            source,
            WholePolicy::new().with_stream_opt_in(true),
            pool.clone(),
        )?;
        checkpoint("cold", &pool)?;
        if case == "pressure-baseline" {
            return Ok(()); // Identical B/reader construction, no packet allocation.
        }
        let mut held = Vec::new(); // Three fixed owners, not recording history.
        for _ in 0..3 {
            let mut packet = PacketBuf::new(pool.clone())?;
            assert!(matches!(
                reader.next_packet_into(&mut packet, deadline())?,
                Some(WholeEvent::Packet)
            ));
            held.push(packet);
        }
        let (send, receive) = sync_channel(1);
        let worker = thread::spawn(move || {
            let result = reader.next_packet_into(&mut buf, deadline());
            send.send((buf, result)).unwrap();
        });
        let until = Instant::now() + Duration::from_secs(5);
        while pool.stats()?.waiting == 0 {
            assert!(Instant::now() < until, "reader never reached budget wait");
            thread::yield_now();
        }
        assert_eq!(pool.stats()?.allocated_blocks, 31);
        checkpoint("blocked", &pool)?;
        held.pop().unwrap().clear(); // Credits do not depend on a result merger.
        let (mut packet, result) = receive.recv_timeout(Duration::from_secs(5))?;
        assert!(matches!(result?, Some(WholeEvent::Packet)));
        let until = Instant::now() + Duration::from_secs(5);
        while !worker.is_finished() {
            assert!(Instant::now() < until);
            thread::yield_now();
        }
        worker.join().map_err(|_| "worker panic")?;
        packet.clear();
        drop(held);
        pool.trim_for_pressure()?;
        assert_eq!(pool.stats()?.idle_blocks, 8);
        checkpoint("trimmed", &pool)?;
    } else {
        let setup = matches!(case.as_str(), "setup" | "coalesce");
        assert!(setup || case == "admit" || case == "refuse");
        let p = if setup {
            128 * MIB as u32
        } else {
            64 * MIB as u32
        };
        let source = Synthetic {
            p,
            count: 1,
            kind: if setup { 1 } else { 9 },
            next: Some(case != "refuse"),
            pos: 0,
        };
        let mut reader = WholePacketReader::seek_checked(source, WholePolicy::new(), pool.clone())?;
        checkpoint("cold", &pool)?;
        if case == "refuse" {
            assert!(matches!(
                reader.next_packet_into(&mut buf, deadline()),
                Err(Error {
                    reason: ErrorKind::Admission {
                        source: AdmissionError::Uncorroborated,
                        ..
                    },
                    ..
                })
            ));
            assert_eq!(pool.stats()?.allocated_blocks, 0);
            checkpoint("refused", &pool)?;
        } else {
            assert!(matches!(
                reader.next_packet_into(&mut buf, deadline())?,
                Some(WholeEvent::Packet)
            ));
            let info = buf.info().unwrap();
            assert_eq!(info.header.packet_length(), p);
            assert_eq!(info.admission.over_soft, !setup);
            assert!(if setup {
                matches!(info.admission.authorization, Authorization::WithinSoft)
            } else {
                matches!(info.admission.authorization, Authorization::NextHeader {offset, ..} if offset == u64::from(p))
            });
            assert_eq!(buf.frame_spans().count(), p as usize / BLOCK_BYTES);
            assert_eq!(
                buf.data_spans().map(|s| s.len()).sum::<usize>(),
                p as usize - 24
            );
            assert!(buf.data_spans().flatten().all(|&b| b == 0x5a));
            checkpoint("accepted", &pool)?;
            if case == "coalesce" {
                let copy = buf.coalesce_until(deadline())?;
                assert_eq!(copy.frame().len(), p as usize);
                assert_eq!(copy.evidence().copied_bytes, p as usize);
                assert_eq!(pool.stats()?.coalesced_bytes, copy.evidence().charged_bytes);
                assert!(
                    buf.frame_spans()
                        .flatten()
                        .copied()
                        .eq(copy.frame().iter().copied())
                );
                checkpoint("coalesced", &pool)?;
                drop(copy);
                assert_eq!(pool.stats()?.coalesced_bytes, 0);
            }
            buf.clear();
            assert_eq!(pool.stats()?.idle_blocks, 8);
            checkpoint("trimmed", &pool)?;
            assert!(matches!(
                reader.next_packet_into(&mut buf, deadline())?,
                Some(WholeEvent::Packet)
            ));
            assert_eq!(buf.info().unwrap().offset, u64::from(p));
            assert!(reader.next_packet_into(&mut buf, deadline())?.is_none());
        }
    }
    Ok(())
}
