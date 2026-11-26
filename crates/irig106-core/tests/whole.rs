//! Hand-packed frame/admission probes; no encoder or capture-reader oracle.
#[path = "support/watchdog.rs"]
mod support;
use irig106_core::*;
use std::{
    io::{self, Cursor, Read, Seek, SeekFrom},
    time::{Duration, Instant},
};
use support::watchdog;
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
            u128::from(self.change && self.input.position() == self.input.get_ref().len() as u64),
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
    watchdog(
        Duration::from_secs(5),
        direct_blocks_admission_eof_and_truncation_case,
    );
}
fn direct_blocks_admission_eof_and_truncation_case() {
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
    watchdog(
        Duration::from_secs(5),
        refusals_legacy_and_no_speculative_growth_case,
    );
}
fn refusals_legacy_and_no_speculative_growth_case() {
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
    watchdog(
        Duration::from_secs(5),
        mixed_ordinary_frames_allocate_zero_after_warmup_case,
    );
}
fn mixed_ordinary_frames_allocate_zero_after_warmup_case() {
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
    watchdog(
        Duration::from_secs(5),
        body_destinations_are_final_storage_even_with_short_and_interrupted_reads_case,
    );
}
fn body_destinations_are_final_storage_even_with_short_and_interrupted_reads_case() {
    let pool = BlockPool::with_defaults().unwrap();
    let mut buf = PacketBuf::new(pool.clone()).unwrap();
    let expected = frame(128, 1, 9, 0x83);
    let input = DirectProbe {
        input: Cursor::new(expected.clone()),
        records: Vec::with_capacity(128),
        interrupt: false,
    };
    let mut r =
        WholePacketReader::stream(input, WholePolicy::new().with_small_body_cutoff(0), pool)
            .unwrap();
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
    watchdog(
        Duration::from_secs(5),
        small_huge_claim_is_one_provisional_block_or_known_eof_drain_case,
    );
}
fn small_huge_claim_is_one_provisional_block_or_known_eof_drain_case() {
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
    watchdog(
        Duration::from_secs(5),
        recovery_and_policy_refusals_are_terminal_and_keep_offsets_case,
    );
}
fn recovery_and_policy_refusals_are_terminal_and_keep_offsets_case() {
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
    assert_eq!(r.into_inner().input.position(), 65_536);
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
    watchdog(
        Duration::from_secs(5),
        budget_impossibility_and_temporary_pressure_use_distinct_typed_errors_case,
    );
}
fn budget_impossibility_and_temporary_pressure_use_distinct_typed_errors_case() {
    let pool = BlockPool::new(BLOCK_BYTES + 196608).unwrap();
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
    watchdog(
        Duration::from_secs(60),
        maximum_setup_fits_default_h_and_b_case,
    );
}
fn maximum_setup_fits_default_h_and_b_case() {
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
    watchdog(
        Duration::from_secs(5),
        setup_scope_unknown_edition_and_header_versions_are_explicit_case,
    );
}
fn setup_scope_unknown_edition_and_header_versions_are_explicit_case() {
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
        let n = b.len().min(24 - self.0.position() as usize);
        self.0.read(&mut b[..n])
    }
}
#[test]
fn fatal_framing_and_operational_errors_never_become_tails_or_recoveries() {
    watchdog(
        Duration::from_secs(5),
        fatal_framing_and_operational_errors_never_become_tails_or_recoveries_case,
    );
}
fn fatal_framing_and_operational_errors_never_become_tails_or_recoveries_case() {
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
    watchdog(
        Duration::from_secs(5),
        worker_return_unblocks_whole_reader_within_same_aggregate_budget_case,
    );
}
fn worker_return_unblocks_whole_reader_within_same_aggregate_budget_case() {
    let pool = BlockPool::new(BLOCK_BYTES + 196608).unwrap();
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
    assert!(pool.stats().unwrap().used_bytes <= BLOCK_BYTES + 196608);
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
    watchdog(
        Duration::from_secs(5),
        failed_probe_restores_cursor_and_never_falls_back_to_stream_trust_case,
    );
}
fn failed_probe_restores_cursor_and_never_falls_back_to_stream_trust_case() {
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
    assert_eq!(r.into_inner().0.input.position(), 65_536);
    assert_eq!(pool.stats().unwrap().block_allocations, 0);
}

#[test]
fn uncorroborated_size_is_refused_before_waiting_for_build_admission() {
    watchdog(
        Duration::from_secs(5),
        uncorroborated_size_is_refused_before_waiting_for_build_admission_case,
    );
}
fn uncorroborated_size_is_refused_before_waiting_for_build_admission_case() {
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
    watchdog(
        Duration::from_secs(5),
        truncated_large_assembly_returns_and_trims_its_partial_block_too_case,
    );
}
fn truncated_large_assembly_returns_and_trims_its_partial_block_too_case() {
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
    watchdog(
        Duration::from_secs(5),
        recovery_uses_chunk_reads_and_preserves_prefetched_frames_and_tail_counts_case,
    );
}
fn recovery_uses_chunk_reads_and_preserves_prefetched_frames_and_tail_counts_case() {
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
    watchdog(
        Duration::from_secs(5),
        ordinary_maximum_body_is_not_split_into_artificial_io_quanta_case,
    );
}
fn ordinary_maximum_body_is_not_split_into_artificial_io_quanta_case() {
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
    watchdog(
        Duration::from_secs(5),
        frame_feasibility_precedes_both_probe_and_known_short_drain_case,
    );
}
fn frame_feasibility_precedes_both_probe_and_known_short_drain_case() {
    for short in [false, true] {
        let pool = BlockPool::new(BLOCK_BYTES + 196608).unwrap();
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
        assert_eq!(source.stamps, 1); // Construction snapshots stability before prefetch.
    }
}
#[test]
fn growth_beyond_an_unchanged_stamp_is_source_changed() {
    watchdog(
        Duration::from_secs(5),
        growth_beyond_an_unchanged_stamp_is_source_changed_case,
    );
}
fn growth_beyond_an_unchanged_stamp_is_source_changed_case() {
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
    watchdog(
        Duration::from_secs(5),
        exact_eof_mutation_between_snapshots_is_refused_before_body_storage_case,
    );
}
fn exact_eof_mutation_between_snapshots_is_refused_before_body_storage_case() {
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
    watchdog(Duration::from_secs(30), bounded_file_read_benchmark_case);
}
fn bounded_file_read_benchmark_case() {
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
    watchdog(
        Duration::from_secs(5),
        recovered_seekable_oversoft_frame_preserves_prefetch_during_probe_or_exact_eof_case,
    );
}
fn recovered_seekable_oversoft_frame_preserves_prefetch_during_probe_or_exact_eof_case() {
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
    watchdog(
        Duration::from_secs(5),
        recovery_chunk_boundaries_and_short_interrupted_input_keep_exact_bytes_case,
    );
}
fn recovery_chunk_boundaries_and_short_interrupted_input_keep_exact_bytes_case() {
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

struct HybridProbe {
    header_only: bool,
    input: Cursor<Vec<u8>>,
    observations: std::sync::Arc<std::sync::Mutex<Vec<(u64, usize, usize)>>>,
}
impl Read for HybridProbe {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let position = self.input.position();
        self.observations
            .lock()
            .unwrap()
            .push((position, out.as_ptr() as usize, out.len()));
        let wanted = if position == 0 && self.header_only {
            out.len().min(24)
        } else {
            out.len()
        };
        self.input.read(&mut out[..wanted])
    }
}
#[test]
fn hybrid_cutoff_selects_copy_or_final_destination_and_preserves_next_header() {
    watchdog(Duration::from_secs(5), || {
        assert_eq!(WholePolicy::new().read_ahead_bytes(), 65_536);
        assert_eq!(WholePolicy::new().small_body_cutoff(), 4096);
        for (body, cutoff, copy) in [
            (4092usize, 4096, true),
            (4096, 4096, false),
            (4092, 4092, false),
        ] {
            let pool = BlockPool::with_defaults().unwrap();
            let mut buf = PacketBuf::new(pool.clone()).unwrap();
            let expected = frame((body + 24) as u32, 0, 9, 0);
            let mut bytes = expected.clone();
            bytes.extend(frame(28, 1, 9, 0));
            let observations = std::sync::Arc::new(std::sync::Mutex::new(Vec::with_capacity(8)));
            let input = HybridProbe {
                header_only: true,
                input: Cursor::new(bytes),
                observations: observations.clone(),
            };
            let mut r = WholePacketReader::stream(
                input,
                WholePolicy::new().with_small_body_cutoff(cutoff),
                pool,
            )
            .unwrap();
            r.next_packet_into(&mut buf, deadline()).unwrap();
            let packet = buf.contiguous().unwrap();
            assert_eq!(packet, expected);
            let records = observations.lock().unwrap();
            let (_, destination, requested) = records[1];
            assert_eq!(destination == packet.as_ptr() as usize + 24, !copy);
            assert_eq!(requested, if copy { 65_536 } else { body });
            drop(records);
            assert_eq!(r.position(), (body + 24) as u64);
            r.next_packet_into(&mut buf, deadline()).unwrap();
            assert_eq!(buf.contiguous().unwrap(), frame(28, 1, 9, 0));
        }
    });
}
#[test]
fn hybrid_fully_buffered_body_copies_even_with_zero_cutoff() {
    watchdog(Duration::from_secs(5), || {
        let pool = BlockPool::with_defaults().unwrap();
        let mut buf = PacketBuf::new(pool.clone()).unwrap();
        let mut r = WholePacketReader::stream(
            Counted {
                input: Cursor::new(frame(128, 0, 9, 0)),
                calls: 0,
            },
            WholePolicy::new().with_small_body_cutoff(0),
            pool,
        )
        .unwrap();
        r.next_packet_into(&mut buf, deadline()).unwrap();
        assert_eq!(buf.contiguous().unwrap(), frame(128, 0, 9, 0));
        assert_eq!(r.into_inner().calls, 1);
    });
}
#[test]
fn hybrid_large_final_fragment_stays_direct_and_buffer_is_budgeted() {
    watchdog(Duration::from_secs(5), || {
        let pool = BlockPool::with_defaults().unwrap();
        let mut buf = PacketBuf::new(pool.clone()).unwrap();
        let before = pool.stats().unwrap();
        let observations = std::sync::Arc::new(std::sync::Mutex::new(Vec::with_capacity(8)));
        let input = HybridProbe {
            header_only: true,
            input: Cursor::new(frame(524_292, 0, 9, 0)),
            observations: observations.clone(),
        };
        let mut r = WholePacketReader::stream(
            input,
            WholePolicy::new().with_stream_opt_in(true),
            pool.clone(),
        )
        .unwrap();
        assert!(pool.stats().unwrap().used_bytes >= before.used_bytes + 65_536);
        assert_eq!(pool.stats().unwrap().allocated_blocks, 0);
        r.next_packet_into(&mut buf, deadline()).unwrap();
        let last = buf.frame_spans().last().unwrap();
        assert_eq!(last.len(), 4);
        let records = observations.lock().unwrap();
        let (_, pointer, requested) = *records.last().unwrap();
        assert_eq!(requested, 4);
        assert_eq!(pointer, last.as_ptr() as usize);
        drop(records);
        drop(r);
        buf.clear();
        assert!(pool.stats().unwrap().used_bytes <= DEFAULT_POOL_BUDGET);
    });
}
#[test]
fn hybrid_invalid_buffer_configuration_is_typed_before_allocation() {
    watchdog(Duration::from_secs(5), || {
        let pool = BlockPool::with_defaults().unwrap();
        let before = pool.stats().unwrap().used_bytes;
        for bytes in [0, 23, usize::MAX] {
            let result = WholePacketReader::stream(
                &[][..],
                WholePolicy::new().with_read_ahead_bytes(bytes),
                pool.clone(),
            );
            assert!(matches!(
                result.err().unwrap().reason,
                ErrorKind::Pool {
                    source: PoolError::InvalidRequest,
                    ..
                }
            ));
            assert_eq!(pool.stats().unwrap().used_bytes, before);
        }
    });
}

#[test]
fn hybrid_large_body_drains_prefetch_then_uses_final_block_destinations() {
    watchdog(Duration::from_secs(5), || {
        let pool = BlockPool::with_defaults().unwrap();
        let mut buf = PacketBuf::new(pool.clone()).unwrap();
        let expected = frame(524_292, 0, 9, 0);
        let observations = std::sync::Arc::new(std::sync::Mutex::new(Vec::with_capacity(8)));
        let input = HybridProbe {
            header_only: false,
            input: Cursor::new(expected.clone()),
            observations: observations.clone(),
        };
        let mut r =
            WholePacketReader::stream(input, WholePolicy::new().with_stream_opt_in(true), pool)
                .unwrap();
        r.next_packet_into(&mut buf, deadline()).unwrap();
        let spans: Vec<_> = buf.frame_spans().collect();
        assert_eq!(
            spans
                .iter()
                .flat_map(|b| b.iter())
                .copied()
                .collect::<Vec<_>>(),
            expected
        );
        let records = observations.lock().unwrap();
        assert_eq!(records.len(), 3);
        assert_eq!(records[0].2, 65_536);
        assert_eq!(
            records[1],
            (
                65_536,
                spans[0].as_ptr() as usize + 65_536,
                BLOCK_BYTES - 65_536
            )
        );
        assert_eq!(
            records[2],
            (BLOCK_BYTES as u64, spans[1].as_ptr() as usize, 4)
        );
    });
}
#[test]
fn hybrid_prefetch_mutation_is_detected_against_constructor_snapshot() {
    watchdog(Duration::from_secs(5), || {
        let pool = BlockPool::with_defaults().unwrap();
        let mut buf = PacketBuf::new(pool.clone()).unwrap();
        let mut source = stable(frame(128, 0, 9, 0));
        source.change = true;
        let mut r =
            WholePacketReader::seek_checked(source, WholePolicy::new(), pool.clone()).unwrap();
        assert!(matches!(
            r.next_packet_into(&mut buf, deadline()).unwrap_err().reason,
            ErrorKind::Admission {
                source: AdmissionError::SourceChanged,
                ..
            }
        ));
        assert_eq!(r.position(), 24);
        assert_eq!(pool.stats().unwrap().block_allocations, 0);
        let error = WholePacketReader::stream(
            &[][..],
            WholePolicy::new().with_read_ahead_bytes(DEFAULT_POOL_BUDGET),
            pool,
        )
        .err()
        .unwrap();
        assert!(matches!(
            error.reason,
            ErrorKind::Pool {
                source: PoolError::BudgetTooSmall { .. },
                ..
            }
        ));
    });
}

struct BoundaryProbe {
    input: Cursor<Vec<u8>>,
    requests: Vec<(u64, usize)>,
    short_at: Option<(u64, usize)>,
}
impl Read for BoundaryProbe {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let offset = self.input.position();
        self.requests.push((offset, out.len()));
        let maximum = self
            .short_at
            .filter(|(at, _)| *at == offset)
            .map_or(out.len(), |(_, n)| n);
        let n = out.len().min(maximum);
        self.input.read(&mut out[..n])
    }
}
#[test]
fn adaptive_boundary_refills_cap_after_large_and_restore_full_after_small() {
    watchdog(Duration::from_secs(5), || {
        let pool = BlockPool::with_defaults().unwrap();
        let mut buf = PacketBuf::new(pool.clone()).unwrap();
        let frames = [
            frame(65_536, 0, 9, 0),
            frame(28, 0, 9, 0),
            frame(65_536, 0, 9, 0),
            frame(28, 0, 9, 0),
        ];
        let input = BoundaryProbe {
            input: Cursor::new(frames.concat()),
            requests: Vec::with_capacity(8),
            short_at: Some((65_560, 4)),
        };
        let mut r = WholePacketReader::stream(input, WholePolicy::new(), pool).unwrap();
        let mut offset = 0;
        for expected in &frames {
            r.next_packet_into(&mut buf, deadline()).unwrap();
            assert_eq!(buf.contiguous().unwrap(), expected);
            assert_eq!(buf.info().unwrap().offset, offset);
            offset += expected.len() as u64;
        }
        assert!(r.next_packet_into(&mut buf, deadline()).unwrap().is_none());
        assert_eq!(
            r.into_inner().requests,
            [
                (0, 65_536),
                (65_536, 24),
                (65_560, 65_536),
                (65_564, 65_536),
                (131_100, 24),
                (131_124, 65_536),
                (131_128, 65_536)
            ]
        );
    });
}
#[test]
fn adaptive_boundary_caps_cover_large_standard_sizes() {
    watchdog(Duration::from_secs(5), || {
        for p in [65_536, 65_560, 524_288] {
            let pool = BlockPool::with_defaults().unwrap();
            let mut buf = PacketBuf::new(pool.clone()).unwrap();
            let f = frame(p, 0, 9, 0);
            let mut bytes = f.clone();
            bytes.extend(&f);
            let input = BoundaryProbe {
                input: Cursor::new(bytes),
                requests: Vec::with_capacity(8),
                short_at: None,
            };
            let mut r = WholePacketReader::stream(input, WholePolicy::new(), pool).unwrap();
            for _ in 0..2 {
                r.next_packet_into(&mut buf, deadline()).unwrap();
                assert_eq!(buf.contiguous().unwrap(), f);
            }
            let requests = r.into_inner().requests;
            let &(_, request) = requests.iter().find(|(at, _)| *at == u64::from(p)).unwrap();
            assert_eq!(request, 24);
        }
    });
}
#[test]
fn read_ahead_minimum_applies_to_stream_and_seek_checked_without_leaking_workspace() {
    watchdog(Duration::from_secs(5), || {
        let pool = BlockPool::with_defaults().unwrap();
        let before = pool.stats().unwrap().used_bytes;
        for capacity in [23, 24] {
            let policy = WholePolicy::new().with_read_ahead_bytes(capacity);
            let stream =
                WholePacketReader::stream(Cursor::new(frame(28, 0, 9, 0)), policy, pool.clone());
            let seek =
                WholePacketReader::seek_checked(stable(frame(28, 0, 9, 0)), policy, pool.clone());
            if capacity == 23 {
                assert!(matches!(
                    stream.err().unwrap().reason,
                    ErrorKind::Pool {
                        source: PoolError::InvalidRequest,
                        ..
                    }
                ));
                assert!(matches!(
                    seek.err().unwrap().reason,
                    ErrorKind::Pool {
                        source: PoolError::InvalidRequest,
                        ..
                    }
                ));
            } else {
                drop(stream.unwrap());
                drop(seek.unwrap());
            }
            assert_eq!(pool.stats().unwrap().used_bytes, before);
        }
    });
}
struct MatrixRead<'a> {
    input: Cursor<&'a [u8]>,
    maximum: usize,
    interrupt: bool,
}
impl Read for MatrixRead<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if !self.interrupt && self.input.position() > 0 {
            self.interrupt = true;
            return Err(io::ErrorKind::Interrupted.into());
        }
        let n = out.len().min(self.maximum);
        self.input.read(&mut out[..n])
    }
}
#[test]
fn adaptive_boundary_straddle_cutoff_and_short_read_matrix() {
    watchdog(Duration::from_secs(30), || {
        let sizes = [28, 32, 48, 64, 128, 4096, 4116, 4120, 65_536, 65_560];
        let frames: Vec<_> = sizes
            .into_iter()
            .enumerate()
            .map(|(i, p)| {
                frame(
                    p,
                    i as u16,
                    9,
                    if p >= 48 { 0x80 | (i as u8 % 4) } else { 0 },
                )
            })
            .collect();
        let mut corpus = Vec::new();
        let mut expected = Vec::new();
        for (i, f) in frames.iter().enumerate() {
            let gap = if i == 3 || i == 7 { 3 } else { 0 };
            corpus.extend(std::iter::repeat_n(0x55, gap));
            expected.push((corpus.len() as u64, gap as u64));
            corpus.extend(f);
        }
        let mut packets = 0;
        for capacity in [24, 25, 31, 64, 256, 4096, 65_536] {
            for cutoff in [0, 1, 4, 24, 4096, 65_536, usize::MAX] {
                for maximum in [1, 7, 23, usize::MAX] {
                    let pool = BlockPool::with_defaults().unwrap();
                    let mut buf = PacketBuf::new(pool.clone()).unwrap();
                    let input = MatrixRead {
                        input: Cursor::new(corpus.as_slice()),
                        maximum,
                        interrupt: false,
                    };
                    let policy = WholePolicy::new()
                        .with_read_ahead_bytes(capacity)
                        .with_small_body_cutoff(cutoff);
                    let mut r = WholePacketReader::stream(input, policy, pool).unwrap();
                    for (i, f) in frames.iter().enumerate() {
                        assert!(matches!(
                            r.next_packet_into(&mut buf, deadline()).unwrap(),
                            Some(WholeEvent::Packet)
                        ));
                        assert_eq!(
                            buf.contiguous().unwrap(),
                            f,
                            "capacity={capacity}, cutoff={cutoff}, short={maximum}, frame={i}"
                        );
                        let info = buf.info().unwrap();
                        assert_eq!((info.offset, info.recovered_bytes), expected[i]);
                        packets += 1;
                    }
                    assert!(r.next_packet_into(&mut buf, deadline()).unwrap().is_none());
                    assert_eq!(r.position(), corpus.len() as u64);
                }
            }
        }
        assert_eq!(packets, 1960);
    });
}

#[test]
fn adaptive_limited_header_does_not_limit_small_body_refills() {
    watchdog(Duration::from_secs(5), || {
        let pool = BlockPool::with_defaults().unwrap();
        let mut buf = PacketBuf::new(pool.clone()).unwrap();
        let frames = [
            frame(65_536, 0, 9, 0),
            frame(28, 0, 9, 0),
            frame(65_536, 0, 9, 0),
            frame(28, 0, 9, 0),
        ];
        let input = BoundaryProbe {
            input: Cursor::new(frames.concat()),
            requests: Vec::with_capacity(8),
            short_at: None,
        };
        let mut r = WholePacketReader::stream(input, WholePolicy::new(), pool).unwrap();
        for expected in &frames {
            r.next_packet_into(&mut buf, deadline()).unwrap();
            assert_eq!(buf.contiguous().unwrap(), expected);
        }
        assert!(r.next_packet_into(&mut buf, deadline()).unwrap().is_none());
        assert_eq!(
            r.into_inner().requests,
            [
                (0, 65_536),
                (65_536, 24),
                (65_560, 65_536),
                (131_096, 4),
                (131_100, 24),
                (131_124, 65_536),
                (131_128, 65_536)
            ]
        );
    });
}
#[test]
fn adaptive_limited_header_does_not_limit_recovery_refills() {
    watchdog(Duration::from_secs(5), || {
        let pool = BlockPool::with_defaults().unwrap();
        let mut buf = PacketBuf::new(pool.clone()).unwrap();
        let mut bytes = frame(65_536, 0, 9, 0);
        bytes.extend(std::iter::repeat_n(0x55, 100_000));
        bytes.extend(frame(28, 0, 9, 0));
        let input = BoundaryProbe {
            input: Cursor::new(bytes),
            requests: Vec::with_capacity(8),
            short_at: None,
        };
        let mut r = WholePacketReader::stream(input, WholePolicy::new(), pool).unwrap();
        r.next_packet_into(&mut buf, deadline()).unwrap();
        r.next_packet_into(&mut buf, deadline()).unwrap();
        assert_eq!(buf.contiguous().unwrap(), frame(28, 0, 9, 0));
        assert_eq!(buf.info().unwrap().offset, 165_536);
        assert_eq!(buf.info().unwrap().recovered_bytes, 100_000);
        let requests = r.into_inner().requests;
        assert_eq!(requests[1], (65_536, 24));
        assert!(requests[2..].iter().all(|(_, n)| *n == 65_536));
    });
}

#[test]
fn previous_large_includes_body_exactly_at_cutoff() {
    watchdog(Duration::from_secs(5), || {
        let pool = BlockPool::with_defaults().unwrap();
        let mut buf = PacketBuf::new(pool.clone()).unwrap();
        let mut bytes = frame(4120, 0, 9, 0);
        bytes.extend(frame(28, 0, 9, 0));
        let input = BoundaryProbe {
            input: Cursor::new(bytes),
            requests: Vec::with_capacity(4),
            short_at: Some((0, 4120)),
        };
        let mut r = WholePacketReader::stream(input, WholePolicy::new(), pool).unwrap();
        r.next_packet_into(&mut buf, deadline()).unwrap();
        r.next_packet_into(&mut buf, deadline()).unwrap();
        assert_eq!(r.into_inner().requests[1], (4120, 24));
    });
}

#[test]
fn whole_reader_workers_reuse_buffers_and_inspect_without_warm_allocations() {
    watchdog(Duration::from_secs(15), || {
        use std::{sync::mpsc::sync_channel, thread};
        fn counted<T>(f: impl FnOnce() -> T) -> (T, usize) {
            ALLOCATIONS.with(|c| {
                assert!(c.get().is_none());
                c.set(Some(0));
            });
            let value = f();
            let count = ALLOCATIONS.with(|c| c.replace(None).unwrap());
            (value, count)
        }
        fn small(index: usize) -> Vec<u8> {
            let mut b = frame(28, 1, 9, 0);
            b[13] = 254u8.wrapping_add(index as u8);
            let sum = b[..22].chunks_exact(2).fold(0u16, |s, w| {
                s.wrapping_add(u16::from_le_bytes([w[0], w[1]]))
            });
            b[22..24].copy_from_slice(&sum.to_le_bytes());
            b
        }
        let mut bytes = Vec::new();
        for i in 0..48 {
            bytes.extend(small(i));
        }
        bytes.extend(frame((9 * BLOCK_BYTES) as u32, 0, 1, 0));
        for i in 48..80 {
            bytes.extend(small(i));
        }
        let pool = BlockPool::with_defaults().unwrap();
        let mut available = Vec::with_capacity(16);
        for _ in 0..16 {
            available.push(PacketBuf::new(pool.clone()).unwrap());
        }
        let mut reader =
            WholePacketReader::stream(Cursor::new(bytes), WholePolicy::new(), pool.clone())
                .unwrap();
        let mut inventory = Inspection::new(InspectionLimits::new(2, 2));
        let (returned, returns) = sync_channel(16);
        let mut inputs = Vec::new();
        let mut releases = Vec::new();
        let mut workers = Vec::new();
        for _ in 0..4 {
            let (send, receive) = sync_channel::<PacketBuf>(4);
            let (release, go) = sync_channel(1);
            inputs.push(send);
            releases.push(release);
            let returned = returned.clone();
            workers.push(thread::spawn(move || {
                for _ in 0..5 {
                    let buffers: [PacketBuf; 4] = std::array::from_fn(|_| {
                        receive.recv_timeout(Duration::from_secs(5)).unwrap()
                    });
                    go.recv_timeout(Duration::from_secs(5)).unwrap();
                    for mut buffer in buffers {
                        let (_, allocations) = counted(|| {
                            assert!(buffer.data_spans().flatten().all(|&b| b == 0x5a));
                            buffer.clear();
                        });
                        returned.send((buffer, allocations)).unwrap();
                    }
                }
            }));
        }
        let mut small_index = 0;
        let mut measured_allocations = 0;
        for round in 0..5 {
            if round == 3 {
                let mut buffer = available.pop().unwrap();
                reader.next_packet_into(&mut buffer, deadline()).unwrap();
                inventory
                    .observe_packet_info(buffer.info().unwrap())
                    .unwrap();
                assert_eq!(buffer.frame_spans().count(), 9);
                buffer.clear();
                available.push(buffer);
                assert_eq!(pool.stats().unwrap().idle_blocks, 8);
            }
            let measure = matches!(round, 1 | 2 | 4);
            for index in 0..16 {
                let mut buffer = available.pop().unwrap();
                let (observation, allocations) = counted(|| {
                    assert!(matches!(
                        reader.next_packet_into(&mut buffer, deadline()).unwrap(),
                        Some(WholeEvent::Packet)
                    ));
                    inventory
                        .observe_packet_info(buffer.info().unwrap())
                        .unwrap()
                });
                assert_eq!(
                    observation.kind(),
                    if small_index == 0 {
                        SequenceKind::First
                    } else {
                        SequenceKind::Contiguous
                    }
                );
                small_index += 1;
                if measure {
                    measured_allocations += allocations;
                }
                inputs[index % 4].send(buffer).unwrap();
            }
            let held = pool.stats().unwrap();
            assert_eq!((held.allocated_blocks, held.idle_blocks), (16, 0));
            assert!(held.used_bytes <= DEFAULT_POOL_BUDGET);
            for release in &releases {
                release.send(()).unwrap();
            }
            for _ in 0..16 {
                let (buffer, allocations) = returns.recv_timeout(Duration::from_secs(5)).unwrap();
                if measure {
                    measured_allocations += allocations;
                }
                available.push(buffer);
            }
            assert_eq!(pool.stats().unwrap().idle_blocks, 16);
            assert_eq!(
                pool.stats().unwrap().block_allocations,
                if round < 3 { 16 } else { 24 }
            );
        }
        assert_eq!(measured_allocations, 0);
        for worker in workers {
            let until = Instant::now() + Duration::from_secs(5);
            while !worker.is_finished() {
                assert!(Instant::now() < until);
                thread::yield_now();
            }
            worker.join().unwrap();
        }
        assert!(
            reader
                .next_packet_into(&mut available[0], deadline())
                .unwrap()
                .is_none()
        );
        inventory.finish();
        assert_eq!(inventory.status(), ScanStatus::Complete);
        assert_eq!(
            (
                inventory.totals().packets,
                inventory.totals().data_bytes,
                inventory.totals().packet_bytes
            ),
            (81, 324, 80 * 28 + 9 * BLOCK_BYTES as u64)
        );
        assert_eq!(inventory.presence().setup_record, Presence::Present);
        assert_eq!(inventory.presence().recording_index, Presence::Absent);
        assert_eq!(inventory.presence().recording_events, Presence::Absent);
    });
}

#[test]
fn target_sized_limits_fail_before_payload_allocation() {
    watchdog(Duration::from_secs(5), || {
        let unrepresentable = isize::MAX as usize + 1;
        assert!(matches!(BlockPool::new(unrepresentable), Err(PoolError::InvalidRequest)));
        let pool = BlockPool::with_defaults().unwrap();
        let used = pool.stats().unwrap().used_bytes;
        let policy = WholePolicy::new().with_read_ahead_bytes(unrepresentable);
        assert!(matches!(WholePacketReader::stream(Cursor::new([]), policy, pool.clone()),
            Err(Error { reason: ErrorKind::Pool { source: PoolError::InvalidRequest }, .. })));
        assert_eq!(pool.stats().unwrap().used_bytes, used);
        if usize::BITS == 32 {
            // Hand-packed largest aligned u32 P; only 28 physical bytes exist.
            let mut bytes = frame(28, 0, 9, 0);
            bytes[4..8].copy_from_slice(&0xffff_fffcu32.to_le_bytes());
            let sum = bytes[..22].chunks_exact(2).fold(0u16, |s, w| {
                s.wrapping_add(u16::from_le_bytes([w[0], w[1]]))
            });
            bytes[22..24].copy_from_slice(&sum.to_le_bytes());
            let mut buf = PacketBuf::new(pool.clone()).unwrap();
            let mut reader = WholePacketReader::stream(Cursor::new(bytes),
                WholePolicy::new().with_hard_limit(u32::MAX).with_stream_opt_in(true), pool.clone()).unwrap();
            assert!(matches!(reader.next_packet_into(&mut buf, deadline()),
                Err(Error { reason: ErrorKind::Admission { source: AdmissionError::TargetLimit }, .. })));
            assert_eq!(pool.stats().unwrap().allocated_blocks, 0);
            assert!(buf.info().is_none());
        }
    });
}
