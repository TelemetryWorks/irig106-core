//! Explicit omissions: fixed storage, exact progress, no complete packet dispatch.
use irig106_core::*;
use std::{
    io::{self, Cursor, Read, Seek, SeekFrom},
    time::{Duration, Instant},
};
#[path = "support/watchdog.rs"]
mod support;
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(5)
}
fn frame(p: usize) -> Vec<u8> {
    let mut b = vec![0x5a; p];
    b[..24].fill(0);
    b[..2].copy_from_slice(&[0x25, 0xeb]);
    b[4..8].copy_from_slice(&(p as u32).to_le_bytes());
    b[8..12].copy_from_slice(&4u32.to_le_bytes());
    b[15] = 9;
    let sum: u32 = b[..22]
        .chunks_exact(2)
        .map(|w| u16::from_le_bytes([w[0], w[1]]) as u32)
        .sum();
    b[22..24].copy_from_slice(&(sum as u16).to_le_bytes());
    b
}
fn report(event: Option<DrainEvent>) -> DrainReport {
    match event {
        Some(DrainEvent::Report(r)) => r,
        other => panic!("expected drain report: {other:?}"),
    }
}
#[test]
fn opt_in_hard_refusal_and_pending_read_ahead_resume_exactly() {
    support::watchdog(Duration::from_secs(5), || {
        let mut first = frame(68);
        first[14] = 0x83; // Secondary12, odd DATA5, filler23, checksum4.
        first[8] = 5;
        let sum: u32 = first[..22]
            .chunks_exact(2)
            .map(|w| u16::from_le_bytes([w[0], w[1]]) as u32)
            .sum();
        first[22..24].copy_from_slice(&(sum as u16).to_le_bytes());
        let bytes = [first, frame(28)].concat();
        let pool = BlockPool::with_defaults().unwrap();
        let policy = WholePolicy::new().with_hard_limit(32);
        let mut r = WholePacketReader::stream(Cursor::new(bytes), policy, pool.clone()).unwrap();
        let mut buf = PacketBuf::new(pool.clone()).unwrap();
        assert!(matches!(
            r.next_packet_into(&mut buf, deadline()).unwrap_err().reason,
            ErrorKind::Admission {
                source: AdmissionError::HardLimit { .. },
                ..
            }
        ));
        assert_eq!(r.position(), 24);
        assert_eq!(r.refused_header().unwrap().packet_length(), 68);
        let d = report(r.drain_next(deadline()).unwrap());
        assert!(r.refused_header().is_none());
        assert!(matches!(
            d.reason,
            DrainReason::HardLimit {
                length: 68,
                limit: 32,
                ..
            }
        ));
        assert_eq!(
            (d.offset, d.requested_bytes, d.consumed_bytes, d.disposition),
            (0, 68, 68, DrainDisposition::Drained)
        );
        assert_eq!(d.ranges.data, 36..41);
        assert_eq!(d.ranges.filler, 41..64);
        assert_eq!(d.ranges.checksum, 64..68);
        assert_eq!(d.ranges.data_checksum, DataChecksumState::NotChecked);
        assert_eq!(pool.stats().unwrap().allocated_blocks, 0);
        r.next_packet_into(&mut buf, deadline()).unwrap();
        assert_eq!(
            (
                buf.info().unwrap().offset,
                buf.info().unwrap().recovered_bytes
            ),
            (68, 0)
        );
        assert!(r.drain_next(deadline()).unwrap().is_none());
    });
}
#[test]
fn impossible_frame_and_active_full_pool_need_no_packet_credits() {
    support::watchdog(Duration::from_secs(5), || {
        let pool = BlockPool::new(BLOCK_BYTES + 131072).unwrap();
        let mut r = WholePacketReader::stream(
            Cursor::new(frame(2 * BLOCK_BYTES + 28)),
            WholePolicy::new(),
            pool.clone(),
        )
        .unwrap();
        let before = pool.stats().unwrap().used_bytes;
        let d = report(r.drain_next(deadline()).unwrap());
        assert!(matches!(d.reason,DrainReason::BudgetTooSmall{needed,limit,..} if needed>limit));
        assert_eq!(d.disposition, DrainDisposition::Drained);
        assert_eq!(pool.stats().unwrap().used_bytes, before);
        assert_eq!(pool.stats().unwrap().allocated_blocks, 0);
        let pool = BlockPool::new(2 * BLOCK_BYTES + 131072).unwrap();
        let mut r =
            WholePacketReader::stream(Cursor::new(frame(48)), WholePolicy::new(), pool.clone())
                .unwrap();
        let mut build = pool.try_begin(2).unwrap();
        let held = [
            build.try_next_block().unwrap(),
            build.try_next_block().unwrap(),
        ];
        let before = pool.stats().unwrap().used_bytes;
        let d = report(r.drain_next(deadline()).unwrap());
        assert_eq!(d.reason, DrainReason::CallerRequested);
        assert_eq!(pool.stats().unwrap().used_bytes, before);
        assert_eq!(pool.stats().unwrap().waiting, 0);
        drop((held, build));
    });
}
struct Fault {
    bytes: Cursor<Vec<u8>>,
    fail_at: u64,
    kind: io::ErrorKind,
    pool: Option<BlockPool>,
    interrupt: bool,
}
impl Read for Fault {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.interrupt {
            self.interrupt = false;
            return Err(io::ErrorKind::Interrupted.into());
        }
        let pos = self.bytes.position();
        if pos >= self.fail_at {
            if let Some(p) = &self.pool {
                p.cancel().unwrap();
                return Err(io::ErrorKind::Interrupted.into());
            }
            return Err(self.kind.into());
        }
        let boundary = if pos < 24 { 24 } else { self.fail_at };
        let n = out.len().min(3).min((boundary - pos) as usize);
        self.interrupt = true;
        self.bytes.read(&mut out[..n])
    }
}
#[test]
fn io_cancel_and_eof_preserve_progress_and_never_rescan_body() {
    support::watchdog(Duration::from_secs(5), || {
        for cancel in [false, true] {
            let pool = BlockPool::with_defaults().unwrap();
            let input = Fault {
                bytes: Cursor::new(frame(68)),
                fail_at: 31,
                kind: io::ErrorKind::PermissionDenied,
                pool: cancel.then(|| pool.clone()),
                interrupt: true,
            };
            let mut r = WholePacketReader::stream(input, WholePolicy::new(), pool).unwrap();
            let e = r.drain_next(deadline()).unwrap_err();
            let d = e.report.unwrap();
            assert_eq!(
                (d.consumed_bytes, d.requested_bytes, d.disposition),
                (31, 68, DrainDisposition::Aborted)
            );
            assert_eq!(e.error.position, 31);
            assert!(if cancel {
                matches!(
                    e.error.reason,
                    ErrorKind::Pool {
                        source: PoolError::Cancelled,
                        ..
                    }
                )
            } else {
                e.error.kind() == io::ErrorKind::PermissionDenied
            });
            assert!(r.drain_next(deadline()).unwrap().is_none());
        }
        let pool = BlockPool::with_defaults().unwrap();
        let input = Fault {
            bytes: Cursor::new(vec![0xff; 64]),
            fail_at: 31,
            kind: io::ErrorKind::PermissionDenied,
            pool: Some(pool.clone()),
            interrupt: true,
        };
        let mut r = WholePacketReader::stream(input, WholePolicy::new(), pool).unwrap();
        let e = r.drain_next(deadline()).unwrap_err();
        assert!(e.report.is_none());
        assert!(matches!(
            e.error.reason,
            ErrorKind::Pool {
                source: PoolError::Cancelled,
                ..
            }
        ));
        assert_eq!(
            (e.error.position, e.error.recovery.unwrap().scanned_bytes),
            (31, 7)
        );
        let mut short = frame(160);
        short[40..68].copy_from_slice(&frame(28));
        short.truncate(80);
        let pool = BlockPool::with_defaults().unwrap();
        let mut r =
            WholePacketReader::stream(Cursor::new(short), WholePolicy::new(), pool).unwrap();
        let d = report(r.drain_next(deadline()).unwrap());
        assert_eq!(
            (d.requested_bytes, d.consumed_bytes, d.disposition),
            (160, 80, DrainDisposition::Truncated)
        );
        assert!(r.drain_next(deadline()).unwrap().is_none());
    });
}
#[test]
fn recovery_and_structural_refusals_keep_typed_evidence() {
    let pool = BlockPool::with_defaults().unwrap();
    let mut r = WholePacketReader::stream(
        Cursor::new([vec![0xff; 7], frame(48)].concat()),
        WholePolicy::new(),
        pool.clone(),
    )
    .unwrap();
    let d = report(r.drain_next(deadline()).unwrap());
    assert_eq!((d.offset, d.recovery.unwrap().scanned_bytes), (7, 7));
    assert!(r.drain_next(deadline()).unwrap().is_none()); // Drained bytes establish framing, not inventory.
    let mut invalid = frame(28);
    invalid[4] = 30;
    let sum: u32 = invalid[..22]
        .chunks_exact(2)
        .map(|w| u16::from_le_bytes([w[0], w[1]]) as u32)
        .sum();
    invalid[22..24].copy_from_slice(&(sum as u16).to_le_bytes());
    let mut r =
        WholePacketReader::stream(Cursor::new(invalid), WholePolicy::new(), pool.clone()).unwrap();
    let e = r.drain_next(deadline()).unwrap_err();
    assert!(e.report.is_none());
    assert!(matches!(
        e.error.reason,
        ErrorKind::Header {
            source: HeaderError::UnalignedPacket { length: 30 },
            ..
        }
    ));
    let mut r = WholePacketReader::stream(
        Cursor::new([vec![0xff; 7], frame(48)].concat()),
        WholePolicy::new().with_recovery_bytes(Some(6)),
        pool,
    )
    .unwrap();
    let e = r.drain_next(deadline()).unwrap_err();
    assert!(matches!(
        e.error.reason,
        ErrorKind::RecoveryLimit { limit: 6, .. }
    ));
    assert_eq!(e.error.recovery.unwrap().scanned_bytes, 6);
}
struct Stable {
    bytes: Cursor<Vec<u8>>,
    mutation_at: Option<u64>,
}
impl Read for Stable {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let n = out.len().min(24);
        self.bytes.read(&mut out[..n])
    }
}
impl Seek for Stable {
    fn seek(&mut self, p: SeekFrom) -> io::Result<u64> {
        self.bytes.seek(p)
    }
}
impl StableSource for Stable {
    fn source_stamp(&mut self) -> io::Result<SourceStamp> {
        Ok(SourceStamp::new(
            self.bytes.get_ref().len() as u64,
            1,
            u128::from(
                self.mutation_at
                    .is_some_and(|at| self.bytes.position() >= at),
            ),
        ))
    }
}
#[test]
fn source_change_aborts_and_expired_deadline_reads_nothing() {
    let pool = BlockPool::with_defaults().unwrap();
    let mut r = WholePacketReader::seek_checked(
        Stable {
            bytes: Cursor::new(frame(48)),
            mutation_at: Some(25),
        },
        WholePolicy::new(),
        pool.clone(),
    )
    .unwrap();
    let e = r.drain_next(deadline()).unwrap_err();
    assert!(matches!(
        e.error.reason,
        ErrorKind::Admission {
            source: AdmissionError::SourceChanged,
            ..
        }
    ));
    assert_eq!(e.report.unwrap().disposition, DrainDisposition::Aborted);
    let mut r =
        WholePacketReader::stream(Cursor::new(frame(48)), WholePolicy::new(), pool).unwrap();
    let e = r.drain_next(Instant::now()).unwrap_err();
    assert!(matches!(
        e.error.reason,
        ErrorKind::Pool {
            source: PoolError::DeadlineExceeded,
            ..
        }
    ));
    assert_eq!(r.position(), 0);
    assert!(e.report.is_none());
}

#[test]
fn huge_claim_on_short_input_uses_u64_progress_on_32_bit_too() {
    let mut bytes = frame(28);
    bytes[4..8].copy_from_slice(&0xffff_fffcu32.to_le_bytes());
    let sum: u32 = bytes[..22]
        .chunks_exact(2)
        .map(|w| u16::from_le_bytes([w[0], w[1]]) as u32)
        .sum();
    bytes[22..24].copy_from_slice(&(sum as u16).to_le_bytes());
    let pool = BlockPool::with_defaults().unwrap();
    let mut r = WholePacketReader::stream(
        Cursor::new(bytes),
        WholePolicy::new().with_hard_limit(u32::MAX),
        pool.clone(),
    )
    .unwrap();
    let e = r
        .next_packet_into(&mut PacketBuf::new(pool.clone()).unwrap(), deadline())
        .unwrap_err();
    assert!(if cfg!(target_pointer_width = "32") {
        matches!(
            e.reason,
            ErrorKind::Admission {
                source: AdmissionError::TargetLimit,
                ..
            }
        )
    } else {
        matches!(
            e.reason,
            ErrorKind::Pool {
                source: PoolError::BudgetTooSmall { .. },
                ..
            }
        )
    });
    assert_eq!(r.position(), 24);
    assert_eq!(r.refused_header().unwrap().packet_length(), 0xffff_fffc);
    let d = report(r.drain_next(deadline()).unwrap());
    assert!(r.refused_header().is_none());
    assert_eq!(
        (d.requested_bytes, d.consumed_bytes, d.disposition),
        (0xffff_fffc, 28, DrainDisposition::Truncated)
    );
    assert!(if cfg!(target_pointer_width = "32") {
        matches!(d.reason, DrainReason::TargetLimit)
    } else {
        matches!(d.reason, DrainReason::BudgetTooSmall { .. })
    });
    assert_eq!(pool.stats().unwrap().allocated_blocks, 0);
}

#[test]
fn hard_ceiling_is_inclusive_for_explicit_drain_reason() {
    support::watchdog(Duration::from_secs(5), || {
        const H: u32 = 68;
        for p in [H, H + 4] {
            let pool = BlockPool::with_defaults().unwrap();
            let mut r = WholePacketReader::stream(
                Cursor::new(frame(p as usize)),
                WholePolicy::new().with_hard_limit(H),
                pool,
            )
            .unwrap();
            let d = report(r.drain_next(deadline()).unwrap());
            assert_eq!(
                (d.requested_bytes, d.consumed_bytes, d.disposition),
                (u64::from(p), u64::from(p), DrainDisposition::Drained)
            );
            if p == H {
                assert_eq!(d.reason, DrainReason::CallerRequested);
            } else {
                assert!(matches!(d.reason,DrainReason::HardLimit{length,limit,..}
                    if length==H+4 && limit==H));
            }
        }
    });
}

#[test]
fn source_mutation_after_first_quantum_aborts_before_reading_remainder() {
    support::watchdog(Duration::from_secs(5), || {
        const QUANTUM: usize = 4096;
        let requested = 24 + 3 * QUANTUM;
        let pool = BlockPool::with_defaults().unwrap();
        let input = Stable {
            bytes: Cursor::new(frame(requested)),
            mutation_at: Some((24 + QUANTUM) as u64),
        };
        let mut r = WholePacketReader::seek_checked(input, WholePolicy::new(), pool).unwrap();
        let e = r.drain_next(deadline()).unwrap_err();
        assert!(matches!(
            e.error.reason,
            ErrorKind::Admission {
                source: AdmissionError::SourceChanged,
                ..
            }
        ));
        let d = e.report.unwrap();
        assert_eq!(d.disposition, DrainDisposition::Aborted);
        assert_eq!(d.requested_bytes, requested as u64);
        assert_eq!(d.consumed_bytes, (24 + QUANTUM) as u64);
        assert!(d.consumed_bytes < d.requested_bytes);
        assert_eq!(e.error.position, d.offset + d.consumed_bytes);
        assert!(r.drain_next(deadline()).unwrap().is_none());
    });
}

#[test]
fn live_size_refusal_then_explicit_drain_then_next_packet() {
    support::watchdog(Duration::from_secs(5), || {
        for (p, hard, budget) in [
            (68, 32, DEFAULT_POOL_BUDGET),
            (
                2 * BLOCK_BYTES + 28,
                DEFAULT_HARD_LIMIT,
                BLOCK_BYTES + 131072,
            ),
        ] {
            let input = [frame(p), frame(28)].concat();
            let pool = BlockPool::new(budget).unwrap();
            let mut r = WholePacketReader::stream(
                Cursor::new(input),
                WholePolicy::new()
                    .with_hard_limit(hard)
                    .with_read_ahead_bytes(24),
                pool.clone(),
            )
            .unwrap();
            let mut buf = PacketBuf::new(pool.clone()).unwrap();
            let first = r.next_packet_into(&mut buf, deadline()).unwrap_err();
            assert!(matches!(
                first.reason,
                ErrorKind::Admission {
                    source: AdmissionError::HardLimit { .. },
                    ..
                } | ErrorKind::Pool {
                    source: PoolError::BudgetTooSmall { .. },
                    ..
                }
            ));
            assert_eq!(r.position(), 24);
            assert!(buf.info().is_none());
            assert_eq!(pool.stats().unwrap().allocated_blocks, 0);
            let repeated = r.next_packet_into(&mut buf, deadline()).unwrap_err();
            assert_eq!(repeated.position, 24); // Same header, never automatic skip.
            let d = report(r.drain_next(deadline()).unwrap());
            assert_eq!(
                (d.offset, d.consumed_bytes, d.disposition),
                (0, p as u64, DrainDisposition::Drained)
            );
            assert_eq!(pool.stats().unwrap().allocated_blocks, 0);
            assert!(matches!(
                r.next_packet_into(&mut buf, deadline()).unwrap(),
                Some(WholeEvent::Packet)
            ));
            assert_eq!(buf.info().unwrap().offset, p as u64);
        }
    });
}
#[test]
fn refusal_then_stop_leaves_live_body_unread() {
    struct Live {
        bytes: Cursor<Vec<u8>>,
    }
    impl Read for Live {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            self.bytes.read(out)
        }
    }
    let pool = BlockPool::with_defaults().unwrap();
    let mut r = WholePacketReader::stream(
        Live {
            bytes: Cursor::new([frame(68), frame(28)].concat()),
        },
        WholePolicy::new()
            .with_hard_limit(32)
            .with_read_ahead_bytes(24),
        pool.clone(),
    )
    .unwrap();
    let mut buf = PacketBuf::new(pool.clone()).unwrap();
    r.next_packet_into(&mut buf, deadline()).unwrap_err();
    assert_eq!(r.position(), 24);
    assert_eq!(pool.stats().unwrap().allocated_blocks, 0);
    assert_eq!(r.into_inner().bytes.position(), 24); // No body consumption or automatic drain.
}

#[test]
fn positioned_recovery_survives_retry_and_expired_explicit_drain() {
    let pool = BlockPool::with_defaults().unwrap();
    let input = [vec![0xff; 7], frame(68), frame(28)].concat();
    let mut r = WholePacketReader::stream(
        Cursor::new(input),
        WholePolicy::new().with_hard_limit(32),
        pool.clone(),
    )
    .unwrap();
    let mut buf = PacketBuf::new(pool).unwrap();
    for _ in 0..2 {
        let e = r.next_packet_into(&mut buf, deadline()).unwrap_err();
        assert_eq!(e.position, 31);
        assert_eq!(e.recovery.unwrap().scanned_bytes, 7);
        assert!(r.refused_header().is_some());
    }
    let e = r.drain_next(Instant::now()).unwrap_err();
    assert_eq!(e.error.recovery.unwrap().scanned_bytes, 7);
    let d = e.report.unwrap();
    assert_eq!(
        (
            d.offset,
            d.consumed_bytes,
            d.recovery.unwrap().scanned_bytes
        ),
        (7, 24, 7)
    );
    assert!(r.refused_header().is_none());
    assert!(r.next_packet_into(&mut buf, deadline()).unwrap().is_none());
}
#[cfg(target_pointer_width = "32")]
#[test]
#[ignore = "4 GiB synthetic drain; explicitly executed only in 32-bit GitHub CI"]
fn target_refusal_then_full_drain_then_next_packet() {
    support::watchdog(Duration::from_secs(90), || {
        struct Large {
            pos: u64,
            first: [u8; 24],
            next: [u8; 24],
        }
        const P: u64 = 0xffff_fffc;
        impl Read for Large {
            fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
                if self.pos >= P + 28 {
                    return Ok(0);
                }
                let (relative, end, header) = if self.pos < P {
                    (self.pos, P, &self.first)
                } else {
                    (self.pos - P, 28, &self.next)
                };
                let n = out.len().min(
                    (if relative < 24 {
                        24 - relative
                    } else {
                        end - relative
                    }) as usize,
                );
                if relative < 24 {
                    out[..n].copy_from_slice(&header[relative as usize..relative as usize + n]);
                } else {
                    out[..n].fill(0);
                }
                self.pos += n as u64;
                Ok(n)
            }
        }
        let next: [u8; 24] = frame(28)[..24].try_into().unwrap();
        let mut first = next;
        first[4..8].copy_from_slice(&(P as u32).to_le_bytes());
        let sum: u32 = first[..22]
            .chunks_exact(2)
            .map(|w| u16::from_le_bytes([w[0], w[1]]) as u32)
            .sum();
        first[22..24].copy_from_slice(&(sum as u16).to_le_bytes());
        let pool = BlockPool::new(BLOCK_BYTES + 131072).unwrap();
        let mut r = WholePacketReader::stream(
            Large {
                pos: 0,
                first,
                next,
            },
            WholePolicy::new()
                .with_hard_limit(u32::MAX)
                .with_read_ahead_bytes(24),
            pool.clone(),
        )
        .unwrap();
        let mut buf = PacketBuf::new(pool.clone()).unwrap();
        assert!(matches!(
            r.next_packet_into(&mut buf, deadline()).unwrap_err().reason,
            ErrorKind::Admission {
                source: AdmissionError::TargetLimit,
                ..
            }
        ));
        let d = report(
            r.drain_next(Instant::now() + Duration::from_secs(60))
                .unwrap(),
        );
        assert_eq!(
            (d.reason, d.consumed_bytes, d.disposition),
            (DrainReason::TargetLimit, P, DrainDisposition::Drained)
        );
        assert_eq!(pool.stats().unwrap().allocated_blocks, 0);
        r.next_packet_into(&mut buf, deadline()).unwrap();
        assert_eq!(buf.info().unwrap().offset, P);
    });
}

#[test]
fn other_errors_remain_terminal_even_with_a_cached_size_refusal() {
    let pool = BlockPool::with_defaults().unwrap();
    let mut r = WholePacketReader::stream(
        Cursor::new(frame(68)),
        WholePolicy::new().with_hard_limit(32),
        pool.clone(),
    )
    .unwrap();
    r.next_packet_into(&mut PacketBuf::new(pool).unwrap(), deadline())
        .unwrap_err();
    assert!(r.refused_header().is_some());
    let mut wrong = PacketBuf::new(BlockPool::with_defaults().unwrap()).unwrap();
    assert!(matches!(
        r.next_packet_into(&mut wrong, deadline())
            .unwrap_err()
            .reason,
        ErrorKind::Pool {
            source: PoolError::InvalidRequest,
            ..
        }
    ));
    assert!(r.refused_header().is_none());
    assert!(r.drain_next(deadline()).unwrap().is_none());
    let pool = BlockPool::with_defaults().unwrap();
    let mut r = WholePacketReader::stream(
        Cursor::new(frame(BLOCK_BYTES + 4)),
        WholePolicy::new(),
        pool.clone(),
    )
    .unwrap();
    let mut buf = PacketBuf::new(pool).unwrap();
    assert!(matches!(
        r.next_packet_into(&mut buf, deadline()).unwrap_err().reason,
        ErrorKind::Admission {
            source: AdmissionError::Uncorroborated,
            ..
        }
    ));
    assert!(r.refused_header().is_none());
    assert!(r.drain_next(deadline()).unwrap().is_none());
}
