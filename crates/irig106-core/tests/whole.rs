//! Hand-packed frame/admission probes; no encoder or capture-reader oracle.
use irig106_core::*;
use std::{
    io::{self, Cursor, Read, Seek, SeekFrom},
    time::{Duration, Instant},
};
fn frame(p: u32, channel: u16, kind: u8, flags: u8) -> Vec<u8> {
    let mut b = vec![0x5a; p as usize];
    b[..24].fill(0);
    b[..2].copy_from_slice(&[0x25, 0xeb]);
    b[2..4].copy_from_slice(&channel.to_le_bytes());
    b[4..8].copy_from_slice(&p.to_le_bytes());
    b[8..12].copy_from_slice(&4u32.to_le_bytes());
    b[14] = flags;
    b[15] = kind;
    let sum = b[..22].chunks_exact(2).fold(0u16, |s, w| {
        s.wrapping_add(u16::from_le_bytes([w[0], w[1]]))
    });
    b[22..24].copy_from_slice(&sum.to_le_bytes());
    b
}
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(2)
}
#[derive(Debug)]
struct Stable {
    input: Cursor<Vec<u8>>,
    change: bool,
}
impl Read for Stable {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        self.input.read(b)
    }
}
impl Seek for Stable {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.input.seek(pos)
    }
}
impl StableSource for Stable {
    fn source_stamp(&mut self) -> io::Result<SourceStamp> {
        Ok(SourceStamp::new(
            self.input.get_ref().len() as u64,
            1,
            u128::from(self.change && self.input.position() > 24),
        ))
    }
}
fn stable(b: Vec<u8>) -> Stable {
    Stable {
        input: Cursor::new(b),
        change: false,
    }
}
#[test]
fn direct_blocks_admission_eof_and_truncation() {
    let pool = BlockPool::with_defaults().unwrap();
    let mut buf = PacketBuf::new(pool.clone()).unwrap();
    let mut bytes = frame(524_292, 7, 9, 0);
    bytes.extend(frame(28, 0, 9, 0));
    let mut r =
        WholePacketReader::seek_checked(stable(bytes), WholePolicy::new(), pool.clone()).unwrap();
    assert!(matches!(
        r.next_packet_into(&mut buf, deadline()).unwrap(),
        Some(WholeEvent::Packet)
    ));
    assert!(matches!(
        buf.info().unwrap().admission.authorization,
        Authorization::NextHeader {
            offset: 524_292,
            ..
        }
    ));
    assert_eq!(buf.frame_spans().count(), 2);
    assert_eq!(buf.frame_spans().map(<[u8]>::len).sum::<usize>(), 524_292);
    assert_eq!(
        buf.data_spans().flatten().copied().collect::<Vec<_>>(),
        [0x5a; 4]
    );
    assert!(matches!(
        r.next_packet_into(&mut buf, deadline()).unwrap(),
        Some(WholeEvent::Packet)
    ));
    assert_eq!(buf.contiguous().unwrap().len(), 28);
    assert!(r.next_packet_into(&mut buf, deadline()).unwrap().is_none());
    for (cut, changed) in [(0, false), (1, false), (0, true)] {
        let mut b = frame(524_292, 7, 9, 0);
        b.truncate(b.len() - cut);
        let mut source = stable(b);
        source.change = changed;
        let mut r =
            WholePacketReader::seek_checked(source, WholePolicy::new(), pool.clone()).unwrap();
        let result = r.next_packet_into(&mut buf, deadline());
        if changed {
            assert!(matches!(
                result.unwrap_err().reason,
                ErrorKind::Admission {
                    source: AdmissionError::SourceChanged,
                    ..
                }
            ));
        } else if cut == 0 {
            assert!(matches!(result.unwrap(), Some(WholeEvent::Packet)));
            assert_eq!(
                buf.info().unwrap().admission.authorization,
                Authorization::Unconfirmed(UnconfirmedReason::ExactEof)
            );
        } else {
            assert!(matches!(
                result.unwrap_err().reason,
                ErrorKind::NoCompletePacket {
                    tail: Some(Tail {
                        kind: TailKind::TruncatedPacket,
                        bytes: 524_291,
                        ..
                    }),
                    ..
                }
            ));
            assert!(buf.info().is_none());
        }
    }
}
#[test]
fn refusals_legacy_and_no_speculative_growth() {
    let pool = BlockPool::with_defaults().unwrap();
    let mut b = PacketBuf::new(pool.clone()).unwrap();
    assert_eq!(WholePolicy::new().hard_limit(), 134_217_728);
    for (policy, accepted) in [
        (WholePolicy::new(), false),
        (WholePolicy::new().with_legacy_setup_any_channel(true), true),
    ] {
        let mut r =
            WholePacketReader::stream(Cursor::new(frame(524_292, 4, 1, 0)), policy, pool.clone())
                .unwrap();
        let result = r.next_packet_into(&mut b, deadline());
        if accepted {
            assert!(matches!(result.unwrap(), Some(WholeEvent::Packet)));
            assert!(b.info().unwrap().admission.legacy_setup_any_channel);
        } else {
            assert!(matches!(
                result.unwrap_err().reason,
                ErrorKind::Admission {
                    source: AdmissionError::Uncorroborated,
                    ..
                }
            ));
            assert_eq!(r.position(), 24);
        }
    }
    let mut r = WholePacketReader::stream(
        Cursor::new(frame(524_292, 0, 1, 0)),
        WholePolicy::new().with_hard_limit(524_288),
        pool.clone(),
    )
    .unwrap();
    assert!(matches!(
        r.next_packet_into(&mut b, deadline()).unwrap_err().reason,
        ErrorKind::Admission {
            source: AdmissionError::HardLimit {
                length: 524_292,
                limit: 524_288,
                ..
            },
            ..
        }
    ));
    let mut raw = frame(28, 0, 9, 0);
    raw[4..8].copy_from_slice(&0xffff_fffcu32.to_le_bytes());
    let sum = raw[..22].chunks_exact(2).fold(0u16, |s, w| {
        s.wrapping_add(u16::from_le_bytes([w[0], w[1]]))
    });
    raw[22..24].copy_from_slice(&sum.to_le_bytes());
    let mut r = WholePacketReader::stream(Cursor::new(raw), WholePolicy::new(), pool).unwrap();
    assert!(matches!(
        r.next_packet_into(&mut b, deadline()).unwrap_err().reason,
        ErrorKind::Admission {
            source: AdmissionError::HardLimit { .. },
            ..
        }
    ));
    assert!(r.next_packet_into(&mut b, deadline()).unwrap().is_none());
}

// Allocation instrumentation is test-only; the library forbids unsafe code.
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};
thread_local! {static ALLOCATIONS:Cell<Option<usize>>=const {Cell::new(None)};}
struct Counter;
fn count() {
    let _ = ALLOCATIONS.try_with(|c| {
        if let Some(n) = c.get() {
            c.set(Some(n + 1));
        }
    });
}
unsafe impl GlobalAlloc for Counter {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        count();
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        count();
        unsafe { System.realloc(p, l, n) }
    }
}
#[global_allocator]
static COUNTER: Counter = Counter;
#[test]
fn mixed_ordinary_frames_allocate_zero_after_warmup() {
    let pool = BlockPool::with_defaults().unwrap();
    let mut buf = PacketBuf::new(pool.clone()).unwrap();
    let mut bytes = Vec::new();
    for _ in 0..8 {
        for p in [28, 400, 4096, 524_288] {
            bytes.extend(frame(p, 3, 9, 0));
        }
    }
    let mut r = WholePacketReader::stream(Cursor::new(bytes), WholePolicy::new(), pool).unwrap();
    r.next_packet_into(&mut buf, deadline()).unwrap(); // Warm final storage.
    ALLOCATIONS.with(|c| c.set(Some(0)));
    for _ in 1..32 {
        assert!(matches!(
            r.next_packet_into(&mut buf, deadline()).unwrap(),
            Some(WholeEvent::Packet)
        ));
        assert_eq!(buf.frame_spans().count(), 1);
    }
    buf.clear();
    let count = ALLOCATIONS.with(|c| c.replace(None).unwrap());
    assert_eq!(count, 0);
}
struct DirectProbe {
    input: Cursor<Vec<u8>>,
    records: Vec<(usize, usize, usize)>,
    interrupt: bool,
}
impl Read for DirectProbe {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        let offset = self.input.position() as usize;
        if offset >= 24 && !self.interrupt {
            self.interrupt = true;
            return Err(io::ErrorKind::Interrupted.into());
        }
        let n = b.len().min(3);
        let n = self.input.read(&mut b[..n])?;
        if offset >= 24 && n > 0 {
            self.records.push((offset, b.as_ptr() as usize, n));
        }
        Ok(n)
    }
}
#[test]
fn body_destinations_are_final_storage_even_with_short_and_interrupted_reads() {
    let pool = BlockPool::with_defaults().unwrap();
    let mut buf = PacketBuf::new(pool.clone()).unwrap();
    let expected = frame(128, 1, 9, 0x83);
    let input = DirectProbe {
        input: Cursor::new(expected.clone()),
        records: Vec::with_capacity(128),
        interrupt: false,
    };
    let mut r = WholePacketReader::stream(input, WholePolicy::new(), pool).unwrap();
    r.next_packet_into(&mut buf, deadline()).unwrap();
    let bytes = buf.contiguous().unwrap();
    assert_eq!(bytes, expected);
    assert_eq!(buf.info().unwrap().ranges.checksum, 124..128);
    assert!(buf.info().unwrap().secondary_integrity.is_some());
    let pointer = bytes.as_ptr() as usize;
    let source = r.into_inner();
    assert!(source.interrupt);
    assert_eq!(source.input.position(), 128);
    for (offset, destination, n) in source.records {
        assert_eq!(destination, pointer + offset);
        assert!(offset + n <= 128);
    }
}
#[test]
fn small_huge_claim_is_one_provisional_block_or_known_eof_drain() {
    for seekable in [false, true] {
        let pool = BlockPool::with_defaults().unwrap();
        let mut buf = PacketBuf::new(pool.clone()).unwrap();
        let mut raw = frame(8 * 1024 * 1024, 0, 9, 0);
        raw.truncate(71);
        let policy = WholePolicy::new().with_stream_opt_in(true);
        let result = if seekable {
            WholePacketReader::seek_checked(stable(raw), policy, pool.clone())
                .unwrap()
                .next_packet_into(&mut buf, deadline())
        } else {
            WholePacketReader::stream(Cursor::new(raw), policy, pool.clone())
                .unwrap()
                .next_packet_into(&mut buf, deadline())
        };
        assert!(matches!(
            result.unwrap_err().reason,
            ErrorKind::NoCompletePacket {
                tail: Some(Tail { bytes: 71, .. }),
                ..
            }
        ));
        assert_eq!(
            pool.stats().unwrap().block_allocations,
            if seekable { 0 } else { 1 }
        );
        assert!(buf.info().is_none());
        assert_eq!(buf.frame_spans().count(), 0);
    }
}
#[test]
fn recovery_and_policy_refusals_are_terminal_and_keep_offsets() {
    let pool = BlockPool::with_defaults().unwrap();
    let mut buf = PacketBuf::new(pool.clone()).unwrap();
    let mut raw = frame(28, 0, 9, 0);
    raw.extend([0x55; 4]);
    raw.extend(frame(28, 1, 9, 0));
    raw.extend([0x44; 25]);
    let mut r =
        WholePacketReader::stream(Cursor::new(raw), WholePolicy::new(), pool.clone()).unwrap();
    r.next_packet_into(&mut buf, deadline()).unwrap();
    r.next_packet_into(&mut buf, deadline()).unwrap();
    assert_eq!(buf.info().unwrap().offset, 32);
    assert_eq!(buf.info().unwrap().recovered_bytes, 4);
    assert!(matches!(
        r.next_packet_into(&mut buf, deadline()).unwrap(),
        Some(WholeEvent::Tail {
            tail: Tail {
                kind: TailKind::Unsynchronized,
                bytes: 25,
                ..
            },
            ..
        })
    ));
    assert!(r.next_packet_into(&mut buf, deadline()).unwrap().is_none());
    let mut bytes = frame(524_292, 0, 9, 0);
    bytes.extend([0x55; 24]);
    let mut r =
        WholePacketReader::seek_checked(stable(bytes), WholePolicy::new(), pool.clone()).unwrap();
    assert!(matches!(
        r.next_packet_into(&mut buf, deadline()).unwrap_err().reason,
        ErrorKind::Admission {
            source: AdmissionError::Uncorroborated,
            ..
        }
    ));
    assert_eq!(r.position(), 24);
    assert_eq!(r.into_inner().input.position(), 24);
    let mut bytes = vec![0x55];
    bytes.extend(frame(28, 0, 9, 0));
    for cap in [0, 1] {
        let mut r = WholePacketReader::stream(
            Cursor::new(bytes.clone()),
            WholePolicy::new().with_recovery_bytes(Some(cap)),
            pool.clone(),
        )
        .unwrap();
        let result = r.next_packet_into(&mut buf, deadline());
        if cap == 0 {
            assert!(matches!(
                result.unwrap_err().reason,
                ErrorKind::RecoveryLimit { limit: 0, .. }
            ));
        } else {
            result.unwrap();
            assert_eq!(buf.info().unwrap().recovered_bytes, 1);
        }
    }
}
#[test]
fn budget_impossibility_and_temporary_pressure_use_distinct_typed_errors() {
    let pool = BlockPool::new(BLOCK_BYTES + 65536).unwrap();
    let mut buf = PacketBuf::new(pool.clone()).unwrap();
    let mut r = WholePacketReader::stream(
        Cursor::new(frame(524_292, 0, 1, 0)),
        WholePolicy::new(),
        pool.clone(),
    )
    .unwrap();
    assert!(matches!(
        r.next_packet_into(&mut buf, deadline()).unwrap_err().reason,
        ErrorKind::Pool {
            source: PoolError::BudgetTooSmall { .. },
            ..
        }
    ));
    assert_eq!(pool.stats().unwrap().block_allocations, 0);
    let mut r = WholePacketReader::stream(
        Cursor::new(frame(28, 0, 9, 0)),
        WholePolicy::new(),
        pool.clone(),
    )
    .unwrap();
    let mut a = pool.try_begin(1).unwrap();
    let block = a.try_next_block().unwrap();
    drop(a);
    // Test thread cannot return this block while it is inside the reader wait.
    let worker = std::thread::spawn(move || {
        let err = r
            .next_packet_into(&mut buf, Instant::now() + Duration::from_millis(5))
            .unwrap_err();
        assert!(matches!(
            err.reason,
            ErrorKind::Pool {
                source: PoolError::DeadlineExceeded,
                ..
            }
        ));
    });
    let watchdog = deadline();
    while !worker.is_finished() {
        assert!(Instant::now() < watchdog);
        std::thread::yield_now();
    }
    worker.join().unwrap();
    drop(block);
}
// Synthetic arrival avoids a second 128 MiB fixture allocation. CI-only heavy qualification.
struct SetupSource {
    header: Vec<u8>,
    position: usize,
}
impl Read for SetupSource {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        if self.position == DEFAULT_HARD_LIMIT as usize {
            return Ok(0);
        }
        let n = b.len().min(DEFAULT_HARD_LIMIT as usize - self.position);
        for (i, byte) in b[..n].iter_mut().enumerate() {
            *byte = if self.position + i < 24 {
                self.header[self.position + i]
            } else {
                0x5a
            };
        }
        self.position += n;
        Ok(n)
    }
}
#[test]
#[ignore = "128 MiB acceptance runs explicitly in GitHub CI"]
fn maximum_setup_fits_default_h_and_b() {
    let mut header = frame(28, 0, 1, 0);
    header[4..8].copy_from_slice(&DEFAULT_HARD_LIMIT.to_le_bytes());
    let sum = header[..22].chunks_exact(2).fold(0u16, |s, w| {
        s.wrapping_add(u16::from_le_bytes([w[0], w[1]]))
    });
    header[22..24].copy_from_slice(&sum.to_le_bytes());
    let pool = BlockPool::with_defaults().unwrap();
    let mut buf = PacketBuf::new(pool.clone()).unwrap();
    let mut r = WholePacketReader::stream(
        SetupSource {
            header,
            position: 0,
        },
        WholePolicy::new(),
        pool.clone(),
    )
    .unwrap();
    assert!(matches!(
        r.next_packet_into(&mut buf, deadline()).unwrap(),
        Some(WholeEvent::Packet)
    ));
    assert_eq!(buf.frame_spans().count(), 256);
    assert_eq!(
        buf.frame_spans().map(<[u8]>::len).sum::<usize>(),
        134_217_728
    );
    assert_eq!(buf.frame_spans().last().unwrap()[BLOCK_BYTES - 1], 0x5a);
    assert!(pool.stats().unwrap().used_bytes <= DEFAULT_POOL_BUDGET);
    buf.clear();
    assert_eq!(pool.stats().unwrap().idle_blocks, 8);
}

#[test]
fn setup_scope_unknown_edition_and_header_versions_are_explicit() {
    for (channel, edition, legacy, allowed) in [
        (0, EditionPolicy::Irig106_24R1, false, true),
        (2, EditionPolicy::Irig106_24R1, false, false),
        (0, EditionPolicy::Unknown(7), false, false),
        (2, EditionPolicy::Unknown(7), true, true),
        (0, EditionPolicy::Unknown(7), true, true),
    ] {
        let pool = BlockPool::with_defaults().unwrap();
        let mut buf = PacketBuf::new(pool.clone()).unwrap();
        let mut bytes = frame(524_292, channel, 1, 0);
        bytes[12] = 0xff;
        let sum = bytes[..22].chunks_exact(2).fold(0u16, |s, w| {
            s.wrapping_add(u16::from_le_bytes([w[0], w[1]]))
        });
        bytes[22..24].copy_from_slice(&sum.to_le_bytes());
        let mut r = WholePacketReader::stream(
            Cursor::new(bytes),
            WholePolicy::new()
                .with_edition(edition)
                .with_legacy_setup_any_channel(legacy),
            pool,
        )
        .unwrap();
        let result = r.next_packet_into(&mut buf, deadline());
        if allowed {
            result.unwrap();
            assert_eq!(buf.info().unwrap().admission.edition, edition);
            assert_eq!(
                buf.info().unwrap().admission.legacy_setup_any_channel,
                legacy
            );
        } else {
            assert!(matches!(
                result.unwrap_err().reason,
                ErrorKind::Admission {
                    source: AdmissionError::Uncorroborated,
                    ..
                }
            ));
        }
    }
}
struct Denied(Cursor<Vec<u8>>);
impl Read for Denied {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        if self.0.position() >= 24 {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        self.0.read(b)
    }
}
#[test]
fn fatal_framing_and_operational_errors_never_become_tails_or_recoveries() {
    let pool = BlockPool::with_defaults().unwrap();
    let mut buf = PacketBuf::new(pool.clone()).unwrap();
    let mut r = WholePacketReader::stream(
        Denied(Cursor::new(frame(28, 0, 9, 0))),
        WholePolicy::new(),
        pool.clone(),
    )
    .unwrap();
    let e = r.next_packet_into(&mut buf, deadline()).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(e.position, 24);
    assert!(buf.info().is_none());
    assert!(r.next_packet_into(&mut buf, deadline()).unwrap().is_none());
    let mut bytes = frame(28, 0, 9, 0);
    let mut invalid = frame(32, 0, 9, 0);
    invalid[4] = 30;
    let sum = invalid[..22].chunks_exact(2).fold(0u16, |s, w| {
        s.wrapping_add(u16::from_le_bytes([w[0], w[1]]))
    });
    invalid[22..24].copy_from_slice(&sum.to_le_bytes());
    bytes.extend(invalid);
    bytes.extend(frame(28, 0, 9, 0));
    let mut r = WholePacketReader::stream(Cursor::new(bytes), WholePolicy::new(), pool).unwrap();
    r.next_packet_into(&mut buf, deadline()).unwrap();
    let e = r.next_packet_into(&mut buf, deadline()).unwrap_err();
    assert!(matches!(
        e.get_ref().unwrap().downcast_ref::<HeaderError>(),
        Some(HeaderError::UnalignedPacket { length: 30 })
    ));
    assert_eq!(e.position, 52);
    assert!(e.recovery.is_none());
    assert!(r.next_packet_into(&mut buf, deadline()).unwrap().is_none());
}

#[test]
fn worker_return_unblocks_whole_reader_within_same_aggregate_budget() {
    let pool = BlockPool::new(BLOCK_BYTES + 65536).unwrap();
    let mut buf = PacketBuf::new(pool.clone()).unwrap();
    let mut r = WholePacketReader::stream(
        Cursor::new(frame(28, 0, 9, 0)),
        WholePolicy::new(),
        pool.clone(),
    )
    .unwrap();
    let mut a = pool.try_begin(1).unwrap();
    let block = a.try_next_block().unwrap();
    drop(a);
    let worker = std::thread::spawn(move || {
        assert!(matches!(
            r.next_packet_into(&mut buf, Instant::now() + Duration::from_secs(30))
                .unwrap(),
            Some(WholeEvent::Packet)
        ));
        buf
    });
    let watchdog = Instant::now() + Duration::from_secs(5);
    while pool.stats().unwrap().waiting == 0 {
        assert!(Instant::now() < watchdog);
        std::thread::yield_now();
    }
    assert!(pool.stats().unwrap().used_bytes <= BLOCK_BYTES + 65536);
    drop(block);
    while !worker.is_finished() {
        assert!(Instant::now() < watchdog);
        std::thread::yield_now();
    }
    let buf = worker.join().unwrap();
    assert_eq!(buf.contiguous().unwrap(), frame(28, 0, 9, 0));
    assert_eq!(pool.stats().unwrap().block_allocations, 1);
}

struct ProbeDenied(Stable);
impl Read for ProbeDenied {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        if self.0.input.position() == 524_292 {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        self.0.read(b)
    }
}
impl Seek for ProbeDenied {
    fn seek(&mut self, p: SeekFrom) -> io::Result<u64> {
        self.0.seek(p)
    }
}
impl StableSource for ProbeDenied {
    fn source_stamp(&mut self) -> io::Result<SourceStamp> {
        self.0.source_stamp()
    }
}
#[test]
fn failed_probe_restores_cursor_and_never_falls_back_to_stream_trust() {
    let pool = BlockPool::with_defaults().unwrap();
    let mut buf = PacketBuf::new(pool.clone()).unwrap();
    let mut bytes = frame(524_292, 0, 9, 0);
    bytes.extend(frame(28, 0, 9, 0));
    let mut r = WholePacketReader::seek_checked(
        ProbeDenied(stable(bytes)),
        WholePolicy::new().with_stream_opt_in(true),
        pool.clone(),
    )
    .unwrap();
    let e = r.next_packet_into(&mut buf, deadline()).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(e.position, 24);
    assert_eq!(r.into_inner().0.input.position(), 24);
    assert_eq!(pool.stats().unwrap().block_allocations, 0);
}

#[test]
fn uncorroborated_size_is_refused_before_waiting_for_build_admission() {
    let pool = BlockPool::with_defaults().unwrap();
    let mut buf = PacketBuf::new(pool.clone()).unwrap();
    let mut r = WholePacketReader::stream(
        Cursor::new(frame(524_292, 2, 9, 0)),
        WholePolicy::new(),
        pool.clone(),
    )
    .unwrap();
    let owner = pool.try_begin(1).unwrap();
    assert!(matches!(
        r.next_packet_into(&mut buf, Instant::now())
            .unwrap_err()
            .reason,
        ErrorKind::Admission {
            source: AdmissionError::Uncorroborated,
            ..
        }
    ));
    assert_eq!(pool.stats().unwrap().waiting, 0);
    drop(owner);
}

#[test]
fn truncated_large_assembly_returns_and_trims_its_partial_block_too() {
    let pool = BlockPool::with_defaults().unwrap();
    let mut buf = PacketBuf::new(pool.clone()).unwrap();
    let mut bytes = frame((10 * BLOCK_BYTES) as u32, 0, 9, 0);
    bytes.truncate(8 * BLOCK_BYTES + 28);
    let mut r = WholePacketReader::stream(
        Cursor::new(bytes),
        WholePolicy::new().with_stream_opt_in(true),
        pool.clone(),
    )
    .unwrap();
    assert!(matches!(
        r.next_packet_into(&mut buf, deadline()).unwrap_err().reason,
        ErrorKind::NoCompletePacket { .. }
    ));
    assert_eq!(pool.stats().unwrap().block_allocations, 9);
    assert_eq!(pool.stats().unwrap().idle_blocks, 8);
    assert!(buf.info().is_none());
}

struct Counted<R> {
    input: R,
    calls: usize,
}
impl<R: Read> Read for Counted<R> {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        self.calls += 1;
        self.input.read(b)
    }
}
#[test]
fn recovery_uses_chunk_reads_and_preserves_prefetched_frames_and_tail_counts() {
    let pool = BlockPool::with_defaults().unwrap();
    let mut buf = PacketBuf::new(pool.clone()).unwrap();
    let mut bytes = vec![0x55; 100_000];
    bytes.extend(frame(28, 0, 9, 0));
    bytes.extend(frame(28, 1, 9, 0));
    bytes.extend([0x44; 31]);
    let mut r = WholePacketReader::stream(
        Counted {
            input: Cursor::new(bytes),
            calls: 0,
        },
        WholePolicy::new(),
        pool,
    )
    .unwrap();
    r.next_packet_into(&mut buf, deadline()).unwrap();
    assert_eq!(buf.info().unwrap().recovered_bytes, 100_000);
    assert_eq!(buf.info().unwrap().offset, 100_000);
    assert_eq!(buf.contiguous().unwrap(), frame(28, 0, 9, 0));
    r.next_packet_into(&mut buf, deadline()).unwrap();
    assert_eq!(buf.info().unwrap().offset, 100_028);
    assert_eq!(buf.info().unwrap().recovered_bytes, 0);
    assert!(matches!(
        r.next_packet_into(&mut buf, deadline()).unwrap(),
        Some(WholeEvent::Tail {
            tail: Tail {
                offset: 100_056,
                bytes: 31,
                kind: TailKind::Unsynchronized,
                ..
            },
            ..
        })
    ));
    let calls = r.into_inner().calls;
    println!("100 KB recovery: {calls} Read calls");
    assert!(calls < 32, "recovery made {calls} calls");
}
#[test]
fn ordinary_maximum_body_is_not_split_into_artificial_io_quanta() {
    let pool = BlockPool::with_defaults().unwrap();
    let mut buf = PacketBuf::new(pool.clone()).unwrap();
    let mut r = WholePacketReader::stream(
        Counted {
            input: Cursor::new(frame(524_288, 0, 9, 0)),
            calls: 0,
        },
        WholePolicy::new(),
        pool,
    )
    .unwrap();
    r.next_packet_into(&mut buf, deadline()).unwrap();
    assert_eq!(r.into_inner().calls, 2);
}
struct Observed {
    input: Cursor<Vec<u8>>,
    size: u64,
    reads: usize,
    stamps: usize,
    change_on_second_stamp: bool,
}
impl Read for Observed {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        self.reads += 1;
        self.input.read(b)
    }
}
impl Seek for Observed {
    fn seek(&mut self, p: SeekFrom) -> io::Result<u64> {
        self.input.seek(p)
    }
}
impl StableSource for Observed {
    fn source_stamp(&mut self) -> io::Result<SourceStamp> {
        self.stamps += 1;
        Ok(SourceStamp::new(
            self.size,
            1,
            u128::from(self.change_on_second_stamp && self.stamps >= 2),
        ))
    }
}
#[test]
fn frame_feasibility_precedes_both_probe_and_known_short_drain() {
    for short in [false, true] {
        let pool = BlockPool::new(BLOCK_BYTES + 65536).unwrap();
        let mut buf = PacketBuf::new(pool.clone()).unwrap();
        let mut bytes = frame(524_292, 0, 9, 0);
        if short {
            bytes.truncate(24);
        } else {
            bytes.extend(frame(28, 0, 9, 0));
        }
        let size = bytes.len() as u64;
        let mut r = WholePacketReader::seek_checked(
            Observed {
                input: Cursor::new(bytes),
                size,
                reads: 0,
                stamps: 0,
                change_on_second_stamp: false,
            },
            WholePolicy::new(),
            pool,
        )
        .unwrap();
        assert!(matches!(
            r.next_packet_into(&mut buf, deadline()).unwrap_err().reason,
            ErrorKind::Pool {
                source: PoolError::BudgetTooSmall { .. },
                ..
            }
        ));
        assert_eq!(r.position(), 24);
        let source = r.into_inner();
        assert_eq!(source.reads, 1);
        assert_eq!(source.stamps, 0);
    }
}
#[test]
fn growth_beyond_an_unchanged_stamp_is_source_changed() {
    let pool = BlockPool::with_defaults().unwrap();
    let mut buf = PacketBuf::new(pool.clone()).unwrap();
    let mut r = WholePacketReader::seek_checked(
        Observed {
            input: Cursor::new(frame(28, 0, 9, 0)),
            size: 24,
            reads: 0,
            stamps: 0,
            change_on_second_stamp: false,
        },
        WholePolicy::new(),
        pool,
    )
    .unwrap();
    assert!(matches!(
        r.next_packet_into(&mut buf, deadline()).unwrap_err().reason,
        ErrorKind::Admission {
            source: AdmissionError::SourceChanged,
            ..
        }
    ));
    assert_eq!(r.position(), 28);
    assert!(buf.info().is_none());
}
#[test]
fn exact_eof_mutation_between_snapshots_is_refused_before_body_storage() {
    let pool = BlockPool::with_defaults().unwrap();
    let mut buf = PacketBuf::new(pool.clone()).unwrap();
    let mut r = WholePacketReader::seek_checked(
        Observed {
            input: Cursor::new(frame(524_292, 0, 9, 0)),
            size: 524_292,
            reads: 0,
            stamps: 0,
            change_on_second_stamp: true,
        },
        WholePolicy::new(),
        pool.clone(),
    )
    .unwrap();
    assert!(matches!(
        r.next_packet_into(&mut buf, deadline()).unwrap_err().reason,
        ErrorKind::Admission {
            source: AdmissionError::SourceChanged,
            ..
        }
    ));
    assert_eq!(r.position(), 24);
    assert_eq!(pool.stats().unwrap().block_allocations, 0);
}
#[test]
#[ignore = "bounded warm-cache file throughput evidence, explicit CI run"]
fn bounded_file_read_benchmark() {
    use std::{
        fs::{File, OpenOptions},
        io::Write,
        time::SystemTime,
    };
    struct Temp(std::path::PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let path = Temp(std::env::temp_dir().join(format!(
            "irig106-core-{}-{}.bench",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path.0)
        .unwrap();
    let large = frame(524_288, 0, 9, 0);
    let small = frame(28, 0, 9, 0);
    for _ in 0..16 {
        file.write_all(&large).unwrap();
        file.write_all(&small).unwrap();
    }
    drop(file);
    let bytes = std::fs::metadata(&path.0).unwrap().len();
    let pool = BlockPool::with_defaults().unwrap();
    let mut buf = PacketBuf::new(pool.clone()).unwrap();
    let mut r = WholePacketReader::stream(
        Counted {
            input: File::open(&path.0).unwrap(),
            calls: 0,
        },
        WholePolicy::new(),
        pool,
    )
    .unwrap();
    let start = Instant::now();
    let mut packets = 0;
    while let Some(event) = r.next_packet_into(&mut buf, deadline()).unwrap() {
        assert!(matches!(event, WholeEvent::Packet));
        packets += 1;
    }
    let seconds = start.elapsed().as_secs_f64();
    let calls = r.into_inner().calls;
    println!(
        "bounded warm-cache file: {packets} packets, {bytes} bytes, {calls} Read calls, {:.1} packets/s, {:.1} MB/s",
        packets as f64 / seconds,
        bytes as f64 / seconds / 1e6
    );
}

#[test]
fn recovered_seekable_oversoft_frame_preserves_prefetch_during_probe_or_exact_eof() {
    for final_frame in [false, true] {
        let pool = BlockPool::with_defaults().unwrap();
        let mut buf = PacketBuf::new(pool.clone()).unwrap();
        let mut bytes = vec![0x55; 100];
        let expected = frame(524_292, 0, 9, 0);
        bytes.extend(&expected);
        if !final_frame {
            bytes.extend(frame(28, 0, 9, 0));
        }
        let mut r =
            WholePacketReader::seek_checked(stable(bytes), WholePolicy::new(), pool).unwrap();
        r.next_packet_into(&mut buf, deadline()).unwrap();
        assert_eq!(buf.info().unwrap().recovered_bytes, 100);
        assert_eq!(
            buf.frame_spans().flatten().copied().collect::<Vec<_>>(),
            expected
        );
        if final_frame {
            assert_eq!(
                buf.info().unwrap().admission.authorization,
                Authorization::Unconfirmed(UnconfirmedReason::ExactEof)
            );
        } else {
            assert!(matches!(
                buf.info().unwrap().admission.authorization,
                Authorization::NextHeader {
                    offset: 524_392,
                    ..
                }
            ));
            r.next_packet_into(&mut buf, deadline()).unwrap();
            assert_eq!(buf.contiguous().unwrap(), frame(28, 0, 9, 0));
        }
    }
}
struct RecoveryShort {
    input: Cursor<Vec<u8>>,
    interrupt: bool,
}
impl Read for RecoveryShort {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        if !self.interrupt && self.input.position() >= 24 {
            self.interrupt = true;
            return Err(io::ErrorKind::Interrupted.into());
        }
        let n = b.len().min(7);
        self.input.read(&mut b[..n])
    }
}
#[test]
fn recovery_chunk_boundaries_and_short_interrupted_input_keep_exact_bytes() {
    for short in [false, true] {
        let pool = BlockPool::with_defaults().unwrap();
        let mut buf = PacketBuf::new(pool.clone()).unwrap();
        let mut bytes = vec![0x55; 8215];
        bytes.extend(frame(28, 0, 9, 0));
        if short {
            let mut r = WholePacketReader::stream(
                RecoveryShort {
                    input: Cursor::new(bytes),
                    interrupt: false,
                },
                WholePolicy::new(),
                pool,
            )
            .unwrap();
            r.next_packet_into(&mut buf, deadline()).unwrap();
        } else {
            let mut r =
                WholePacketReader::stream(Cursor::new(bytes), WholePolicy::new(), pool).unwrap();
            r.next_packet_into(&mut buf, deadline()).unwrap();
        }
        assert_eq!(buf.info().unwrap().offset, 8215);
        assert_eq!(buf.contiguous().unwrap(), frame(28, 0, 9, 0));
    }
}

#[test]
#[ignore = "owner decision evidence only; release-mode matrix runs explicitly in CI"]
fn small_body_read_strategy_benchmark() {
    use std::{
        fs::{File, OpenOptions},
        io::{BufReader, BufWriter, Write},
        time::SystemTime,
    };
    const BUFFER: usize = 65_536;
    // Exactly one counter read. Linux accounts that read after producing its
    // snapshot; subtract its contribution from the following snapshot delta.
    fn kernel_reads() -> Option<u64> {
        #[cfg(target_os = "linux")]
        {
            let mut raw = [0; 512];
            let n = File::open("/proc/thread-self/io")
                .ok()?
                .read(&mut raw)
                .ok()?;
            std::str::from_utf8(&raw[..n])
                .ok()?
                .lines()
                .find_map(|line| line.strip_prefix("syscr: "))?
                .trim()
                .parse()
                .ok()
        }
        #[cfg(not(target_os = "linux"))]
        {
            None
        }
    }
    struct Temp(std::path::PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    struct Sample {
        seconds: f64,
        calls: usize,
        kernel: Option<u64>,
    }
    fn measure<R: Read>(
        input: R,
        count: impl FnOnce(R) -> usize,
        pool: &BlockPool,
        buf: &mut PacketBuf,
        packets: usize,
        p: usize,
    ) -> Sample {
        let mut reader =
            WholePacketReader::stream(input, WholePolicy::new(), pool.clone()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        let before = kernel_reads();
        let start = Instant::now();
        let mut seen = 0usize;
        let mut digest = 0usize;
        while let Some(event) = reader.next_packet_into(buf, deadline).unwrap() {
            assert!(matches!(event, WholeEvent::Packet));
            let bytes = std::hint::black_box(buf.contiguous().unwrap());
            digest = digest.wrapping_add(usize::from(bytes[0]) + usize::from(bytes[p - 1]));
            seen += 1;
        }
        let seconds = start.elapsed().as_secs_f64();
        let after = kernel_reads();
        let kernel = before.zip(after).map(|(a, b)| b - a - 1);
        assert_eq!(seen, packets);
        assert_eq!(digest, packets * (0x25 + 0x5a));
        let calls = count(reader.into_inner());
        #[cfg(target_os = "linux")]
        assert_eq!(
            kernel,
            Some(calls as u64),
            "kernel counter calibration / unexpected read syscalls"
        );
        Sample {
            seconds,
            calls,
            kernel,
        }
    }
    let target = std::env::var("IRIG106_SMALL_BODY_BENCH_BYTES")
        .ok()
        .map(|v| v.parse::<usize>().unwrap())
        .unwrap_or(16 * 1024 * 1024);
    assert!((524_288..=32 * 1024 * 1024).contains(&target));
    println!(
        "small_body benchmark: fixed 64 KiB buffered-copy prototype, median of 3, alternating order; generation/construction/first block initialization excluded; warm cache; EOF read included"
    );
    println!(
        "small_body,size,mode,packets,bytes,source_reads,kernel_read_syscalls,seconds,packets_per_s,MB_per_s"
    );
    for (case, p) in [28usize, 400, 4096, 65_536, 524_288]
        .into_iter()
        .enumerate()
    {
        let packets = target / p;
        let total = packets * p;
        let stamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let name = format!(
            "irig106-core-small-{}-{stamp}-{p}.bench",
            std::process::id()
        );
        let path = Temp(std::env::temp_dir().join(name));
        let mut output = BufWriter::with_capacity(
            BUFFER,
            OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path.0)
                .unwrap(),
        );
        let mut bytes = frame(p as u32, 0, 9, 0);
        bytes[8..12].copy_from_slice(&((p - 24) as u32).to_le_bytes());
        let checksum = bytes[..22].chunks_exact(2).fold(0u16, |sum, w| {
            sum.wrapping_add(u16::from_le_bytes([w[0], w[1]]))
        });
        bytes[22..24].copy_from_slice(&checksum.to_le_bytes());
        for _ in 0..packets {
            output.write_all(&bytes).unwrap();
        }
        output.flush().unwrap();
        drop(output);
        let pool = BlockPool::with_defaults().unwrap();
        let mut buf = PacketBuf::new(pool.clone()).unwrap();
        let mut warm = pool.try_begin(1).unwrap();
        drop(warm.try_next_block().unwrap());
        drop(warm);
        let mut samples: [Vec<Sample>; 2] = std::array::from_fn(|_| Vec::with_capacity(3));
        for repetition in 0..3 {
            for turn in 0..2 {
                let mode = (case + repetition + turn) % 2;
                let file = Counted {
                    input: File::open(&path.0).unwrap(),
                    calls: 0,
                };
                let sample = if mode == 0 {
                    measure(file, |source| source.calls, &pool, &mut buf, packets, p)
                } else {
                    // Test-only prototype: same production reader; an external
                    // fixed BufReader copies small bodies, bypasses large reads.
                    measure(
                        BufReader::with_capacity(BUFFER, file),
                        |source| source.into_inner().calls,
                        &pool,
                        &mut buf,
                        packets,
                        p,
                    )
                };
                samples[mode].push(sample);
            }
        }
        for (index, mut group) in samples.into_iter().enumerate() {
            group.sort_by(|a, b| a.seconds.total_cmp(&b.seconds));
            let median = &group[1];
            let mode = if index == 0 {
                "direct"
            } else {
                "buffered_copy_64KiB"
            };
            let kernel = median
                .kernel
                .map_or_else(|| "unsupported".to_owned(), |n| n.to_string());
            println!(
                "small_body,{p},{mode},{packets},{total},{},{kernel},{:.6},{:.1},{:.1}",
                median.calls,
                median.seconds,
                packets as f64 / median.seconds,
                total as f64 / median.seconds / 1e6
            );
        }
    }
}
